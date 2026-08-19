use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

        if let Some(run_dir) = self.run_dir(run_id) {
            let scripts_dir = run_dir.join("scripts");
            std::fs::create_dir_all(&scripts_dir)?;
            let args_json = serde_json::to_vec_pretty(args).map_err(io::Error::other)?;
            atomic_write_new(&run_dir.join("args.json"), &args_json)?;
            atomic_write_new(&script_revision_path(&run_dir, 0), script.as_bytes())?;
            atomic_write_replace(&run_dir.join("script.rhai"), script.as_bytes())?;
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
        let Some(run_dir) = self.run_dir(&state.run_id) else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
        atomic_write_replace(&run_dir.join("state.json"), &json)
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
        if let Some(run_dir) = self.run_dir(run_id) {
            if let Err(error) = atomic_write_replace(&run_dir.join("cleared"), b"") {
                tracing::warn!(run_id, %error, "failed to tombstone cleared workflow run");
            }
            if let Err(error) = std::fs::remove_file(run_dir.join("state.json"))
                && error.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(run_id, %error, "failed to remove workflow manifest during clear");
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

pub(crate) fn read_bounded_nofollow(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "workflow artifact is not a regular file: {}",
                path.display()
            ),
        ));
    }
    if metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "workflow artifact exceeds {limit} bytes: {}",
                path.display()
            ),
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("workflow artifact changed during open: {}", path.display()),
        ));
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "workflow artifact exceeds {limit} bytes: {}",
                path.display()
            ),
        ));
    }
    Ok(bytes)
}

fn atomic_write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("immutable workflow file already exists: {}", path.display()),
        ));
    }
    atomic_write(path, bytes, false)
}

fn atomic_write_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write(path, bytes, true)
}

fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "workflow path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "workflow path is not UTF-8"))?;
    let tmp = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if !replace && path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("immutable workflow file already exists: {}", path.display()),
            ));
        }
        #[cfg(windows)]
        if replace {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
