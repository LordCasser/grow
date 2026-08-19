use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::session::persistence::PersistenceMsg;

use super::tracker::WorkflowRunState;

pub(crate) const WORKFLOW_RUN_MANIFEST_VERSION: u8 = 5;
pub(crate) const MAX_RESTORED_WORKFLOW_RUNS: usize = 128;
pub(crate) const MAX_WORKFLOW_MANIFEST_BYTES: u64 = 512 * 1024;
pub(crate) const MAX_WORKFLOW_ARGS_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRunManifest {
    pub version: u8,
    pub state: WorkflowRunState,
    pub script_revision: u32,
}

#[derive(Debug, Clone)]
pub struct RestoredWorkflowRun {
    pub manifest: WorkflowRunManifest,
    pub script: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone)]
struct RunSource {
    script: String,
    args: serde_json::Value,
    revision: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowRunStore {
    session_dir: Option<PathBuf>,
    persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
    sources: Arc<parking_lot::Mutex<HashMap<String, RunSource>>>,
}

impl WorkflowRunStore {
    pub(crate) fn new(
        session_dir: Option<PathBuf>,
        persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
    ) -> Self {
        Self {
            session_dir,
            persistence_tx,
            sources: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn from_restored(
        session_dir: Option<PathBuf>,
        persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
        restored: Vec<RestoredWorkflowRun>,
        timeline: Option<&chat_state::Timeline>,
    ) -> (Self, Vec<WorkflowRunState>) {
        let store = Self::new(session_dir, persistence_tx);
        let mut states = Vec::with_capacity(restored.len());
        let mut repaired = Vec::new();
        {
            let mut sources = store.sources.lock();
            // Storage enumerates these in canonical Timeline spawn order.
            for run in restored {
                if run.manifest.version != WORKFLOW_RUN_MANIFEST_VERSION {
                    tracing::warn!(
                        version = run.manifest.version,
                        expected = WORKFLOW_RUN_MANIFEST_VERSION,
                        "ignoring unsupported Workflow manifest"
                    );
                    continue;
                }
                let run_id = run.manifest.state.run_id.clone();
                let Some(lifecycle) =
                    timeline.and_then(|timeline| timeline.workflow_lifecycle(&run_id))
                else {
                    tracing::warn!(%run_id, "ignoring Workflow manifest without a Timeline spawn fact");
                    continue;
                };
                let expected_journal = format!("workflows/{run_id}/journal.jsonl");
                if run.manifest.state.name != lifecycle.name
                    || run.manifest.state.objective != lifecycle.objective
                    || run.manifest.state.private != lifecycle.private
                    || run.manifest.state.journal_path.as_deref() != Some(expected_journal.as_str())
                {
                    tracing::warn!(%run_id, "ignoring Workflow manifest that does not match its Timeline spawn fact");
                    continue;
                }
                sources.insert(
                    run_id.clone(),
                    RunSource {
                        script: run.script,
                        args: run.args,
                        revision: run.manifest.script_revision,
                    },
                );
                let mut state = run.manifest.state;
                let (status, message, execution_was_open) = if lifecycle.open {
                    (
                        super::tracker::WorkflowRunStatus::Interrupted,
                        Some("process_interrupted".into()),
                        true,
                    )
                } else {
                    let status = lifecycle
                        .status
                        .expect("a closed Workflow execution has a terminal status");
                    (
                        super::tracker::WorkflowRunStatus::from_timeline(status),
                        lifecycle.message.clone(),
                        false,
                    )
                };
                if state.reconcile_lifecycle_after_restore(
                    lifecycle.execution_epoch,
                    status,
                    message,
                    execution_was_open,
                ) {
                    repaired.push(state.clone());
                }
                states.push(state);
            }
        }
        for state in repaired {
            if let Err(error) = store.persist_now(&state) {
                tracing::warn!(
                    run_id = %state.run_id,
                    %error,
                    "failed to persist repaired workflow restore state"
                );
            }
        }
        (store, states)
    }

    pub(crate) fn manifest_matches_timeline_spawn(
        manifest: &WorkflowRunManifest,
        timeline: Option<&chat_state::Timeline>,
    ) -> bool {
        if manifest.version != WORKFLOW_RUN_MANIFEST_VERSION {
            return false;
        }
        let state = &manifest.state;
        let Some(lifecycle) =
            timeline.and_then(|timeline| timeline.workflow_lifecycle(&state.run_id))
        else {
            return false;
        };
        let expected_journal = format!("workflows/{}/journal.jsonl", state.run_id);
        state.name == lifecycle.name
            && state.objective == lifecycle.objective
            && state.private == lifecycle.private
            && state.journal_path.as_deref() == Some(expected_journal.as_str())
    }

    pub(crate) fn register(
        &self,
        run_id: &str,
        script: &str,
        args: &serde_json::Value,
    ) -> io::Result<()> {
        validate_run_id(run_id)?;
        if self.sources.lock().contains_key(run_id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("workflow source already registered: {run_id}"),
            ));
        }

        if let Some(session_dir) = self.session_dir.as_deref() {
            let run_relative = Path::new("workflows").join(run_id);
            crate::session::storage::ContainedDirectory::open(
                session_dir,
                &run_relative.join("scripts"),
                "Workflow scripts directory",
                true,
            )?;
            let args_json = serde_json::to_vec_pretty(args).map_err(io::Error::other)?;
            if args_json.len() as u64 > MAX_WORKFLOW_ARGS_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Workflow args exceed {MAX_WORKFLOW_ARGS_BYTES} bytes"),
                ));
            }
            if script.len() as u64 > super::registry::MAX_WORKFLOW_SOURCE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Workflow source exceeds {} bytes",
                        super::registry::MAX_WORKFLOW_SOURCE_BYTES
                    ),
                ));
            }
            crate::session::storage::write_contained_new_durable(
                session_dir,
                &run_relative.join("args.json"),
                &args_json,
            )?;
            crate::session::storage::write_contained_new_durable(
                session_dir,
                &run_relative.join("scripts/0000.rhai"),
                script.as_bytes(),
            )?;
            crate::session::storage::write_contained_atomic_durable(
                session_dir,
                &run_relative.join("script.rhai"),
                script.as_bytes(),
            )?;
        }

        self.sources.lock().insert(
            run_id.to_owned(),
            RunSource {
                script: script.to_owned(),
                args: args.clone(),
                revision: 0,
            },
        );
        Ok(())
    }

    fn manifest_for(&self, state: &WorkflowRunState) -> Option<WorkflowRunManifest> {
        let revision = self
            .sources
            .lock()
            .get(&state.run_id)
            .map(|source| source.revision)?;
        Some(WorkflowRunManifest {
            version: WORKFLOW_RUN_MANIFEST_VERSION,
            state: state.clone(),
            script_revision: revision,
        })
    }

    pub(crate) fn persist_now(&self, state: &WorkflowRunState) -> io::Result<()> {
        let Some(manifest) = self.manifest_for(state) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "workflow state has no registered resume source",
            ));
        };
        let Some(session_dir) = self.session_dir.as_deref() else {
            return Ok(());
        };
        write_workflow_run_manifest(session_dir, &manifest)
    }

    pub(crate) fn persist(&self, state: &WorkflowRunState) -> io::Result<()> {
        let manifest = self.manifest_for(state).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "workflow state has no registered resume source",
            )
        })?;
        self.persistence_tx
            .send(PersistenceMsg::WorkflowRunState(manifest))
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "workflow persistence channel closed",
                )
            })
    }

    pub(crate) async fn persist_ack(&self, state: &WorkflowRunState) -> io::Result<()> {
        let manifest = self.manifest_for(state).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "workflow state has no registered resume source",
            )
        })?;
        let (respond_to, response) = oneshot::channel();
        self.persistence_tx
            .send(PersistenceMsg::WorkflowRunStateAndAck {
                manifest,
                respond_to,
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "workflow persistence channel closed",
                )
            })?;
        response.await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "workflow persistence actor dropped acknowledgement",
            )
        })?
    }

    pub(crate) fn remove(&self, run_id: &str) {
        self.sources.lock().remove(run_id);
        if let Some(session_dir) = self.session_dir.as_deref() {
            if let Err(error) = tombstone_workflow_run(session_dir, run_id) {
                tracing::warn!(run_id, %error, "failed to tombstone cleared workflow run");
            }
        }
        if self
            .persistence_tx
            .send(PersistenceMsg::DeleteWorkflowRunState(run_id.to_owned()))
            .is_err()
        {
            tracing::warn!(run_id, "workflow persistence channel closed during clear");
        }
    }

    pub(crate) fn script_for(&self, run_id: &str) -> Option<String> {
        self.sources
            .lock()
            .get(run_id)
            .map(|source| source.script.clone())
    }

    pub(crate) fn args_for(&self, run_id: &str) -> Option<serde_json::Value> {
        self.sources
            .lock()
            .get(run_id)
            .map(|source| source.args.clone())
    }

    pub(crate) fn script_copy_path(&self, run_id: &str) -> Option<PathBuf> {
        validate_run_id(run_id).ok()?;
        self.sources.lock().contains_key(run_id).then_some(())?;
        Some(self.run_dir(run_id)?.join("script.rhai"))
    }

    fn run_dir(&self, run_id: &str) -> Option<PathBuf> {
        self.session_dir
            .as_ref()
            .map(|dir| dir.join("workflows").join(run_id))
    }
}

pub(crate) fn validate_run_id(run_id: &str) -> io::Result<()> {
    if run_id.is_empty()
        || run_id.len() > chat_state::MAX_WORKFLOW_RUN_ID_BYTES
        || !run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid workflow run id",
        ));
    }
    Ok(())
}

pub(crate) fn script_revision_path(run_dir: &Path, revision: u32) -> PathBuf {
    run_dir.join("scripts").join(format!("{revision:04}.rhai"))
}

pub(crate) fn write_workflow_run_manifest(
    session_dir: &Path,
    manifest: &WorkflowRunManifest,
) -> io::Result<()> {
    let run_id = &manifest.state.run_id;
    validate_run_id(run_id)?;
    let run_relative = Path::new("workflows").join(run_id);
    let run_dir = crate::session::storage::ContainedDirectory::open(
        session_dir,
        &run_relative,
        "Workflow run directory",
        true,
    )?;
    #[cfg(any(unix, windows))]
    let lock = lock_workflow_state(&run_dir)?;
    #[cfg(any(unix, windows))]
    let cleared = match run_dir.read_bounded(
        std::ffi::OsStr::new("cleared"),
        "Workflow cleared marker",
        0,
    ) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    #[cfg(not(any(unix, windows)))]
    let cleared = false;
    if cleared {
        return Ok(());
    }
    #[cfg(any(unix, windows))]
    match run_dir.read_bounded(
        std::ffi::OsStr::new("state.json"),
        "Workflow manifest",
        MAX_WORKFLOW_MANIFEST_BYTES,
    ) {
        Ok(existing) => {
            let on_disk: WorkflowRunManifest = serde_json::from_slice(&existing)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if on_disk.state.run_id != *run_id {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Workflow manifest run id does not match its directory",
                ));
            }
            if on_disk.state.revision > manifest.state.revision {
                tracing::debug!(
                    %run_id,
                    on_disk_revision = on_disk.state.revision,
                    incoming_revision = manifest.state.revision,
                    "skipping stale Workflow manifest write"
                );
                return Ok(());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let json = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    if json.len() as u64 > MAX_WORKFLOW_MANIFEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Workflow manifest exceeds {MAX_WORKFLOW_MANIFEST_BYTES} bytes"),
        ));
    }
    #[cfg(any(unix, windows))]
    let result = run_dir.write_atomic(std::ffi::OsStr::new("state.json"), &json, true, true);
    #[cfg(any(unix, windows))]
    {
        let _ = lock.unlock();
        result
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (run_dir, json);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle-relative Workflow storage is unsupported on this platform",
        ))
    }
}

pub(crate) fn tombstone_workflow_run(session_dir: &Path, run_id: &str) -> io::Result<()> {
    validate_run_id(run_id)?;
    let run_relative = Path::new("workflows").join(run_id);
    let run_dir = crate::session::storage::ContainedDirectory::open(
        session_dir,
        &run_relative,
        "Workflow run directory",
        true,
    )?;
    #[cfg(any(unix, windows))]
    {
        let lock = lock_workflow_state(&run_dir)?;
        let result = (|| {
            run_dir.write_atomic(std::ffi::OsStr::new("cleared"), b"", true, true)?;
            match run_dir.remove_file(std::ffi::OsStr::new("state.json"), true) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            Ok(())
        })();
        let _ = lock.unlock();
        result
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = run_dir;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle-relative Workflow storage is unsupported on this platform",
        ))
    }
}

#[cfg(any(unix, windows))]
fn lock_workflow_state(
    run_dir: &crate::session::storage::ContainedDirectory,
) -> io::Result<std::fs::File> {
    let lock = run_dir.open_read_write_create(std::ffi::OsStr::new("state.lock"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for Workflow state lock",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::workflow::tracker::WorkflowTracker;

    fn timeline_with_workflow(run_id: &str, name: &str, objective: &str) -> chat_state::Timeline {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: run_id.into(),
                    execution_epoch: 0,
                    name: name.into(),
                    objective: objective.into(),
                    private: false,
                },
            ))
            .unwrap();
        timeline
    }

    #[test]
    fn script_and_args_are_immutable() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let store = WorkflowRunStore::new(Some(dir.path().to_path_buf()), tx);
        let args = serde_json::json!({"objective": "ship"});

        store.register("wf_1", "complete(1);", &args).unwrap();
        std::fs::write(
            dir.path().join("workflows/wf_1/script.rhai"),
            "complete(2);",
        )
        .unwrap();

        let run_dir = dir.path().join("workflows/wf_1");
        assert_eq!(
            std::fs::read_to_string(run_dir.join("scripts/0000.rhai")).unwrap(),
            "complete(1);"
        );
        assert!(!run_dir.join("scripts/0001.rhai").exists());
        assert_eq!(store.script_for("wf_1").as_deref(), Some("complete(1);"));
        assert_eq!(store.args_for("wf_1"), Some(args));
    }

    #[cfg(unix)]
    #[test]
    fn register_rejects_symlinked_workflow_root() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), dir.path().join("workflows")).unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let store = WorkflowRunStore::new(Some(dir.path().to_path_buf()), tx);

        let error = store
            .register("wf_1", "complete(1);", &serde_json::json!({}))
            .expect_err("Workflow writes must not traverse a symlinked root");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn acknowledged_persist_returns_storage_failure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let store = WorkflowRunStore::new(None, tx);
        store
            .register("wf_1", "complete(1);", &serde_json::json!({}))
            .unwrap();
        let state = WorkflowTracker::default().start_run(
            "wf_1".into(),
            "demo".into(),
            "objective".into(),
            Vec::new(),
            None,
            None,
        );
        let writer = tokio::spawn(async move {
            let Some(PersistenceMsg::WorkflowRunStateAndAck { respond_to, .. }) = rx.recv().await
            else {
                panic!("expected acknowledged workflow manifest");
            };
            let _ = respond_to.send(Err(io::Error::other("disk full")));
        });

        assert_eq!(
            store.persist_ack(&state).await.unwrap_err().to_string(),
            "disk full"
        );
        writer.await.unwrap();
    }

    fn manifest(run_id: &str, revision: u64) -> WorkflowRunManifest {
        let mut state = WorkflowTracker::default().start_run(
            run_id.into(),
            "demo".into(),
            "objective".into(),
            Vec::new(),
            Some(8),
            Some(format!("workflows/{run_id}/journal.jsonl")),
        );
        state.revision = revision;
        WorkflowRunManifest {
            version: WORKFLOW_RUN_MANIFEST_VERSION,
            state,
            script_revision: 0,
        }
    }

    #[test]
    fn stale_workflow_manifest_cannot_overwrite_newer_revision() {
        let dir = tempfile::tempdir().unwrap();
        let newer = manifest("wf_revision", 7);
        let stale = manifest("wf_revision", 6);
        write_workflow_run_manifest(dir.path(), &newer).unwrap();
        write_workflow_run_manifest(dir.path(), &stale).unwrap();

        let persisted: WorkflowRunManifest = serde_json::from_slice(
            &std::fs::read(dir.path().join("workflows/wf_revision/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.state.revision, 7);
    }

    #[test]
    fn workflow_tombstone_prevents_manifest_recreation() {
        let dir = tempfile::tempdir().unwrap();
        let first = manifest("wf_cleared", 1);
        write_workflow_run_manifest(dir.path(), &first).unwrap();
        tombstone_workflow_run(dir.path(), "wf_cleared").unwrap();
        write_workflow_run_manifest(dir.path(), &manifest("wf_cleared", 2)).unwrap();

        let run_dir = dir.path().join("workflows/wf_cleared");
        assert!(run_dir.join("cleared").exists());
        assert!(!run_dir.join("state.json").exists());
    }

    #[test]
    fn active_manifest_is_interrupted_when_no_live_executor_survives_restore() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = WorkflowTracker::default().start_run(
            "wf_active".into(),
            "deep-research".into(),
            "objective".into(),
            Vec::new(),
            Some(8),
            Some("workflows/wf_active/journal.jsonl".into()),
        );
        let original_revision = state.revision;
        let restored = RestoredWorkflowRun {
            manifest: WorkflowRunManifest {
                version: WORKFLOW_RUN_MANIFEST_VERSION,
                state,
                script_revision: 0,
            },
            script: "complete(1);".into(),
            args: serde_json::json!({}),
        };

        let timeline = timeline_with_workflow("wf_active", "deep-research", "objective");
        let (_store, states) =
            WorkflowRunStore::from_restored(None, tx, vec![restored], Some(&timeline));
        let state = &states[0];
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted
        );
        assert!(state.revision > original_revision);
        assert!(state.agent_usage_incomplete);
        assert!(
            state
                .pause_message
                .as_deref()
                .is_some_and(|message| message == "process_interrupted")
        );
    }

    #[test]
    fn restore_uses_timeline_epoch_instead_of_stale_manifest_epoch() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = WorkflowTracker::default().start_run(
            "wf_resume".into(),
            "demo".into(),
            "objective".into(),
            Vec::new(),
            Some(8),
            Some("workflows/wf_resume/journal.jsonl".into()),
        );
        let restored = RestoredWorkflowRun {
            manifest: WorkflowRunManifest {
                version: WORKFLOW_RUN_MANIFEST_VERSION,
                state,
                script_revision: 0,
            },
            script: "complete(1);".into(),
            args: serde_json::json!({}),
        };
        let mut timeline = timeline_with_workflow("wf_resume", "demo", "objective");
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: "wf_resume".into(),
                    execution_epoch: 0,
                    status: chat_state::WorkflowExecutionStatus::Failed,
                    duration_ms: 1,
                    message: Some("retry".into()),
                },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Resumed {
                    run_id: "wf_resume".into(),
                    execution_epoch: 1,
                },
            ))
            .unwrap();

        let (_store, states) =
            WorkflowRunStore::from_restored(None, tx, vec![restored], Some(&timeline));
        assert_eq!(states[0].execution_epoch, 1);
        assert_eq!(
            states[0].status,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted
        );
    }

    #[test]
    fn unsupported_manifest_is_not_restored() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = WorkflowTracker::default().start_run(
            "wf_legacy".into(),
            "demo".into(),
            "objective".into(),
            Vec::new(),
            Some(1_000),
            None,
        );
        let restored = RestoredWorkflowRun {
            manifest: WorkflowRunManifest {
                version: WORKFLOW_RUN_MANIFEST_VERSION - 1,
                state,
                script_revision: 0,
            },
            script: "complete(1);".into(),
            args: serde_json::json!({}),
        };

        let (_store, states) = WorkflowRunStore::from_restored(None, tx, vec![restored], None);
        assert!(states.is_empty());
    }
}
