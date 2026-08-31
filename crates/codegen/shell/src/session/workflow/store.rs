use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::session::persistence::PersistenceMsg;

use super::tracker::WorkflowRunState;

pub(crate) const WORKFLOW_RUN_MANIFEST_VERSION: u8 = 6;
pub(crate) const MAX_RESTORED_WORKFLOW_RUNS: usize = 128;
pub(crate) const MAX_WORKFLOW_MANIFEST_BYTES: u64 =
    chat_state::MAX_WORKFLOW_INITIAL_MANIFEST_BYTES as u64;
pub(crate) const MAX_WORKFLOW_ARGS_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    session_directory: Option<Arc<crate::session::storage::ContainedDirectory>>,
    persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
    sources: Arc<parking_lot::Mutex<HashMap<String, RunSource>>>,
}

impl WorkflowRunStore {
    pub(crate) fn new(
        session_directory: Option<Arc<crate::session::storage::ContainedDirectory>>,
        persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
    ) -> Self {
        Self {
            session_directory,
            persistence_tx,
            sources: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn from_restored(
        session_directory: Option<Arc<crate::session::storage::ContainedDirectory>>,
        persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
        restored: Vec<RestoredWorkflowRun>,
        timeline: Option<&chat_state::Timeline>,
    ) -> (Self, Vec<WorkflowRunState>) {
        let store = Self::new(session_directory, persistence_tx);
        let mut states = Vec::with_capacity(restored.len());
        let mut repaired = Vec::new();
        {
            let mut sources = store.sources.lock();
            // Storage enumerates these in canonical Timeline spawn order.
            for run in restored {
                let RestoredWorkflowRun {
                    manifest,
                    script,
                    args,
                } = run;
                let run_id = manifest.state.run_id.clone();
                let Some(lifecycle) =
                    timeline.and_then(|timeline| timeline.workflow_lifecycle(&run_id))
                else {
                    tracing::warn!(%run_id, "ignoring Workflow manifest without a Timeline spawn fact");
                    continue;
                };
                let resolution = match resolve_workflow_restore_manifest(
                    &run_id,
                    &lifecycle,
                    Some(manifest),
                ) {
                    Ok(resolution) => resolution,
                    Err(error) => {
                        tracing::warn!(%run_id, %error, "ignoring Workflow with no valid Timeline-owned restore projection");
                        continue;
                    }
                };
                let source_revision = resolution.manifest.script_revision;
                let mut state = resolution.manifest.state;
                let was_repaired = reconcile_workflow_lifecycle(&mut state, &lifecycle);
                debug_assert!(state.validate_restored_projection().is_ok());
                sources.insert(
                    run_id.clone(),
                    RunSource {
                        script,
                        args,
                        revision: source_revision,
                    },
                );
                if was_repaired || resolution.used_timeline_seed {
                    repaired.push(state.clone());
                }
                states.push(state);
            }
        }
        for state in repaired {
            if let Err(error) = store.persist(&state) {
                tracing::warn!(
                    run_id = %state.run_id,
                    %error,
                    "failed to queue repaired workflow restore state"
                );
            }
        }
        (store, states)
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

        if let Some(session) = self.session_directory.as_deref() {
            let run_relative = Path::new("workflows").join(run_id);
            session.open_relative(
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
            let run_dir = session.open_relative(&run_relative, "Workflow run directory", false)?;
            let scripts =
                run_dir.open_relative(Path::new("scripts"), "Workflow scripts directory", false)?;
            run_dir.write_atomic(std::ffi::OsStr::new("args.json"), &args_json, true, false)?;
            scripts.write_atomic(
                std::ffi::OsStr::new("0000.rhai"),
                script.as_bytes(),
                true,
                false,
            )?;
            run_dir.write_atomic(
                std::ffi::OsStr::new("script.rhai"),
                script.as_bytes(),
                true,
                true,
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

    /// Validate the exact durable projection before its Timeline lifecycle is
    /// opened. The persistence writer calls the same encoder, so a Run cannot
    /// publish `Workflow::Spawned` and only then discover that its manifest is
    /// structurally too large to exist.
    pub(crate) fn validate_persistable(&self, state: &WorkflowRunState) -> io::Result<()> {
        self.initial_manifest(state).map(|_| ())
    }

    /// Build the exact credential-free projection embedded in the Timeline
    /// spawn fact. Mutable sidecar manifests are caches of this initial state
    /// plus later lifecycle and journal progress.
    pub(crate) fn initial_manifest(
        &self,
        state: &WorkflowRunState,
    ) -> io::Result<WorkflowRunManifest> {
        let manifest = self.manifest_for(state).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "workflow state has no registered resume source",
            )
        })?;
        encode_workflow_manifest(&manifest)?;
        Ok(manifest)
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
}

#[derive(Debug)]
pub(crate) struct WorkflowManifestResolution {
    pub manifest: WorkflowRunManifest,
    pub used_timeline_seed: bool,
}

/// Select a restore projection without consulting the filesystem. Timeline is
/// authoritative for the immutable seed and execution lifecycle; the sidecar
/// may contribute only mutable progress from the same frozen Run contract.
pub(crate) fn resolve_workflow_restore_manifest(
    run_id: &str,
    lifecycle: &chat_state::WorkflowLifecycle,
    sidecar: Option<WorkflowRunManifest>,
) -> io::Result<WorkflowManifestResolution> {
    validate_run_id(run_id)?;
    let initial =
        decode_workflow_manifest_value(lifecycle.initial_manifest.clone()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Workflow {run_id} has an invalid Timeline initial projection: {error}"),
            )
        })?;
    if !initial_manifest_matches_spawn(run_id, lifecycle, &initial) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Workflow {run_id} Timeline initial projection does not match its spawn fact"),
        ));
    }
    validate_reconciled_projection(&initial, lifecycle).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Workflow {run_id} Timeline initial projection is invalid: {error}"),
        )
    })?;

    if let Some(sidecar) = sidecar
        && frozen_manifest_matches(&sidecar, &initial, lifecycle.execution_epoch)
        && validate_reconciled_projection(&sidecar, lifecycle).is_ok()
    {
        return Ok(WorkflowManifestResolution {
            manifest: sidecar,
            used_timeline_seed: false,
        });
    }

    Ok(WorkflowManifestResolution {
        manifest: initial,
        used_timeline_seed: true,
    })
}

fn initial_manifest_matches_spawn(
    run_id: &str,
    lifecycle: &chat_state::WorkflowLifecycle,
    initial: &WorkflowRunManifest,
) -> bool {
    let state = &initial.state;
    let expected_journal = format!("workflows/{run_id}/journal.jsonl");
    initial.version == WORKFLOW_RUN_MANIFEST_VERSION
        && initial.script_revision == 0
        && state.run_id == run_id
        && state.name == lifecycle.name
        && state.objective == lifecycle.objective
        && state.execution_epoch == 0
        && state.status == super::tracker::WorkflowRunStatus::Active
        && state.turn_handoff == chat_state::WorkflowTurnHandoff::None
        && state.journal_path.as_deref() == Some(expected_journal.as_str())
}

fn frozen_manifest_matches(
    candidate: &WorkflowRunManifest,
    initial: &WorkflowRunManifest,
    lifecycle_epoch: u64,
) -> bool {
    let candidate_state = &candidate.state;
    let initial_state = &initial.state;
    candidate.version == initial.version
        && candidate.script_revision == initial.script_revision
        && candidate_state.revision >= initial_state.revision
        && candidate_state.execution_epoch <= lifecycle_epoch
        && candidate_state.run_id == initial_state.run_id
        && candidate_state.name == initial_state.name
        && candidate_state.objective == initial_state.objective
        && candidate_state.definition_id == initial_state.definition_id
        && candidate_state.definition_scope == initial_state.definition_scope
        && candidate_state.definition_hash == initial_state.definition_hash
        && candidate_state.runtime_route == initial_state.runtime_route
        && candidate_state.phases == initial_state.phases
        && candidate_state.journal_path == initial_state.journal_path
}

fn validate_reconciled_projection(
    manifest: &WorkflowRunManifest,
    lifecycle: &chat_state::WorkflowLifecycle,
) -> Result<(), &'static str> {
    let mut state = manifest.state.clone();
    reconcile_workflow_lifecycle(&mut state, lifecycle);
    state.validate_restored_projection()
}

fn reconcile_workflow_lifecycle(
    state: &mut WorkflowRunState,
    lifecycle: &chat_state::WorkflowLifecycle,
) -> bool {
    let (status, turn_handoff, message, execution_was_open) = if lifecycle.open {
        (
            super::tracker::WorkflowRunStatus::Interrupted,
            chat_state::WorkflowTurnHandoff::Completion,
            Some("process_interrupted".into()),
            true,
        )
    } else {
        (
            super::tracker::WorkflowRunStatus::from_timeline(
                lifecycle
                    .status
                    .expect("closed Workflow lifecycle has a terminal status"),
            ),
            lifecycle
                .handoff
                .expect("closed Workflow lifecycle has a turn handoff"),
            lifecycle.message.clone(),
            false,
        )
    };
    state.reconcile_lifecycle_after_restore(
        lifecycle.execution_epoch,
        status,
        turn_handoff,
        message,
        execution_was_open,
    )
}

pub(crate) fn workflow_script_hash(script: &str) -> String {
    blake3::hash(script.as_bytes()).to_hex().to_string()
}

pub(crate) fn workflow_args_hash(args: &serde_json::Value) -> io::Result<String> {
    let canonical = canonicalize_json(args);
    let bytes = serde_json::to_vec(&canonical).map_err(io::Error::other)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonicalize_json).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonicalize_json(value)))
                    .collect(),
            )
        }
        scalar => scalar.clone(),
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

fn encode_workflow_manifest(manifest: &WorkflowRunManifest) -> io::Result<Vec<u8>> {
    let json = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    if json.len() as u64 > MAX_WORKFLOW_MANIFEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Workflow manifest exceeds {MAX_WORKFLOW_MANIFEST_BYTES} bytes"),
        ));
    }
    Ok(json)
}

/// Decode the version envelope before the typed body. This keeps the v6
/// schema strict while making an older manifest fail with the architectural
/// incompatibility, rather than a misleading missing-field error.
pub(crate) fn decode_workflow_manifest(bytes: &[u8]) -> io::Result<WorkflowRunManifest> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Workflow manifest has no valid version",
            )
        })?;
    if version != u64::from(WORKFLOW_RUN_MANIFEST_VERSION) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "unsupported Workflow manifest version {version}; expected {WORKFLOW_RUN_MANIFEST_VERSION}"
            ),
        ));
    }
    serde_json::from_value(value).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn decode_workflow_manifest_value(
    value: serde_json::Value,
) -> io::Result<WorkflowRunManifest> {
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Workflow manifest has no valid version",
            )
        })?;
    if version != u64::from(WORKFLOW_RUN_MANIFEST_VERSION) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "unsupported Workflow manifest version {version}; expected {WORKFLOW_RUN_MANIFEST_VERSION}"
            ),
        ));
    }
    serde_json::from_value(value).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
pub(crate) fn write_workflow_run_manifest(
    session_dir: &Path,
    manifest: &WorkflowRunManifest,
) -> io::Result<()> {
    let session = crate::session::storage::ContainedDirectory::open(
        session_dir,
        Path::new(""),
        "Workflow session directory",
        false,
    )?;
    write_workflow_run_manifest_in_directory(&session, manifest)
}

pub(crate) fn write_workflow_run_manifest_in_directory(
    session: &crate::session::storage::ContainedDirectory,
    manifest: &WorkflowRunManifest,
) -> io::Result<()> {
    let run_id = &manifest.state.run_id;
    validate_run_id(run_id)?;
    let run_relative = Path::new("workflows").join(run_id);
    let run_dir = session.open_relative(&run_relative, "Workflow run directory", true)?;
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
            let on_disk = decode_workflow_manifest(&existing)?;
            if on_disk.state.run_id != *run_id {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Workflow manifest run id does not match its directory",
                ));
            }
            match on_disk.state.revision.cmp(&manifest.state.revision) {
                std::cmp::Ordering::Greater => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "stale Workflow manifest revision for {run_id}: persisted {}, incoming {}",
                            on_disk.state.revision, manifest.state.revision
                        ),
                    ));
                }
                std::cmp::Ordering::Equal if on_disk == *manifest => return Ok(()),
                std::cmp::Ordering::Equal => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "conflicting Workflow manifest content at revision {} for {run_id}",
                            manifest.state.revision
                        ),
                    ));
                }
                std::cmp::Ordering::Less => {}
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let json = encode_workflow_manifest(manifest)?;
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

#[cfg(test)]
pub(crate) fn tombstone_workflow_run(session_dir: &Path, run_id: &str) -> io::Result<()> {
    let session = crate::session::storage::ContainedDirectory::open(
        session_dir,
        Path::new(""),
        "Workflow session directory",
        false,
    )?;
    tombstone_workflow_run_in_directory(&session, run_id)
}

pub(crate) fn tombstone_workflow_run_in_directory(
    session: &crate::session::storage::ContainedDirectory,
    run_id: &str,
) -> io::Result<()> {
    validate_run_id(run_id)?;
    let run_relative = Path::new("workflows").join(run_id);
    let run_dir = session.open_relative(&run_relative, "Workflow run directory", true)?;
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

    fn test_session(path: &Path) -> Arc<crate::session::storage::ContainedDirectory> {
        Arc::new(
            crate::session::storage::ContainedDirectory::open(
                path,
                Path::new(""),
                "Workflow store test session",
                false,
            )
            .unwrap(),
        )
    }

    fn timeline_with_workflow(run_id: &str, name: &str, objective: &str) -> chat_state::Timeline {
        let state = WorkflowTracker::default().start_run(
            run_id.into(),
            name.into(),
            objective.into(),
            Vec::new(),
            None,
            Some(format!("workflows/{run_id}/journal.jsonl")),
            crate::session::workflow::tracker::test_runtime_route(),
        );
        let initial_manifest = WorkflowRunManifest {
            version: WORKFLOW_RUN_MANIFEST_VERSION,
            state,
            script_revision: 0,
        };
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: run_id.into(),
                    execution_epoch: 0,
                    name: name.into(),
                    objective: objective.into(),
                    script_hash: "0".repeat(64),
                    args_hash: "0".repeat(64),
                    initial_manifest: serde_json::to_value(initial_manifest).unwrap(),
                },
            ))
            .unwrap();
        timeline
    }

    #[test]
    fn script_and_args_are_immutable() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let store = WorkflowRunStore::new(Some(test_session(dir.path())), tx);
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
        let store = WorkflowRunStore::new(Some(test_session(dir.path())), tx);

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
            crate::session::workflow::tracker::test_runtime_route(),
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
    fn admission_uses_the_writer_manifest_size_limit() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let store = WorkflowRunStore::new(None, tx);
        store
            .register("wf_large", "complete(1);", &serde_json::json!({}))
            .unwrap();
        let state = WorkflowTracker::default().start_run(
            "wf_large".into(),
            "demo".into(),
            "x".repeat(MAX_WORKFLOW_MANIFEST_BYTES as usize),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );

        let error = store.validate_persistable(&state).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("Workflow manifest exceeds"));
    }

    fn manifest(run_id: &str, revision: u64) -> WorkflowRunManifest {
        let mut state = WorkflowTracker::default().start_run(
            run_id.into(),
            "demo".into(),
            "objective".into(),
            Vec::new(),
            Some(8),
            Some(format!("workflows/{run_id}/journal.jsonl")),
            crate::session::workflow::tracker::test_runtime_route(),
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
        let error = write_workflow_run_manifest(dir.path(), &stale).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let persisted: WorkflowRunManifest = serde_json::from_slice(
            &std::fs::read(dir.path().join("workflows/wf_revision/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.state.revision, 7);
    }

    #[test]
    fn equal_workflow_revision_is_idempotent_only_for_identical_content() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest("wf_equal_revision", 4);
        write_workflow_run_manifest(dir.path(), &manifest).unwrap();
        write_workflow_run_manifest(dir.path(), &manifest).unwrap();

        let mut conflicting = manifest.clone();
        conflicting.state.save_prompt = true;
        let error = write_workflow_run_manifest(dir.path(), &conflicting).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        let persisted: WorkflowRunManifest = serde_json::from_slice(
            &std::fs::read(dir.path().join("workflows/wf_equal_revision/state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted, manifest);
    }

    #[test]
    fn concurrent_workflow_manifest_writes_converge_on_highest_revision() {
        let dir = tempfile::tempdir().unwrap();
        let lower_dir = dir.path().to_path_buf();
        let higher_dir = lower_dir.clone();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let lower_barrier = barrier.clone();
        let higher_barrier = barrier;
        let (lower_result, higher_result) = std::thread::scope(|scope| {
            let lower = scope.spawn(move || {
                lower_barrier.wait();
                write_workflow_run_manifest(&lower_dir, &manifest("wf_concurrent", 6))
            });
            let higher = scope.spawn(move || {
                higher_barrier.wait();
                write_workflow_run_manifest(&higher_dir, &manifest("wf_concurrent", 7))
            });
            (lower.join().unwrap(), higher.join().unwrap())
        });
        higher_result.unwrap();
        if let Err(error) = lower_result {
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }

        let persisted: WorkflowRunManifest = serde_json::from_slice(
            &std::fs::read(dir.path().join("workflows/wf_concurrent/state.json")).unwrap(),
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
            crate::session::workflow::tracker::test_runtime_route(),
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
        assert_eq!(
            state.turn_handoff,
            chat_state::WorkflowTurnHandoff::Completion
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
    fn restore_uses_timeline_attention_handoff_instead_of_stale_manifest_projection() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = WorkflowTracker::default().start_run(
            "wf_attention".into(),
            "review".into(),
            "objective".into(),
            Vec::new(),
            Some(8),
            Some("workflows/wf_attention/journal.jsonl".into()),
            crate::session::workflow::tracker::test_runtime_route(),
        );
        let original_revision = state.revision;
        let restored = RestoredWorkflowRun {
            manifest: WorkflowRunManifest {
                version: WORKFLOW_RUN_MANIFEST_VERSION,
                state,
                script_revision: 0,
            },
            script: "await_user(\"back_off\", \"review\");".into(),
            args: serde_json::json!({}),
        };
        let mut timeline = timeline_with_workflow("wf_attention", "review", "objective");
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: "wf_attention".into(),
                    execution_epoch: 0,
                    status: chat_state::WorkflowExecutionStatus::BackOffPaused,
                    handoff: chat_state::WorkflowTurnHandoff::AttentionRequired,
                    duration_ms: 1,
                    message: Some("review".into()),
                },
            ))
            .unwrap();

        let (_store, states) =
            WorkflowRunStore::from_restored(None, tx, vec![restored], Some(&timeline));
        assert_eq!(states.len(), 1);
        assert_eq!(
            states[0].status,
            crate::session::workflow::tracker::WorkflowRunStatus::BackOffPaused
        );
        assert_eq!(
            states[0].turn_handoff,
            chat_state::WorkflowTurnHandoff::AttentionRequired
        );
        assert!(states[0].revision > original_revision);
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
            crate::session::workflow::tracker::test_runtime_route(),
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
                    handoff: chat_state::WorkflowTurnHandoff::Completion,
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
    fn semantically_invalid_manifest_falls_back_without_hiding_other_runs() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut invalid_manifest_json = serde_json::to_value(manifest("wf_invalid", 1)).unwrap();
        *invalid_manifest_json
            .pointer_mut("/state/runtime_route/samplers/test-model/contract_fingerprint")
            .expect("test manifest contains its sampler fingerprint") = serde_json::json!("");
        let invalid_manifest = serde_json::from_value(invalid_manifest_json)
            .expect("semantic damage remains valid serialized structure");
        let invalid = RestoredWorkflowRun {
            manifest: invalid_manifest,
            script: "complete(0);".into(),
            args: serde_json::json!({"invalid": true}),
        };
        let valid = RestoredWorkflowRun {
            manifest: manifest("wf_valid", 1),
            script: "complete(1);".into(),
            args: serde_json::json!({"valid": true}),
        };
        let mut timeline = timeline_with_workflow("wf_invalid", "demo", "objective");
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: "wf_valid".into(),
                    execution_epoch: 0,
                    name: "demo".into(),
                    objective: "objective".into(),
                    script_hash: "0".repeat(64),
                    args_hash: "0".repeat(64),
                    initial_manifest: serde_json::to_value(manifest("wf_valid", 1)).unwrap(),
                },
            ))
            .unwrap();

        let (store, states) =
            WorkflowRunStore::from_restored(None, tx, vec![invalid, valid], Some(&timeline));

        assert_eq!(
            states
                .iter()
                .map(|state| state.run_id.as_str())
                .collect::<Vec<_>>(),
            vec!["wf_invalid", "wf_valid"]
        );
        assert_eq!(
            store.script_for("wf_invalid").as_deref(),
            Some("complete(0);")
        );
        assert_eq!(
            store.script_for("wf_valid").as_deref(),
            Some("complete(1);")
        );
        assert!(WorkflowTracker::from_snapshot(states).is_ok());
    }

    #[test]
    fn semantically_invalid_sidecar_rebuilds_from_timeline_initial_projection() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let initial = manifest("wf_rebuilt", 1);
        let mut damaged_json = serde_json::to_value(initial.clone()).unwrap();
        *damaged_json
            .pointer_mut("/state/runtime_route/samplers/test-model/contract_fingerprint")
            .expect("test manifest contains its sampler fingerprint") = serde_json::json!("");
        let damaged = serde_json::from_value(damaged_json).unwrap();
        let restored = RestoredWorkflowRun {
            manifest: damaged,
            script: "complete(1);".into(),
            args: serde_json::json!({"restored": true}),
        };
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: "wf_rebuilt".into(),
                    execution_epoch: 0,
                    name: "demo".into(),
                    objective: "objective".into(),
                    script_hash: "0".repeat(64),
                    args_hash: "0".repeat(64),
                    initial_manifest: serde_json::to_value(initial).unwrap(),
                },
            ))
            .unwrap();

        let (store, states) =
            WorkflowRunStore::from_restored(None, tx, vec![restored], Some(&timeline));

        assert_eq!(states.len(), 1);
        assert_eq!(states[0].run_id, "wf_rebuilt");
        assert_eq!(
            states[0].status,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted
        );
        assert_eq!(
            store.script_for("wf_rebuilt").as_deref(),
            Some("complete(1);")
        );
    }

    #[test]
    fn frozen_sidecar_drift_falls_back_to_timeline_initial_projection() {
        let initial = manifest("wf_frozen", 1);
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: "wf_frozen".into(),
                    execution_epoch: 0,
                    name: "demo".into(),
                    objective: "objective".into(),
                    script_hash: "0".repeat(64),
                    args_hash: "0".repeat(64),
                    initial_manifest: serde_json::to_value(&initial).unwrap(),
                },
            ))
            .unwrap();
        let lifecycle = timeline.workflow_lifecycle("wf_frozen").unwrap();
        let mut drifted = initial.clone();
        drifted.script_revision = 1;

        let resolution =
            resolve_workflow_restore_manifest("wf_frozen", &lifecycle, Some(drifted)).unwrap();

        assert!(resolution.used_timeline_seed);
        assert_eq!(resolution.manifest, initial);
    }

    #[test]
    fn valid_sidecar_cannot_bypass_an_invalid_timeline_seed() {
        let sidecar = manifest("wf_seed", 1);
        let mut invalid_seed = serde_json::to_value(&sidecar).unwrap();
        invalid_seed["version"] = serde_json::json!(WORKFLOW_RUN_MANIFEST_VERSION - 1);
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: "wf_seed".into(),
                    execution_epoch: 0,
                    name: "demo".into(),
                    objective: "objective".into(),
                    script_hash: "0".repeat(64),
                    args_hash: "0".repeat(64),
                    initial_manifest: invalid_seed,
                },
            ))
            .unwrap();
        let lifecycle = timeline.workflow_lifecycle("wf_seed").unwrap();

        let error =
            resolve_workflow_restore_manifest("wf_seed", &lifecycle, Some(sidecar)).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("Timeline initial projection"));
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
            crate::session::workflow::tracker::test_runtime_route(),
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

    #[test]
    fn legacy_manifest_reports_version_before_decoding_the_v6_body() {
        let error = decode_workflow_manifest(
            br#"{"version":5,"state":{"run_id":"wf_legacy"},"script_revision":0}"#,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
        assert!(
            error
                .to_string()
                .contains("unsupported Workflow manifest version 5; expected 6")
        );
    }
}
