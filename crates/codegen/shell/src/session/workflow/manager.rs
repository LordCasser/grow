use std::collections::HashMap;
use std::io::{Read as _, Seek as _, Write as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fs2::FileExt as _;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use workflow::{Journal, WorkflowOutcome, WorkflowRunParams};

use super::host_service::{
    DiagnosticHook, HostDrainOutcome, WorkflowHostParams, spawn_workflow_host_service,
};
use super::notify::WorkflowNotifySender;
use super::registry::ResolvedWorkflow;
use super::store::WorkflowRunStore;
use super::tracker::{
    WorkflowAgentCatalogSource, WorkflowRunState, WorkflowRunStatus, WorkflowRuntimeRoute,
    WorkflowTracker,
};

pub(crate) const WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION: usize = 4;
pub(crate) const WORKFLOW_DEFAULT_AGENT_BUDGET: u64 = workflow::DEFAULT_AGENT_BUDGET;

struct ActiveRun {
    cancel: CancellationToken,
    pause_intent: Arc<AtomicBool>,
    suspend_intent: Arc<AtomicBool>,
    /// Resolves only after the watcher has settled the terminal state and
    /// committed the matching authoritative Timeline boundary. Control callers
    /// await this receiver instead of sampling the tracker while the watcher is
    /// settling.
    done: oneshot::Receiver<Result<WorkflowRunState, String>>,
}

struct SessionJournalStorage {
    run: crate::session::storage::ContainedDirectory,
}

impl workflow::JournalStorage for SessionJournalStorage {
    fn read_bounded(&self, max_bytes: u64) -> std::io::Result<Vec<u8>> {
        self.run.read_bounded(
            std::ffi::OsStr::new("journal.jsonl"),
            "Workflow journal",
            max_bytes,
        )
    }

    fn append(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut file = self
            .run
            .open_read_write_create(std::ffi::OsStr::new("journal.jsonl"))?;
        file.lock_exclusive()?;
        let result = (|| {
            let len = file.metadata()?.len();
            if len.saturating_add(bytes.len() as u64) > workflow::journal::MAX_JOURNAL_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Workflow journal exceeds its byte limit",
                ));
            }
            file.seek(std::io::SeekFrom::End(0))?;
            file.write_all(bytes)?;
            file.sync_data()?;
            // A newly-created journal is not durable until its directory entry
            // is synchronized.  This runs before the engine may dispatch the
            // operation whose intent was appended above.
            self.run.sync()
        })();
        let _ = file.unlock();
        result
    }

    fn truncate(&self, len: u64) -> std::io::Result<()> {
        let file = self
            .run
            .open_read_write_create(std::ffi::OsStr::new("journal.jsonl"))?;
        file.lock_exclusive()?;
        let result = (|| {
            if len > file.metadata()?.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Workflow journal truncate would extend the file",
                ));
            }
            file.set_len(len)?;
            file.sync_data()?;
            self.run.sync()
        })();
        let _ = file.unlock();
        result
    }
}

pub(crate) struct LaunchSpec {
    pub objective: String,
    pub args: serde_json::Value,
    pub agent_budget: Option<u64>,
    pub max_concurrency: Option<u16>,
    pub resume_run_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LaunchError {
    #[error("workflow run not found: {0}")]
    UnknownRun(String),
    #[error("journal error: {0}")]
    Journal(String),
    #[error("workflow store error: {0}")]
    Store(String),
    #[error("workflow Timeline error: {0}")]
    Timeline(String),
    #[error("run is not resumable (status: {0})")]
    NotResumable(String),
    #[error(
        "run is budget-limited at {used} of {limit} agents; resume it \
         with an agent_budget above {used}"
    )]
    BudgetNotRaised { used: u64, limit: u64 },
    #[error(
        "session already has the maximum of {WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION} active workflow runs"
    )]
    TooManyActiveRuns,
    #[error("session is shutting down; Workflow admission is closed")]
    SessionShuttingDown,
}

pub(crate) struct WorkflowManager {
    session_id: String,
    session_directory: Option<Arc<crate::session::storage::ContainedDirectory>>,
    cwd: PathBuf,
    tracker: Arc<parking_lot::Mutex<WorkflowTracker>>,
    store: WorkflowRunStore,
    notify: WorkflowNotifySender,
    subagent_event_tx:
        mpsc::UnboundedSender<tools::implementations::grow_build::task::types::SubagentEvent>,
    diagnostics: DiagnosticHook,
    session_cmd_tx: mpsc::UnboundedSender<crate::session::commands::SessionCommand>,
    timeline: chat_state::ChatStateHandle,
    templates: HashMap<String, String>,
    /// Route captured by the next newly-created Run. Existing Runs carry
    /// their own immutable snapshot in `WorkflowRunState`.
    next_run_route: WorkflowRuntimeRoute,
    /// Live discovery inputs consulted only at new-Run admission. The resolved
    /// definitions are copied into that Run's immutable route.
    agent_catalog_source: WorkflowAgentCatalogSource,
    active: HashMap<String, ActiveRun>,
    /// Session teardown closes this gate before draining executors.  The
    /// generation makes an admission snapshot explicit: a launch must verify
    /// it again after every await before mutating the run store/tracker.
    admission_open: bool,
    admission_generation: u64,
}

impl WorkflowManager {
    /// Re-open Workflow admission after the root session has committed its
    /// complete runtime-resume barrier. Startup must keep admission closed
    /// until the whole restored tree is ready.
    pub(crate) fn reopen_after_runtime_restore(&mut self) {
        if !self.admission_open {
            self.admission_open = true;
            self.admission_generation = self.admission_generation.saturating_add(1);
        }
    }

    /// Preflight every physically suspended, business-active Run without
    /// mutating its journal, manifest, tracker, or Timeline. A single invalid
    /// Run rejects the batch so the caller can validate the complete set
    /// before launching any member of it.
    pub(crate) fn validate_suspended_executions(&self) -> Result<Vec<String>, String> {
        let candidates = self
            .tracker
            .lock()
            .list()
            .into_iter()
            .filter(|run| run.execution_suspended && run.status == WorkflowRunStatus::Active)
            .collect::<Vec<_>>();
        let mut run_ids = Vec::with_capacity(candidates.len());
        for state in candidates {
            self.validate_suspended_run(&state)
                .map_err(|error| format!("workflow {}: {error}", state.run_id))?;
            run_ids.push(state.run_id);
        }
        Ok(run_ids)
    }

    /// Start every physically suspended, business-active Workflow under the
    /// current root restore owner. The caller keeps the Session execution gate
    /// closed until this whole batch has crossed its durable Resumed boundary.
    pub(crate) async fn restore_suspended_executions(&mut self) -> Result<(), String> {
        let run_ids = self.validate_suspended_executions()?;
        for run_id in run_ids {
            let (_, outcome) = self
                .restore_suspended_execution(&run_id)
                .await
                .map_err(|error| format!("workflow {run_id}: {error}"))?;
            tokio::task::spawn_local(async move {
                let _ = outcome.await;
            });
        }
        Ok(())
    }

    /// A cold process can observe a released root while an active Workflow
    /// still has only its old suspended lifecycle. The tree checkpoint has
    /// already been consumed, so do not auto-run journal work: create an
    /// explicit resumable infrastructure pause for the same Run instead.
    pub(crate) async fn pause_released_suspended_runs_for_manual_resume(
        &mut self,
    ) -> Result<(), String> {
        let candidates = self
            .tracker
            .lock()
            .list()
            .into_iter()
            .filter(|run| run.status == WorkflowRunStatus::Active && run.execution_suspended)
            .collect::<Vec<_>>();
        for candidate in candidates {
            let run_id = candidate.run_id.clone();
            let desired_epoch = candidate
                .execution_epoch
                .checked_add(1)
                .ok_or_else(|| format!("workflow {run_id} execution epoch is exhausted"))?;
            let mut lifecycle = self.workflow_lifecycle(&run_id).await?;
            if lifecycle.execution_epoch == candidate.execution_epoch
                && !lifecycle.open
                && !lifecycle.closed
                && lifecycle.execution_suspended
                && lifecycle.status == Some(chat_state::WorkflowExecutionStatus::Suspended)
            {
                let event =
                    chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Resumed {
                        run_id: run_id.clone(),
                        execution_epoch: desired_epoch,
                    });
                if let Err(error) = self.timeline.record_timeline_event_durably(event).await {
                    lifecycle = self.workflow_lifecycle(&run_id).await?;
                    if !workflow_lifecycle_is_open_at(&lifecycle, desired_epoch) {
                        return Err(format!(
                            "workflow {run_id} recovery admission was not confirmed: {error}"
                        ));
                    }
                } else {
                    lifecycle = self.workflow_lifecycle(&run_id).await?;
                }
            }
            let already_paused = workflow_lifecycle_is_infra_paused_at(&lifecycle, desired_epoch);
            if !already_paused && !workflow_lifecycle_is_open_at(&lifecycle, desired_epoch) {
                return Err(format!(
                    "workflow {run_id} has no exact released-suspension lifecycle to reconcile"
                ));
            }
            let paused = self
                .tracker
                .lock()
                .pause_released_suspended_for_recovery(
                    &run_id,
                    desired_epoch,
                    "The session restore was released before this Workflow executor started. Resume this Run explicitly after reviewing its pending operations.".into(),
                )
                .ok_or_else(|| format!("workflow {run_id} changed during recovery pause"))?;
            self.store
                .persist_ack(&paused)
                .await
                .map_err(|error| format!("workflow {run_id} recovery pause manifest: {error}"))?;
            if !already_paused {
                let event =
                    chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Ended {
                        run_id: run_id.clone(),
                        execution_epoch: desired_epoch,
                        status: chat_state::WorkflowExecutionStatus::InfraPaused,
                        execution_suspended: false,
                        handoff: chat_state::WorkflowTurnHandoff::None,
                        duration_ms: paused.elapsed_ms_floor,
                        message: paused.pause_message.clone(),
                    });
                if let Err(error) = self.timeline.record_timeline_event_durably(event).await {
                    lifecycle = self.workflow_lifecycle(&run_id).await?;
                    if !workflow_lifecycle_is_infra_paused_at(&lifecycle, desired_epoch) {
                        return Err(format!(
                            "workflow {run_id} recovery pause was not confirmed: {error}"
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    async fn workflow_lifecycle(
        &self,
        run_id: &str,
    ) -> Result<chat_state::WorkflowLifecycle, String> {
        let events = self
            .timeline
            .timeline_events()
            .await
            .ok_or_else(|| "Workflow Timeline is unavailable".to_owned())?;
        chat_state::Timeline::from_events(events)
            .map_err(|error| error.to_string())?
            .workflow_lifecycle(run_id)
            .ok_or_else(|| format!("workflow {run_id} has no Timeline lifecycle"))
    }

    /// Restore one Run after the root has passed the complete runtime-resume
    /// barrier and explicitly reopened admission. The existing Run source,
    /// args, route, budget, concurrency, and identity remain authoritative.
    pub(crate) async fn restore_suspended_execution(
        &mut self,
        run_id: &str,
    ) -> Result<(String, oneshot::Receiver<WorkflowOutcome>), LaunchError> {
        self.admission_snapshot()?;
        let state = self
            .tracker
            .lock()
            .get(run_id)
            .ok_or_else(|| LaunchError::UnknownRun(run_id.to_owned()))?;
        let resolved = self
            .validate_suspended_run(&state)
            .map_err(LaunchError::Store)?;
        let args = self
            .store
            .args_for(run_id)
            .ok_or_else(|| LaunchError::Store("immutable launch args are missing".into()))?;
        self.launch(
            resolved,
            LaunchSpec {
                objective: state.objective,
                args,
                agent_budget: None,
                max_concurrency: None,
                resume_run_id: Some(run_id.to_owned()),
            },
        )
        .await
    }

    fn validate_suspended_run(&self, state: &WorkflowRunState) -> Result<ResolvedWorkflow, String> {
        if !state.execution_suspended || state.status != WorkflowRunStatus::Active {
            return Err("only an execution-suspended active Run can be auto-restored".into());
        }
        state
            .validate_restored_projection()
            .map_err(str::to_owned)?;
        let definition_id = state
            .definition_id
            .clone()
            .ok_or_else(|| "frozen Workflow definition identity is missing".to_owned())?;
        let definition_scope = state
            .definition_scope
            .ok_or_else(|| "frozen Workflow definition scope is missing".to_owned())?;
        let definition_hash = state
            .definition_hash
            .clone()
            .ok_or_else(|| "frozen Workflow source hash is missing".to_owned())?;
        let script = self
            .store
            .script_for(&state.run_id)
            .ok_or_else(|| "immutable Workflow script is missing".to_owned())?;
        if script.len() as u64 > super::registry::MAX_WORKFLOW_SOURCE_BYTES {
            return Err("immutable Workflow script exceeds its source limit".into());
        }
        if super::registry::content_hash(&script) != definition_hash {
            return Err("immutable Workflow script does not match its frozen source hash".into());
        }
        let args = self
            .store
            .args_for(&state.run_id)
            .ok_or_else(|| "immutable launch args are missing".to_owned())?;
        let encoded_args = serde_json::to_vec(&args)
            .map_err(|error| format!("immutable launch args cannot be encoded: {error}"))?;
        if encoded_args.len() as u64 > super::store::MAX_WORKFLOW_ARGS_BYTES {
            return Err("immutable launch args exceed their size limit".into());
        }
        let meta = super::registry::parse_workflow(&script, None)
            .map_err(|error| format!("immutable Workflow source is invalid: {error}"))?;
        if meta.name != state.name {
            return Err("immutable Workflow source name differs from the frozen Run".into());
        }
        self.store
            .validate_persistable(state)
            .map_err(|error| format!("Workflow manifest cannot be restored: {error}"))?;
        self.validate_suspended_journal(state)?;
        Ok(ResolvedWorkflow {
            definition_id,
            scope: definition_scope,
            content_hash: definition_hash,
            meta,
            script,
            source: super::registry::WorkflowSource::Inline,
        })
    }

    fn validate_suspended_journal(&self, state: &WorkflowRunState) -> Result<(), String> {
        let Some(relative) = state.journal_path.as_deref() else {
            if self.session_directory.is_some() {
                return Err("persisted Workflow journal path is missing".into());
            }
            return Ok(());
        };
        let expected = format!("workflows/{}/journal.jsonl", state.run_id);
        if relative != expected {
            return Err("persisted Workflow journal path does not match its Run".into());
        }
        let storage = self
            .journal_storage(&state.run_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "persisted Workflow journal has no session authority".to_owned())?;
        let bytes = match storage.read_bounded(workflow::journal::MAX_JOURNAL_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("Workflow journal cannot be read: {error}")),
        };
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err("Workflow journal has an unterminated tail".into());
        }
        let mut projection = Journal::memory();
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let entry =
                serde_json::from_slice::<workflow::JournalEntry>(line).map_err(|error| {
                    format!("Workflow journal line {} is invalid: {error}", index + 1)
                })?;
            projection
                .project_physical_entry(entry)
                .map_err(|error| format!("Workflow journal is inconsistent: {error}"))?;
        }
        Ok(())
    }

    fn journal_storage(
        &self,
        run_id: &str,
    ) -> Result<Option<Arc<dyn workflow::JournalStorage>>, LaunchError> {
        let Some(session) = self.session_directory.as_deref() else {
            return Ok(None);
        };
        super::store::validate_run_id(run_id)
            .map_err(|error| LaunchError::Journal(error.to_string()))?;
        let run = session
            .open_relative(
                &std::path::Path::new("workflows").join(run_id),
                "Workflow journal directory",
                false,
            )
            .map_err(|error| LaunchError::Journal(error.to_string()))?;
        Ok(Some(Arc::new(SessionJournalStorage { run })))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        session_id: String,
        session_directory: Option<Arc<crate::session::storage::ContainedDirectory>>,
        cwd: PathBuf,
        tracker: Arc<parking_lot::Mutex<WorkflowTracker>>,
        store: WorkflowRunStore,
        notify: WorkflowNotifySender,
        subagent_event_tx: mpsc::UnboundedSender<
            tools::implementations::grow_build::task::types::SubagentEvent,
        >,
        diagnostics: DiagnosticHook,
        session_cmd_tx: mpsc::UnboundedSender<crate::session::commands::SessionCommand>,
        timeline: chat_state::ChatStateHandle,
        templates: HashMap<String, String>,
        next_run_route: WorkflowRuntimeRoute,
        agent_catalog_source: WorkflowAgentCatalogSource,
    ) -> Self {
        Self {
            session_id,
            session_directory,
            cwd,
            tracker,
            store,
            notify,
            subagent_event_tx,
            diagnostics,
            session_cmd_tx,
            timeline,
            templates,
            next_run_route,
            agent_catalog_source,
            active: HashMap::new(),
            admission_open: true,
            admission_generation: 0,
        }
    }

    pub(crate) fn close_admission(&mut self) -> u64 {
        if self.admission_open {
            self.admission_open = false;
            self.admission_generation = self.admission_generation.saturating_add(1);
        }
        self.admission_generation
    }

    pub(crate) fn admission_snapshot(&self) -> Result<u64, LaunchError> {
        self.admission_open
            .then_some(self.admission_generation)
            .ok_or(LaunchError::SessionShuttingDown)
    }

    pub(crate) fn ensure_open_for_ingress(&self) -> Result<(), LaunchError> {
        self.admission_snapshot().map(|_| ())
    }

    pub(crate) fn check_admission(&self, generation: u64) -> Result<(), LaunchError> {
        if self.admission_open && self.admission_generation == generation {
            Ok(())
        } else {
            Err(LaunchError::SessionShuttingDown)
        }
    }

    fn rollback_unspawned_run(&self, run_id: &str) {
        self.tracker.lock().clear_run(run_id);
        self.store.remove(run_id);
    }

    /// Update the launch default after a durably committed session model
    /// transition. Runs already tracked are intentionally unaffected.
    pub(crate) fn set_next_run_route(&mut self, route: WorkflowRuntimeRoute) {
        route
            .validate()
            .expect("session model selection must provide a valid Workflow route");
        self.next_run_route = route;
    }

    /// Update the complete parent-Agent projection used to admit future Runs.
    /// Active Runs already own both their filter and resolved child harnesses.
    pub(crate) fn set_next_run_agent_profile(
        &mut self,
        name: String,
        filter: agent::config::SubagentFilter,
    ) {
        self.next_run_route.set_subagent_filter(filter);
        self.agent_catalog_source.set_parent_agent_name(name);
    }

    pub(crate) fn tracker(&self) -> Arc<parking_lot::Mutex<WorkflowTracker>> {
        self.tracker.clone()
    }

    fn signal_terminal_failure(&self, run_id: &str, error: impl Into<String>) {
        let _ = self.session_cmd_tx.send(
            crate::session::commands::SessionCommand::WorkflowTerminalFailure {
                run_id: run_id.to_owned(),
                error: error.into(),
            },
        );
    }

    /// Close a lifecycle boundary that was durably opened but could not reach
    /// executor admission. The Timeline remains the authoritative execution
    /// ledger; the manifest is a recoverable projection of the same terminal
    /// state.
    async fn interrupt_rejected_launch(&self, run_id: &str, execution_epoch: u64, message: String) {
        let Some(interrupted) = self.tracker.lock().interrupt(run_id, message.clone()) else {
            return;
        };
        if let Err(error) = self.store.persist_ack(&interrupted).await {
            tracing::warn!(%run_id, %error, "failed to persist rejected Workflow launch state");
        }
        let elapsed = self.tracker.lock().elapsed_ms(run_id);
        match self
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: run_id.to_owned(),
                    execution_epoch,
                    status: chat_state::WorkflowExecutionStatus::Interrupted,
                    execution_suspended: false,
                    handoff: interrupted.turn_handoff,
                    duration_ms: elapsed,
                    message: interrupted.pause_message.clone(),
                },
            ))
            .await
        {
            Ok(_) => self.notify.broadcast(&interrupted, elapsed, 0, true).await,
            Err(error) => {
                tracing::error!(%run_id, %error, "failed to close rejected Workflow launch in Timeline");
                self.signal_terminal_failure(
                    run_id,
                    format!("workflow terminal Timeline could not be committed: {error}"),
                );
            }
        }
    }

    pub(crate) async fn launch(
        &mut self,
        resolved: ResolvedWorkflow,
        spec: LaunchSpec,
    ) -> Result<(String, oneshot::Receiver<WorkflowOutcome>), LaunchError> {
        let admission_generation = self.admission_snapshot()?;
        if let Some(run_id) = spec.resume_run_id.as_deref()
            && self
                .tracker
                .lock()
                .get(run_id)
                .is_some_and(|run| run.status.is_resumable() || run.execution_suspended)
            && let Some(active) = self.active.get_mut(run_id)
        {
            // The watcher projects a stopped state before persisting Ended.
            // Keep its receiver owned by the manager across cancellation, and
            // do not let reaping discard a failed terminal acknowledgment.
            let settled = (&mut active.done).await;
            self.active.remove(run_id);
            settled
                .map_err(|_| {
                    LaunchError::Timeline(format!(
                        "workflow {run_id} terminal watcher stopped before acknowledgement"
                    ))
                })?
                .map_err(LaunchError::Timeline)?;
        }
        self.reap_terminal_runs();
        if self.active.len() >= WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION {
            return Err(LaunchError::TooManyActiveRuns);
        }

        let definition_id = resolved.definition_id.clone();
        let definition_scope = resolved.scope;
        let definition_hash = resolved.content_hash.clone();
        let allow_fork_context = false;
        let mut execution_script = resolved.script;
        let (run_id, journal, state, resumed, execution_epoch) = match &spec.resume_run_id {
            Some(run_id) => {
                let existing = self
                    .tracker
                    .lock()
                    .get(run_id)
                    .ok_or_else(|| LaunchError::UnknownRun(run_id.clone()))?;
                if !existing.status.is_resumable() && !existing.execution_suspended {
                    return Err(LaunchError::NotResumable(
                        existing.status.as_str().to_string(),
                    ));
                }
                if existing.status
                    == crate::session::workflow::tracker::WorkflowRunStatus::BudgetLimited
                    && existing.agents_used >= workflow::MAX_AGENT_BUDGET
                {
                    return Err(LaunchError::NotResumable(
                        "maximum agent budget reached; start a new run".into(),
                    ));
                }
                let original_args = self.store.args_for(run_id).ok_or_else(|| {
                    LaunchError::Store("immutable launch args are missing".into())
                })?;
                if original_args != spec.args {
                    return Err(LaunchError::Store(
                        "workflow launch args are immutable across resume".into(),
                    ));
                }
                execution_script = self.store.script_for(run_id).ok_or_else(|| {
                    LaunchError::Store("immutable workflow script is missing".into())
                })?;
                let journal = match existing.journal_path.as_ref() {
                    Some(relative) => {
                        let expected = format!("workflows/{run_id}/journal.jsonl");
                        if relative != &expected {
                            return Err(LaunchError::Journal(
                                "persisted journal path does not match its workflow run".into(),
                            ));
                        }
                        let storage = self.journal_storage(run_id)?.ok_or_else(|| {
                            LaunchError::Journal(
                                "persisted Workflow journal has no session authority".into(),
                            )
                        })?;
                        Journal::load_storage(storage)
                            .map_err(|error| LaunchError::Journal(error.to_string()))?
                    }
                    None => Journal::memory(),
                };
                if existing.status
                    == crate::session::workflow::tracker::WorkflowRunStatus::BudgetLimited
                {
                    let used = journal.agent_reservation_count();
                    let previous = existing.agent_budget.unwrap_or(0);
                    let candidate = spec.agent_budget.unwrap_or(previous).max(previous);
                    if spec.agent_budget.is_none_or(|raised| raised <= previous)
                        || used >= candidate
                    {
                        return Err(LaunchError::BudgetNotRaised {
                            used,
                            limit: previous,
                        });
                    }
                }
                let execution_epoch = self
                    .tracker
                    .lock()
                    .execution_epoch(run_id)
                    .unwrap_or(0)
                    .saturating_add(1);

                (run_id.clone(), journal, existing, true, execution_epoch)
            }
            None => {
                let run_id = format!("wf_{}", uuid::Uuid::now_v7().simple());
                let agent_budget = spec.agent_budget.unwrap_or(WORKFLOW_DEFAULT_AGENT_BUDGET);
                let max_concurrency = spec.max_concurrency.unwrap_or(
                    tools::implementations::grow_build::workflow::WorkflowToolInput::DEFAULT_MAX_CONCURRENCY,
                );
                let mut runtime_route = self.next_run_route.clone();
                runtime_route
                    .capture_agent_definitions(&self.agent_catalog_source)
                    .await
                    .map_err(|error| LaunchError::Store(error.to_owned()))?;
                self.store
                    .register(&run_id, &execution_script, &spec.args)
                    .map_err(|error| LaunchError::Store(error.to_string()))?;
                let journal_rel = format!("workflows/{run_id}/journal.jsonl");
                let journal_storage = match self.journal_storage(&run_id) {
                    Ok(storage) => storage,
                    Err(error) => {
                        self.store.remove(&run_id);
                        return Err(error);
                    }
                };
                let journal = match journal_storage {
                    Some(storage) => Journal::with_storage(storage),
                    None => Journal::memory(),
                };
                let state = self.tracker.lock().start_run(
                    run_id.clone(),
                    resolved.meta.name,
                    spec.objective.clone(),
                    resolved.meta.phases,
                    Some(agent_budget),
                    self.session_directory.as_ref().map(|_| journal_rel),
                    runtime_route,
                );
                let state = {
                    let mut tracker = self.tracker.lock();
                    tracker.set_definition_provenance(
                        &run_id,
                        definition_id,
                        definition_scope,
                        definition_hash,
                    );
                    tracker
                        .set_max_concurrency(&run_id, max_concurrency)
                        .expect("new workflow run must exist")
                };
                (run_id, journal, state, false, 0)
            }
        };
        if let Err(error) = self.check_admission(admission_generation) {
            if !resumed {
                self.rollback_unspawned_run(&run_id);
            }
            return Err(error);
        }
        let initial_spawn = if resumed {
            None
        } else {
            let projection = (|| {
                let manifest = self.store.initial_manifest(&state)?;
                let manifest = serde_json::to_value(manifest).map_err(std::io::Error::other)?;
                let script_hash = super::store::workflow_script_hash(&execution_script);
                let args_hash = super::store::workflow_args_hash(&spec.args)?;
                Ok::<_, std::io::Error>((manifest, script_hash, args_hash))
            })();
            match projection {
                Ok(projection) => Some(projection),
                Err(error) => {
                    self.rollback_unspawned_run(&run_id);
                    return Err(LaunchError::Store(error.to_string()));
                }
            }
        };
        let lifecycle = if resumed {
            if let Err(error) = self.store.validate_persistable(&state) {
                return Err(LaunchError::Store(format!(
                    "workflow state cannot be persisted: {error}"
                )));
            }
            chat_state::WorkflowEvent::Resumed {
                run_id: run_id.clone(),
                execution_epoch,
            }
        } else {
            let (initial_manifest, script_hash, args_hash) =
                initial_spawn.expect("new Workflow Run has an initial projection");
            chat_state::WorkflowEvent::Spawned {
                run_id: run_id.clone(),
                execution_epoch,
                name: state.name.clone(),
                objective: state.objective.clone(),
                script_hash,
                args_hash,
                initial_manifest,
            }
        };
        if let Err(error) = self
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(lifecycle))
            .await
        {
            if !resumed {
                // Only causal validation proves Spawned was never offered to
                // persistence. A failed/lost ACK may follow a durable append;
                // a sidecar tombstone would then contradict Timeline recovery.
                if matches!(error, chat_state::TimelineWriteError::Invalid(_)) {
                    self.rollback_unspawned_run(&run_id);
                } else {
                    return Err(LaunchError::Timeline(format!(
                        "workflow {run_id} spawn commit is uncertain; reload the session to inspect it: {error}"
                    )));
                }
            }
            return Err(LaunchError::Timeline(error.to_string()));
        }

        let mut journal = journal;
        let resume_failure_message = (resumed
            && state.status == crate::session::workflow::tracker::WorkflowRunStatus::Failed)
            .then(|| state.pause_message.clone().unwrap_or_default());
        let state = if resumed {
            let mut tracker = self.tracker.lock();
            tracker.reconcile_agents_used(&run_id, journal.agent_reservation_count());
            let resumed = tracker
                .resume_run(&run_id, spec.agent_budget)
                .expect("resume admission was validated before the durable boundary");
            if let Some(max_concurrency) = spec.max_concurrency {
                tracker
                    .set_max_concurrency(&run_id, max_concurrency)
                    .expect("resumed workflow must remain tracked")
            } else {
                resumed
            }
        } else {
            state
        };
        if let Some(failure_message) = resume_failure_message
            && let Err(error) = journal.prune_trailing_host_error(failure_message.as_str())
        {
            let message = error.to_string();
            let interrupted = {
                let mut tracker = self.tracker.lock();
                tracker.interrupt(
                    &run_id,
                    format!("workflow journal could not prepare resume: {message}"),
                )
            };
            let turn_handoff = interrupted
                .as_ref()
                .map(|state| state.turn_handoff)
                .unwrap_or(chat_state::WorkflowTurnHandoff::Completion);
            if let Some(interrupted) = interrupted {
                let _ = self.store.persist_ack(&interrupted).await;
            }
            if let Err(timeline_error) = self
                .timeline
                .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                    chat_state::WorkflowEvent::Ended {
                        run_id: run_id.clone(),
                        execution_epoch,
                        status: chat_state::WorkflowExecutionStatus::Interrupted,
                        execution_suspended: false,
                        handoff: turn_handoff,
                        duration_ms: 0,
                        message: Some(message.clone()),
                    },
                ))
                .await
            {
                self.signal_terminal_failure(
                    &run_id,
                    format!("workflow terminal Timeline could not be committed: {timeline_error}"),
                );
            }
            return Err(LaunchError::Journal(message));
        }

        if let Err(error) = self.store.persist_ack(&state).await {
            let interrupted = self.tracker.lock().interrupt(
                &run_id,
                "workflow state persistence failed before execution; start a new run",
            );
            if let Some(interrupted) = interrupted {
                if let Err(persist_error) = self.store.persist_ack(&interrupted).await {
                    tracing::warn!(run_id = %run_id, %persist_error, "failed to persist interrupted workflow state");
                }
                if let Err(timeline_error) = self
                    .timeline
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                        chat_state::WorkflowEvent::Ended {
                            run_id: run_id.clone(),
                            execution_epoch,
                            status: chat_state::WorkflowExecutionStatus::Interrupted,
                            execution_suspended: false,
                            handoff: interrupted.turn_handoff,
                            duration_ms: 0,
                            message: Some(error.to_string()),
                        },
                    ))
                    .await
                {
                    tracing::error!(run_id = %run_id, %timeline_error, "failed to close rejected workflow execution in Timeline");
                    self.signal_terminal_failure(
                        &run_id,
                        format!(
                            "workflow terminal Timeline could not be committed: {timeline_error}"
                        ),
                    );
                }
            }
            return Err(LaunchError::Store(error.to_string()));
        }
        self.check_admission(admission_generation)?;
        self.notify
            .emit(&state, self.tracker.lock().elapsed_ms(&run_id), 0)
            .await;

        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let scratch_directory = match self.session_directory.as_deref() {
            Some(session) => Arc::new(
                match session.open_relative(
                    &std::path::Path::new("workflows")
                        .join(&run_id)
                        .join("scratch"),
                    "Workflow scratch directory",
                    true,
                ) {
                    Ok(directory) => directory,
                    Err(error) => {
                        let message =
                            format!("workflow executor resources could not be created: {error}");
                        self.interrupt_rejected_launch(&run_id, execution_epoch, message.clone())
                            .await;
                        return Err(LaunchError::Store(message));
                    }
                },
            ),
            None => {
                let path = std::env::temp_dir().join(format!(
                    "grow-workflow-scratch-{}",
                    uuid::Uuid::now_v7().simple()
                ));
                if let Err(error) = std::fs::create_dir(&path) {
                    let message =
                        format!("workflow executor resources could not be created: {error}");
                    self.interrupt_rejected_launch(&run_id, execution_epoch, message.clone())
                        .await;
                    return Err(LaunchError::Store(message));
                }
                let scratch = match crate::session::storage::ContainedDirectory::open(
                    &path,
                    std::path::Path::new(""),
                    "ephemeral Workflow scratch directory",
                    false,
                ) {
                    Ok(directory) => directory,
                    Err(error) => {
                        let message =
                            format!("workflow executor resources could not be opened: {error}");
                        self.interrupt_rejected_launch(&run_id, execution_epoch, message.clone())
                            .await;
                        return Err(LaunchError::Store(message));
                    }
                };
                Arc::new(scratch)
            }
        };

        self.check_admission(admission_generation)?;

        let suspend_intent = Arc::new(AtomicBool::new(false));
        let (host_service, host_drained) = spawn_workflow_host_service(
            WorkflowHostParams {
                run_id: run_id.clone(),
                cwd: self.cwd.clone(),
                scratch_directory,
                tracker: self.tracker.clone(),
                store: self.store.clone(),
                notify: self.notify.clone(),
                subagent_event_tx: self.subagent_event_tx.clone(),
                parent_session_id: self.session_id.clone(),
                parent_timeline: Some(self.timeline.clone()),
                allow_fork_context,
                templates: self.templates.clone(),
                diagnostics: self.diagnostics.clone(),
                cancel: cancel.clone(),
                suspend_intent: suspend_intent.clone(),
                max_concurrency: state.max_concurrency,
                runtime_route: state.runtime_route.clone(),
            },
            host_rx,
        );

        let script = execution_script;
        let args = spec.args;
        let exec_cancel = cancel.clone();
        let exec = tokio::task::spawn_blocking(move || {
            workflow::run_workflow(WorkflowRunParams {
                script,
                args,
                journal,
                host_tx,
                cancel: exec_cancel,
                max_ops: WorkflowRunParams::DEFAULT_MAX_OPS,
            })
        });

        let pause_intent = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = oneshot::channel();
        self.active.insert(
            run_id.clone(),
            ActiveRun {
                cancel: cancel.clone(),
                pause_intent: pause_intent.clone(),
                suspend_intent: suspend_intent.clone(),
                done: done_rx,
            },
        );

        let (outcome_tx, outcome_rx) = oneshot::channel();
        let tracker = self.tracker.clone();
        let store = self.store.clone();
        let notify = self.notify.clone();
        let session_cmd_tx = self.session_cmd_tx.clone();
        let completion_session_directory = self.session_directory.clone();
        let completion_cwd = self.cwd.clone();
        let watcher_run_id = run_id.clone();
        let watcher_cancel = cancel.clone();
        let timeline = self.timeline.clone();
        tokio::spawn(async move {
            let mut outcome = exec.await.unwrap_or_else(|e| WorkflowOutcome::Failed {
                error: format!("workflow executor panicked: {e}"),
            });
            if !host_service.is_finished() {
                watcher_cancel.cancel();
            }
            let drain_failed = if suspend_intent.load(Ordering::Acquire) {
                // The caller's shared exit deadline may expire, but the Run
                // keeps owning its actual host workers until they join.
                !matches!(host_drained.await, Ok(HostDrainOutcome::Drained))
            } else {
                !matches!(
                    tokio::time::timeout(std::time::Duration::from_secs(25), host_drained).await,
                    Ok(Ok(HostDrainOutcome::Drained))
                )
            };
            if drain_failed {
                host_service.abort();
            }
            let _ = host_service.await;
            if drain_failed {
                tracing::warn!(run_id = %watcher_run_id, "workflow host/child drain did not complete before lifecycle update");
                outcome = WorkflowOutcome::Failed {
                    error:
                        "workflow cleanup did not complete; run is interrupted and cannot resume"
                            .into(),
                };
            }
            let state = {
                let mut tracker = tracker.lock();
                if tracker.execution_epoch(&watcher_run_id) != Some(execution_epoch) {
                    None
                } else if drain_failed {
                    tracker.interrupt(
                        &watcher_run_id,
                        "workflow cleanup timed out or could not be acknowledged; start a new run",
                    )
                } else if suspend_intent.load(Ordering::Acquire)
                    && matches!(outcome, WorkflowOutcome::Cancelled)
                    && !pause_intent.load(Ordering::Acquire)
                {
                    tracker.suspend_execution(&watcher_run_id)
                } else if pause_intent.load(Ordering::Relaxed)
                    && matches!(
                        outcome,
                        WorkflowOutcome::Cancelled
                            | WorkflowOutcome::Paused { .. }
                            | WorkflowOutcome::AwaitingUser { .. }
                    )
                {
                    tracker.pause_user(&watcher_run_id, None)
                } else {
                    tracker.apply_outcome(&watcher_run_id, &outcome)
                }
            };
            if let Some(mut state) = state {
                if suspend_intent.load(Ordering::Acquire)
                    && !state.status.is_terminal()
                    && !state.execution_suspended
                {
                    state = tracker
                        .lock()
                        .suspend_execution(&watcher_run_id)
                        .unwrap_or(state);
                }
                let mut manifest_persisted = true;
                if let Err(error) = store.persist_ack(&state).await {
                    tracing::warn!(run_id = %watcher_run_id, %error, "workflow terminal manifest was not durably written");
                    let _ = session_cmd_tx.send(
                        crate::session::commands::SessionCommand::WorkflowTerminalFailure {
                            run_id: watcher_run_id.clone(),
                            error: format!(
                                "workflow terminal manifest could not be committed: {error}"
                            ),
                        },
                    );
                    outcome = WorkflowOutcome::Failed {
                        error: format!(
                            "workflow terminal state could not be persisted: {error}; run is interrupted"
                        ),
                    };
                    state = tracker
                        .lock()
                        .interrupt(
                            &watcher_run_id,
                            format!(
                                "workflow terminal state could not be persisted: {error}; start a new run"
                            ),
                        )
                        .unwrap_or(state);
                    if let Err(interrupt_error) = store.persist_ack(&state).await {
                        manifest_persisted = false;
                        tracing::error!(run_id = %watcher_run_id, %interrupt_error, "failed to persist workflow interruption marker");
                    }
                }
                // Consume the once-per-hash save hint only after the successful
                // terminal state is durable. A persistence failure turns the
                // Run into Interrupted and must not suppress a later successful
                // Run's prompt for the same draft hash.
                if manifest_persisted
                    && state.status
                        == crate::session::workflow::tracker::WorkflowRunStatus::Complete
                    && let (Some(session), Some(definition_id), Some(definition_hash)) = (
                        completion_session_directory.as_deref(),
                        state.definition_id.as_ref(),
                        state.definition_hash.as_deref(),
                    )
                    && let Ok(mut workspace) =
                        super::workspace::WorkflowWorkspace::open_reconciled_in_session(
                            session,
                            &completion_cwd,
                        )
                    && workspace
                        .take_save_prompt(definition_id, definition_hash)
                        .unwrap_or(false)
                {
                    state = tracker
                        .lock()
                        .set_save_prompt(&watcher_run_id, true)
                        .unwrap_or(state);
                    if let Err(error) = store.persist_ack(&state).await {
                        tracing::warn!(run_id = %watcher_run_id, %error, "workflow save prompt marker was not durably written");
                    }
                }
                let elapsed = tracker.lock().elapsed_ms(&watcher_run_id);
                if let Err(error) = timeline
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                        chat_state::WorkflowEvent::Ended {
                            run_id: watcher_run_id.clone(),
                            execution_epoch,
                            execution_suspended: state.execution_suspended,
                            status: if state.execution_suspended {
                                if state.status == WorkflowRunStatus::Active {
                                    chat_state::WorkflowExecutionStatus::Suspended
                                } else {
                                    state.status.to_timeline()
                                }
                            } else {
                                state.status.to_timeline()
                            },
                            handoff: state.turn_handoff,
                            duration_ms: elapsed,
                            message: state.pause_message.clone(),
                        },
                    ))
                    .await
                {
                    tracing::error!(run_id = %watcher_run_id, %error, manifest_persisted, "workflow terminal Timeline boundary was rejected");
                    let message =
                        format!("workflow Timeline terminal could not be committed: {error}");
                    let _ = session_cmd_tx.send(
                        crate::session::commands::SessionCommand::WorkflowTerminalFailure {
                            run_id: watcher_run_id.clone(),
                            error: message.clone(),
                        },
                    );
                    let _ = done_tx.send(Err(message.clone()));
                    let _ = outcome_tx.send(WorkflowOutcome::Failed { error: message });
                    return;
                }
                notify.broadcast(&state, elapsed, 0, true).await;
                if state.turn_handoff != chat_state::WorkflowTurnHandoff::None {
                    tokio::spawn(deliver_workflow_handoff_acknowledged(
                        session_cmd_tx.clone(),
                        state.clone(),
                    ));
                }
                let _ = done_tx.send(Ok(state));
            } else {
                let message = format!(
                    "workflow {watcher_run_id} execution epoch {execution_epoch} was superseded before its terminal boundary"
                );
                let _ = done_tx.send(Err(message));
            }
            let _ = outcome_tx.send(outcome);
        });

        Ok((run_id, outcome_rx))
    }

    #[cfg(test)]
    pub(crate) fn test_bundle() -> (
        Arc<tokio::sync::Mutex<WorkflowManager>>,
        Arc<parking_lot::Mutex<WorkflowTracker>>,
    ) {
        Self::test_bundle_with_session_dir(None)
    }

    #[cfg(test)]
    pub(crate) fn test_bundle_with_timeline(
        timeline: chat_state::ChatStateHandle,
        session_id: String,
        session_directory: Arc<crate::session::storage::ContainedDirectory>,
    ) -> (
        Arc<tokio::sync::Mutex<WorkflowManager>>,
        Arc<parking_lot::Mutex<WorkflowTracker>>,
        mpsc::UnboundedReceiver<tools::implementations::grow_build::task::types::SubagentEvent>,
    ) {
        let tracker = Arc::new(parking_lot::Mutex::new(WorkflowTracker::default()));
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let (subagent_tx, mut subagent_rx) = mpsc::unbounded_channel();
        let (observed_subagent_tx, observed_subagent_rx) = mpsc::unbounded_channel();
        tokio::task::spawn_local(async move {
            while let Some(event) = subagent_rx.recv().await {
                match event {
                    tools::implementations::grow_build::task::types::SubagentEvent::Cancel(
                        request,
                    ) => {
                        let _ = request.respond_to.send(
                            tools::implementations::grow_build::task::types::SubagentCancelOutcome::Cancelled,
                        );
                    }
                    event => {
                        if observed_subagent_tx.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let (persist_tx, mut persist_rx) = mpsc::unbounded_channel();
        tokio::task::spawn_local(async move {
            while let Some(message) = persist_rx.recv().await {
                if let crate::session::persistence::PersistenceMsg::WorkflowRunStateAndAck {
                    respond_to,
                    ..
                } = message
                {
                    let _ = respond_to.send(Ok(()));
                }
            }
        });
        let store = WorkflowRunStore::new(Some(session_directory.clone()), persist_tx.clone());
        let notify = super::notify::WorkflowNotifySender::new(
            agent_client_protocol::schema::v1::SessionId::new(session_id.clone()),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            sampler::PreviewEventBudget::default(),
            Arc::new(parking_lot::Mutex::new(None)),
            store.clone(),
        );
        let manager = Arc::new(tokio::sync::Mutex::new(WorkflowManager::new(
            session_id,
            Some(session_directory),
            std::env::temp_dir(),
            tracker.clone(),
            store,
            notify,
            subagent_tx,
            Arc::new(|_, _, _| {}),
            mpsc::unbounded_channel().0,
            timeline,
            HashMap::new(),
            crate::session::workflow::tracker::test_runtime_route(),
            crate::session::workflow::tracker::WorkflowAgentCatalogSource::for_test(
                std::env::temp_dir(),
            ),
        )));
        (manager, tracker, observed_subagent_rx)
    }

    #[cfg(test)]
    pub(crate) async fn test_seed_workflow_run(
        &mut self,
        run_id: &str,
        script: &str,
    ) -> WorkflowRunState {
        use tools::implementations::grow_build::workflow::{WorkflowDefinitionId, WorkflowScope};

        let args = serde_json::json!({"fixed": true});
        self.store.register(run_id, script, &args).unwrap();
        let journal_path = self
            .session_directory
            .as_ref()
            .map(|_| format!("workflows/{run_id}/journal.jsonl"));
        if let Some(storage) = self.journal_storage(run_id).unwrap() {
            // Restoration requires the same durable journal shape that launch
            // would create; keep missing-journal rejection meaningful.
            let mut journal = Journal::with_storage(storage);
            journal
                .record(
                    0,
                    "fixture_seed",
                    "fixture-seed".into(),
                    serde_json::json!({}),
                )
                .unwrap();
        }
        let initial_state = {
            let mut tracker = self.tracker.lock();
            tracker.start_run(
                run_id.to_owned(),
                "restore-test".into(),
                "restore the same Run".into(),
                Vec::new(),
                Some(41),
                journal_path,
                crate::session::workflow::tracker::test_runtime_route(),
            );
            tracker.set_definition_provenance(
                run_id,
                WorkflowDefinitionId::new("session:restore-test"),
                WorkflowScope::Session,
                super::registry::content_hash(script),
            );
            tracker.set_max_concurrency(run_id, 2).unwrap()
        };
        let initial_manifest =
            serde_json::to_value(self.store.initial_manifest(&initial_state).unwrap()).unwrap();
        self.timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: run_id.to_owned(),
                    execution_epoch: 0,
                    name: initial_state.name.clone(),
                    objective: initial_state.objective.clone(),
                    script_hash: super::store::workflow_script_hash(script),
                    args_hash: super::store::workflow_args_hash(&args).unwrap(),
                    initial_manifest,
                },
            ))
            .await
            .unwrap();
        initial_state
    }

    #[cfg(test)]
    pub(crate) async fn test_suspend_seeded_run(&self, run_id: &str) -> WorkflowRunState {
        let suspended = self.tracker.lock().suspend_execution(run_id).unwrap();
        self.timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: run_id.to_owned(),
                    execution_epoch: 0,
                    status: chat_state::WorkflowExecutionStatus::Suspended,
                    execution_suspended: true,
                    handoff: chat_state::WorkflowTurnHandoff::None,
                    duration_ms: 0,
                    message: None,
                },
            ))
            .await
            .unwrap();
        suspended
    }

    #[cfg(test)]
    pub(crate) fn test_bundle_with_session_dir(
        session_dir: Option<PathBuf>,
    ) -> (
        Arc<tokio::sync::Mutex<WorkflowManager>>,
        Arc<parking_lot::Mutex<WorkflowTracker>>,
    ) {
        let tracker = Arc::new(parking_lot::Mutex::new(WorkflowTracker::default()));
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let (persist_tx, _persist_rx) = mpsc::unbounded_channel();
        let session_directory = test_session_directory(session_dir);
        let store = WorkflowRunStore::new(session_directory.clone(), persist_tx.clone());
        let notify = super::notify::WorkflowNotifySender::new(
            agent_client_protocol::schema::v1::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            sampler::PreviewEventBudget::default(),
            Arc::new(parking_lot::Mutex::new(None)),
            store.clone(),
        );
        let manager = Arc::new(tokio::sync::Mutex::new(WorkflowManager::new(
            "test-session".into(),
            session_directory,
            std::env::temp_dir(),
            tracker.clone(),
            store,
            notify,
            mpsc::unbounded_channel().0,
            Arc::new(|_, _, _| {}),
            mpsc::unbounded_channel().0,
            test_timeline(),
            std::collections::HashMap::new(),
            crate::session::workflow::tracker::test_runtime_route(),
            crate::session::workflow::tracker::WorkflowAgentCatalogSource::for_test(
                std::env::temp_dir(),
            ),
        )));
        (manager, tracker)
    }

    fn reap_terminal_runs(&mut self) {
        let settled = self
            .active
            .iter_mut()
            .filter_map(|(run_id, run)| match run.done.try_recv() {
                Ok(_) | Err(oneshot::error::TryRecvError::Closed) => Some(run_id.clone()),
                Err(oneshot::error::TryRecvError::Empty) => None,
            })
            .collect::<Vec<_>>();
        for run_id in settled {
            self.active.remove(&run_id);
        }
    }

    fn cancel_children_for_run(&self, run_id: &str) -> bool {
        let (respond_to, _response) = oneshot::channel();
        self.subagent_event_tx
            .send(
                tools::implementations::grow_build::task::types::SubagentEvent::Cancel(
                    tools::implementations::grow_build::task::types::SubagentCancelRequest {
                        parent_session_id: Some(self.session_id.clone()),
                        target: tools::implementations::grow_build::task::types::SubagentCancelTarget::WorkflowRunId(
                            run_id.to_owned(),
                        ),
                        respond_to,
                    },
                ),
            )
            .is_ok()
    }

    async fn await_terminal_barrier(
        run_id: &str,
        done: oneshot::Receiver<Result<WorkflowRunState, String>>,
    ) -> Result<WorkflowRunState, String> {
        done.await.map_err(|_| {
            format!("workflow {run_id} terminal watcher stopped before acknowledgement")
        })?
    }

    pub(crate) async fn pause(&mut self, run_id: &str) -> Result<WorkflowRunState, String> {
        let Some(run) = self.active.remove(run_id) else {
            return Err(format!("workflow {run_id} is not active"));
        };
        run.pause_intent.store(true, Ordering::Relaxed);
        run.cancel.cancel();
        let _ = self.cancel_children_for_run(run_id);
        let state = Self::await_terminal_barrier(run_id, run.done).await?;
        if state.status != WorkflowRunStatus::UserPaused
            || state.turn_handoff != chat_state::WorkflowTurnHandoff::None
        {
            return Err(format!(
                "workflow {run_id} settled as {} with {:?} handoff instead of an explicit user pause",
                state.status.as_str(),
                state.turn_handoff
            ));
        }
        Ok(state)
    }

    pub(crate) async fn cancel(&mut self, run_id: &str) -> Result<WorkflowRunState, String> {
        if let Some(run) = self.active.remove(run_id) {
            run.cancel.cancel();
            let _ = self.cancel_children_for_run(run_id);
            let state = Self::await_terminal_barrier(run_id, run.done).await?;
            if !state.status.is_resumable() {
                return Ok(state);
            }
            // A natural pause may have won the race with the cancel request.
            // Its Ended boundary is now acknowledged, so close that resumable
            // execution through the normal inactive cancellation transaction.
        }
        let _ = self.cancel_children_for_run(run_id);
        let (execution_epoch, elapsed) = {
            let tracker = self.tracker.lock();
            let Some(state) = tracker.get(run_id) else {
                return Err(format!("workflow {run_id} was not found"));
            };
            if !state.status.is_resumable() {
                return Err(format!(
                    "workflow {run_id} is not cancellable (status: {})",
                    state.status.as_str()
                ));
            }
            (
                tracker.execution_epoch(run_id).unwrap_or(0),
                tracker.elapsed_ms(run_id),
            )
        };
        if let Err(error) = self
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Closed {
                    run_id: run_id.to_owned(),
                    execution_epoch,
                    status: chat_state::WorkflowExecutionStatus::Cancelled,
                    handoff: chat_state::WorkflowTurnHandoff::None,
                    duration_ms: elapsed,
                    message: Some("cancelled while no execution was active".into()),
                },
            ))
            .await
        {
            tracing::error!(%run_id, %error, "refusing to cancel inactive Workflow without a durable Timeline close");
            self.signal_terminal_failure(
                run_id,
                format!("workflow terminal Timeline could not be committed: {error}"),
            );
            return Err(format!(
                "workflow {run_id} Timeline close was not durable: {error}"
            ));
        }
        let state = {
            let mut tracker = self.tracker.lock();
            tracker.close_cancelled(run_id)
        };
        match state {
            Some(state) => {
                if let Err(error) = self.store.persist_ack(&state).await {
                    tracing::error!(%run_id, %error, "Workflow close is durable but cancelled manifest could not be persisted");
                    self.signal_terminal_failure(
                        run_id,
                        format!("workflow terminal manifest could not be committed: {error}"),
                    );
                }
                let (state, elapsed) = {
                    let tracker = self.tracker.lock();
                    (
                        tracker.get(run_id).expect("run still tracked"),
                        tracker.elapsed_ms(run_id),
                    )
                };
                self.notify.emit(&state, elapsed, 0).await;
                if state.turn_handoff != chat_state::WorkflowTurnHandoff::None {
                    tokio::spawn(deliver_workflow_handoff_acknowledged(
                        self.session_cmd_tx.clone(),
                        state.clone(),
                    ));
                }
                Ok(state)
            }
            None => Err(format!("workflow {run_id} could not be closed")),
        }
    }

    pub(crate) async fn cancel_all_and_drain(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<(), Vec<String>> {
        let active: Vec<(String, ActiveRun)> = self.active.drain().collect();
        for (run_id, run) in &active {
            run.cancel.cancel();
            let _ = self.cancel_children_for_run(run_id);
        }
        type PendingWorkflowDrain = (
            Option<String>,
            oneshot::Receiver<Result<WorkflowRunState, String>>,
        );
        let mut pending: Vec<PendingWorkflowDrain> = active
            .into_iter()
            .map(|(run_id, run)| (Some(run_id), run.done))
            .collect();

        let mut failures = Vec::new();
        let mut timed_out = Vec::new();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut pending = pending.into_iter();
        while let Some((run_id, done)) = pending.next() {
            let Some(run_id) = run_id else {
                continue;
            };
            match tokio::time::timeout_at(deadline, done).await {
                Ok(Ok(Ok(_state))) => {}
                Ok(Ok(Err(error))) => failures.push(format!("{run_id}: {error}")),
                Ok(Err(error)) => failures.push(format!(
                    "{run_id}: terminal watcher channel closed before acknowledgement ({error})"
                )),
                Err(_) => {
                    failures.push(format!("{run_id}: terminal drain timed out"));
                    timed_out.push(run_id);
                    timed_out.extend(pending.filter_map(|(run_id, _)| run_id));
                    break;
                }
            }
        }
        if failures.is_empty() {
            return Ok(());
        }

        tracing::warn!(
            failures = ?failures,
            "workflow shutdown drain did not complete cleanly"
        );
        for run_id in &timed_out {
            if self
                .tracker
                .lock()
                .get(run_id)
                .is_some_and(|s| s.status.is_terminal() || s.status.is_paused())
            {
                continue;
            }
            let state = {
                let mut tracker = self.tracker.lock();
                tracker.interrupt(
                    run_id,
                    "session shutdown timed out before workflow cleanup completed; the run cannot resume",
                )
            };
            if let Some(state) = state {
                if let Err(error) = self.store.persist_ack(&state).await {
                    tracing::error!(%run_id, %error, "failed to persist workflow shutdown interruption");
                }
                let elapsed = self.tracker.lock().elapsed_ms(run_id);
                if let Err(error) = self
                    .timeline
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                        chat_state::WorkflowEvent::Ended {
                            run_id: run_id.clone(),
                            execution_epoch: state.execution_epoch,
                            status: chat_state::WorkflowExecutionStatus::Interrupted,
                            execution_suspended: false,
                            handoff: state.turn_handoff,
                            duration_ms: elapsed,
                            message: state.pause_message.clone(),
                        },
                    ))
                    .await
                {
                    tracing::error!(%run_id, %error, "failed to persist Workflow shutdown terminal in Timeline");
                    self.signal_terminal_failure(
                        run_id,
                        format!("workflow terminal Timeline could not be committed: {error}"),
                    );
                }
                self.notify.broadcast(&state, elapsed, 0, true).await;
            }
        }
        Err(failures)
    }

    pub(crate) async fn suspend_all_and_drain(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), String> {
        self.close_admission();
        for run in self.active.values() {
            run.suspend_intent.store(true, Ordering::Release);
            run.cancel.cancel();
        }
        let ids: Vec<_> = self.active.keys().cloned().collect();
        for id in ids {
            let run = self
                .active
                .get_mut(&id)
                .expect("owned Run remains registered");
            // Borrow the receiver so a timeout cannot discard the owner/ACK.
            tokio::time::timeout_at(deadline, &mut run.done)
                .await
                .map_err(|_| format!("Workflow {id} suspension is still settling"))?
                .map_err(|_| format!("Workflow {id} lost its suspension acknowledgement"))??;
            self.active.remove(&id);
        }
        Ok(())
    }

    /// Bounded fail-stop cancellation used when a session owner did not
    /// drain and teardown must not await this manager's terminal/persistence
    /// locks. Normal shutdown uses `cancel_all_and_drain`; this method only
    /// revokes execution and child admission without claiming durability.
    pub(crate) fn request_cancel_all(&self) {
        for (run_id, run) in &self.active {
            if run.suspend_intent.load(Ordering::Acquire) {
                continue;
            }
            run.cancel.cancel();
            let _ = self.cancel_children_for_run(run_id);
        }
    }

    #[cfg(test)]
    pub(crate) fn test_insert_active_run(
        &mut self,
        run_id: String,
        done: oneshot::Receiver<Result<WorkflowRunState, String>>,
    ) {
        self.active.insert(
            run_id,
            ActiveRun {
                cancel: CancellationToken::new(),
                pause_intent: Arc::new(AtomicBool::new(false)),
                suspend_intent: Arc::new(AtomicBool::new(false)),
                done,
            },
        );
    }

    pub(crate) fn script_copy_for(&self, run_id: &str) -> Option<String> {
        self.store.script_for(run_id)
    }

    pub(crate) fn args_copy_for(&self, run_id: &str) -> serde_json::Value {
        self.store
            .args_for(run_id)
            .unwrap_or(serde_json::Value::Null)
    }
}

/// Retry until SessionActor durably admits the model-visible receipt or its
/// mailbox closes. The Workflow lifecycle barrier is deliberately independent:
/// waiting for the actor here would deadlock a pause/cancel command that is
/// itself awaiting the manager's terminal barrier. A restored session replays
/// the same immutable run/epoch identity when this handoff cannot complete.
async fn deliver_workflow_handoff_acknowledged(
    session_cmd_tx: mpsc::UnboundedSender<crate::session::commands::SessionCommand>,
    state: WorkflowRunState,
) {
    const INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_millis(100);
    const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);

    let mut attempt = 1_usize;
    let mut backoff = INITIAL_BACKOFF;
    loop {
        let (respond_to, admitted) = oneshot::channel();
        if session_cmd_tx
            .send(
                crate::session::commands::SessionCommand::WorkflowHandoffReady {
                    state: Box::new(state.clone()),
                    respond_to,
                },
            )
            .is_err()
        {
            tracing::warn!(run_id = %state.run_id, "workflow handoff delivery stopped because the session mailbox closed");
            return;
        }
        let error = match admitted.await {
            Ok(Ok(())) => return,
            Ok(Err(error)) => error,
            Err(error) => format!("acknowledgement dropped: {error}"),
        };
        tracing::warn!(
            run_id = %state.run_id,
            %error,
            attempt,
            retry_delay_ms = backoff.as_millis() as u64,
            "workflow handoff receipt admission failed; retrying"
        );
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff.saturating_mul(2), MAX_BACKOFF);
        attempt = attempt.saturating_add(1);
    }
}

fn workflow_lifecycle_is_open_at(
    lifecycle: &chat_state::WorkflowLifecycle,
    execution_epoch: u64,
) -> bool {
    lifecycle.execution_epoch == execution_epoch && lifecycle.open && !lifecycle.closed
}

fn workflow_lifecycle_is_infra_paused_at(
    lifecycle: &chat_state::WorkflowLifecycle,
    execution_epoch: u64,
) -> bool {
    lifecycle.execution_epoch == execution_epoch
        && !lifecycle.open
        && !lifecycle.closed
        && !lifecycle.execution_suspended
        && lifecycle.status == Some(chat_state::WorkflowExecutionStatus::InfraPaused)
}

#[cfg(test)]
fn test_session_directory(
    path: Option<PathBuf>,
) -> Option<Arc<crate::session::storage::ContainedDirectory>> {
    path.map(|path| {
        Arc::new(
            crate::session::storage::ContainedDirectory::open(
                &path,
                std::path::Path::new(""),
                "Workflow test session",
                false,
            )
            .expect("pin Workflow test session"),
        )
    })
}

#[cfg(test)]
pub(super) fn test_timeline() -> chat_state::ChatStateHandle {
    test_timeline_with_persistence(
        Box::new(chat_state::NullTimelinePersistence),
        CancellationToken::new(),
    )
}

#[cfg(test)]
fn test_timeline_with_persistence(
    persistence: Box<dyn chat_state::TimelinePersistence>,
    cancellation: CancellationToken,
) -> chat_state::ChatStateHandle {
    let config = sampling_types::SamplingConfig {
        base_url: "https://api.example.com".into(),
        model: "test-model".into(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: std::num::NonZeroU64::new(128_000).expect("non-zero test window"),
        reasoning_effort: None,
        stream_tool_calls: None,
    };
    chat_state::ChatStateActor::spawn(
        Vec::new(),
        config,
        persistence,
        mpsc::unbounded_channel().0,
        cancellation,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::persistence::PersistenceMsg;
    use crate::session::workflow::registry::resolve_inline;

    type SubagentEventRx =
        mpsc::UnboundedReceiver<tools::implementations::grow_build::task::types::SubagentEvent>;
    type CancelLog = Arc<
        parking_lot::Mutex<
            Vec<tools::implementations::grow_build::task::types::SubagentCancelTarget>,
        >,
    >;

    fn test_manager(session_dir: Option<PathBuf>) -> (WorkflowManager, SubagentEventRx) {
        let (manager, events, _cancels) = test_manager_with_cancels(session_dir);
        (manager, events)
    }

    fn test_manager_with_cancels(
        session_dir: Option<PathBuf>,
    ) -> (WorkflowManager, SubagentEventRx, CancelLog) {
        test_manager_with_persistence(session_dir, false)
    }

    fn test_manager_with_persistence(
        session_dir: Option<PathBuf>,
        reject_manifest_ack: bool,
    ) -> (WorkflowManager, SubagentEventRx, CancelLog) {
        use tools::implementations::grow_build::task::types::{
            SubagentCancelOutcome, SubagentEvent,
        };

        let (subagent_tx, mut raw_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancels: CancelLog = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let cancels_stub = cancels.clone();
        tokio::spawn(async move {
            while let Some(event) = raw_rx.recv().await {
                match event {
                    SubagentEvent::Cancel(request) => {
                        cancels_stub.lock().push(request.target.clone());
                        let _ = request.respond_to.send(SubagentCancelOutcome::Cancelled);
                    }
                    other => {
                        if event_tx.send(other).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let (persist_tx, mut persist_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut manifest_ack_count = 0usize;
            while let Some(message) = persist_rx.recv().await {
                if let PersistenceMsg::WorkflowRunStateAndAck { respond_to, .. } = message {
                    let result = if reject_manifest_ack && manifest_ack_count > 0 {
                        Err(std::io::Error::other("manifest disk failure"))
                    } else {
                        Ok(())
                    };
                    manifest_ack_count += 1;
                    let _ = respond_to.send(result);
                }
            }
        });
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let session_directory = test_session_directory(session_dir);
        let store = WorkflowRunStore::new(session_directory.clone(), persist_tx.clone());
        let notify = WorkflowNotifySender::new(
            agent_client_protocol::schema::v1::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            sampler::PreviewEventBudget::default(),
            Arc::new(parking_lot::Mutex::new(None)),
            store.clone(),
        );
        let tracker = Arc::new(parking_lot::Mutex::new(WorkflowTracker::default()));
        let manager = WorkflowManager::new(
            "test-session".into(),
            session_directory,
            std::env::temp_dir(),
            tracker,
            store,
            notify,
            subagent_tx,
            Arc::new(|_, _, _| {}),
            mpsc::unbounded_channel().0,
            test_timeline(),
            HashMap::new(),
            crate::session::workflow::tracker::test_runtime_route(),
            crate::session::workflow::tracker::WorkflowAgentCatalogSource::for_test(
                std::env::temp_dir(),
            ),
        );
        (manager, event_rx, cancels)
    }

    fn spec() -> LaunchSpec {
        LaunchSpec {
            objective: "obj".into(),
            args: serde_json::json!({}),
            agent_budget: None,
            max_concurrency: None,
            resume_run_id: None,
        }
    }

    fn insert_suspended_workflow(
        manager: &WorkflowManager,
        run_id: &str,
        script: &str,
        status: WorkflowRunStatus,
    ) -> WorkflowRunState {
        use tools::implementations::grow_build::workflow::{WorkflowDefinitionId, WorkflowScope};

        manager
            .store
            .register(run_id, script, &serde_json::json!({"fixed": true}))
            .unwrap();
        let journal_path = manager
            .session_directory
            .as_ref()
            .map(|_| format!("workflows/{run_id}/journal.jsonl"));
        manager.tracker.lock().start_run(
            run_id.to_owned(),
            "restore-test".into(),
            "restore the same Run".into(),
            Vec::new(),
            Some(41),
            journal_path,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        let hash = super::super::registry::content_hash(script);
        manager.tracker.lock().set_definition_provenance(
            run_id,
            WorkflowDefinitionId::new("session:restore-test"),
            WorkflowScope::Session,
            hash,
        );
        manager
            .tracker
            .lock()
            .set_max_concurrency(run_id, 2)
            .expect("inserted workflow remains tracked");
        if status != WorkflowRunStatus::Active {
            assert_eq!(status, WorkflowRunStatus::UserPaused);
            manager
                .tracker
                .lock()
                .pause_user(run_id, Some("paused".into()))
                .expect("inserted workflow remains tracked");
        }
        manager
            .tracker
            .lock()
            .suspend_execution(run_id)
            .expect("workflow can be suspended")
    }

    #[tokio::test]
    async fn suspended_restore_reuses_run_source_route_budget_and_epoch() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = r#"let meta = #{ name: "restore-test", description: "d" };
complete("restored");"#;
        let args = serde_json::json!({"fixed": true});
        let run_id = "wf_restore_same_run";
        manager.store.register(run_id, script, &args).unwrap();
        // This test seeds a suspended Run directly instead of creating it via
        // `launch`; persist a real initial journal row so its fixture matches
        // the durable state that a launched Run would restore.
        let journal_storage = manager.journal_storage(run_id).unwrap().unwrap();
        let mut seeded_journal = Journal::with_storage(journal_storage);
        seeded_journal
            .record(
                0,
                "fixture_seed",
                "fixture-seed".into(),
                serde_json::json!({}),
            )
            .unwrap();
        let initial_state = {
            let mut tracker = manager.tracker.lock();
            tracker.start_run(
                run_id.to_owned(),
                "restore-test".into(),
                "restore the same Run".into(),
                Vec::new(),
                Some(41),
                Some(format!("workflows/{run_id}/journal.jsonl")),
                crate::session::workflow::tracker::test_runtime_route(),
            );
            tracker.set_definition_provenance(
                run_id,
                tools::implementations::grow_build::workflow::WorkflowDefinitionId::new(
                    "session:restore-test",
                ),
                tools::implementations::grow_build::workflow::WorkflowScope::Session,
                super::super::registry::content_hash(script),
            );
            tracker.set_max_concurrency(run_id, 2).unwrap()
        };
        let initial_manifest =
            serde_json::to_value(manager.store.initial_manifest(&initial_state).unwrap()).unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: run_id.into(),
                    execution_epoch: 0,
                    name: initial_state.name.clone(),
                    objective: initial_state.objective.clone(),
                    script_hash: super::super::store::workflow_script_hash(script),
                    args_hash: super::super::store::workflow_args_hash(&args).unwrap(),
                    initial_manifest,
                },
            ))
            .await
            .unwrap();
        let suspended = manager.tracker.lock().suspend_execution(run_id).unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                    operation_id: "suspend-restore-op".into(),
                    root_session_id: "test-session".into(),
                },
            ))
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: run_id.into(),
                    execution_epoch: 0,
                    status: chat_state::WorkflowExecutionStatus::Suspended,
                    execution_suspended: true,
                    handoff: chat_state::WorkflowTurnHandoff::None,
                    duration_ms: 0,
                    message: None,
                },
            ))
            .await
            .unwrap();
        manager.close_admission();
        assert_eq!(
            manager.validate_suspended_executions().unwrap(),
            vec![run_id.to_owned()]
        );
        manager.reopen_after_runtime_restore();

        let (restored_id, outcome) = manager.restore_suspended_execution(run_id).await.unwrap();
        assert_eq!(restored_id, run_id);
        assert!(matches!(
            outcome.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));
        let restored = manager.tracker.lock().get(run_id).unwrap();
        assert_eq!(restored.execution_epoch, 1);
        assert_eq!(restored.status, WorkflowRunStatus::Complete);
        assert_eq!(restored.agent_budget, suspended.agent_budget);
        assert_eq!(restored.max_concurrency, suspended.max_concurrency);
        assert_eq!(restored.runtime_route, suspended.runtime_route);
        assert_eq!(restored.definition_id, suspended.definition_id);
        assert_eq!(restored.definition_scope, suspended.definition_scope);
        assert_eq!(restored.definition_hash, suspended.definition_hash);
        assert_eq!(manager.store.script_for(run_id).as_deref(), Some(script));
        assert_eq!(manager.store.args_for(run_id), Some(args));
        assert_eq!(restored.agents, suspended.agents);
        assert!(
            dir.path()
                .join("workflows")
                .join(run_id)
                .join("journal.jsonl")
                .exists()
        );
    }

    #[tokio::test]
    async fn suspended_restore_preflight_excludes_business_paused_runs() {
        let (manager, _rx) = test_manager(None);
        let script = r#"let meta = #{ name: "restore-test", description: "d" };
complete("done");"#;
        let paused = insert_suspended_workflow(
            &manager,
            "wf_paused_suspended",
            script,
            WorkflowRunStatus::UserPaused,
        );
        assert!(manager.validate_suspended_executions().unwrap().is_empty());
        assert_eq!(paused.status, WorkflowRunStatus::UserPaused);
        assert!(paused.execution_suspended);
        assert!(manager.active.is_empty());
    }

    #[tokio::test]
    async fn released_root_cold_start_pauses_unstarted_workflow_for_explicit_same_run_resume() {
        let (mut manager, _rx) = test_manager(None);
        let script = r#"let meta = #{ name: "restore-test", description: "d" };
complete("restored after explicit resume");"#;
        let args = serde_json::json!({"fixed": true});
        let run_id = "wf_released_root_recovery";
        let operation_id = "root-released-before-workflow";
        let root_session_id = "test-session";
        manager.store.register(run_id, script, &args).unwrap();
        let initial_state = {
            let mut tracker = manager.tracker.lock();
            tracker.start_run(
                run_id.into(),
                "restore-test".into(),
                "restore the same Run".into(),
                Vec::new(),
                Some(41),
                None,
                crate::session::workflow::tracker::test_runtime_route(),
            );
            tracker.set_definition_provenance(
                run_id,
                tools::implementations::grow_build::workflow::WorkflowDefinitionId::new(
                    "session:restore-test",
                ),
                tools::implementations::grow_build::workflow::WorkflowScope::Session,
                super::super::registry::content_hash(script),
            );
            tracker.set_max_concurrency(run_id, 2).unwrap()
        };
        let initial_manifest =
            serde_json::to_value(manager.store.initial_manifest(&initial_state).unwrap()).unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: run_id.into(),
                    execution_epoch: 0,
                    name: initial_state.name.clone(),
                    objective: initial_state.objective.clone(),
                    script_hash: super::super::store::workflow_script_hash(script),
                    args_hash: super::super::store::workflow_args_hash(&args).unwrap(),
                    initial_manifest,
                },
            ))
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                    operation_id: operation_id.into(),
                    root_session_id: root_session_id.into(),
                },
            ))
            .await
            .unwrap();
        manager.tracker.lock().suspend_execution(run_id).unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: run_id.into(),
                    execution_epoch: 0,
                    status: chat_state::WorkflowExecutionStatus::Suspended,
                    execution_suspended: true,
                    handoff: chat_state::WorkflowTurnHandoff::None,
                    duration_ms: 0,
                    message: None,
                },
            ))
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::CommandsSettled {
                    operation_id: operation_id.into(),
                    commands: Vec::new(),
                },
            ))
            .await
            .unwrap();
        let checkpoint = manager
            .timeline
            .checkpoint_runtime(
                operation_id.into(),
                "a".repeat(64),
                None,
                None,
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            )
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::TreeSuspended {
                    operation_id: operation_id.into(),
                    nodes: vec![chat_state::session_runtime::RuntimeNodeRef {
                        session_id: root_session_id.into(),
                        checkpoint_seq: checkpoint.seq,
                    }],
                },
            ))
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::RestorePrepared {
                    operation_id: operation_id.into(),
                    instance_id: "recovery-instance".into(),
                },
            ))
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::Resumed {
                    operation_id: operation_id.into(),
                    instance_id: "recovery-instance".into(),
                },
            ))
            .await
            .unwrap();
        manager
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::ExecutionReleased {
                    operation_id: operation_id.into(),
                    instance_id: "recovery-instance".into(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(
            manager.timeline.runtime_state().await.unwrap().phase,
            chat_state::session_runtime::RuntimePhase::Running
        );

        manager
            .pause_released_suspended_runs_for_manual_resume()
            .await
            .unwrap();
        let paused = manager.tracker.lock().get(run_id).unwrap();
        assert_eq!(paused.status, WorkflowRunStatus::InfraPaused);
        assert!(!paused.execution_suspended);
        assert_eq!(paused.execution_epoch, 1);
        assert!(
            manager.active.is_empty(),
            "cold reconciliation must not dispatch host work"
        );
        let timeline =
            chat_state::Timeline::from_events(manager.timeline.timeline_events().await.unwrap())
                .unwrap();
        let lifecycle = timeline.workflow_lifecycle(run_id).unwrap();
        assert_eq!(lifecycle.execution_epoch, 1);
        assert_eq!(
            lifecycle.status,
            Some(chat_state::WorkflowExecutionStatus::InfraPaused)
        );
        assert!(!lifecycle.execution_suspended);
        assert!(!lifecycle.open && !lifecycle.closed);
        assert!(manager.store.script_for(run_id).is_some());
        assert_eq!(manager.store.args_for(run_id), Some(args.clone()));

        let paused_revision = paused.revision;
        let event_count = manager.timeline.timeline_events().await.unwrap().len();
        manager
            .pause_released_suspended_runs_for_manual_resume()
            .await
            .unwrap();
        assert_eq!(
            manager.tracker.lock().get(run_id).unwrap().revision,
            paused_revision
        );
        assert_eq!(
            manager.timeline.timeline_events().await.unwrap().len(),
            event_count
        );

        let (_, outcome) = manager
            .launch(
                resolve_inline(script.into()).unwrap(),
                LaunchSpec {
                    objective: "restore the same Run".into(),
                    args,
                    agent_budget: None,
                    max_concurrency: Some(2),
                    resume_run_id: Some(run_id.into()),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            outcome.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));
        assert_eq!(manager.tracker.lock().list().len(), 1);
        let resumed = manager.tracker.lock().get(run_id).unwrap();
        assert_eq!(resumed.status, WorkflowRunStatus::Complete);
        assert_eq!(resumed.execution_epoch, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn workflow_handoff_retries_until_actor_admission_ack() {
        let mut tracker = WorkflowTracker::default();
        let mut state = tracker.start_run(
            "workflow-retry".into(),
            "retry".into(),
            "verify delivery".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        state.status = WorkflowRunStatus::Complete;
        state.turn_handoff = chat_state::WorkflowTurnHandoff::Completion;
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let delivery = tokio::spawn(deliver_workflow_handoff_acknowledged(cmd_tx, state.clone()));

        let crate::session::commands::SessionCommand::WorkflowHandoffReady {
            state: first,
            respond_to: first_ack,
            ..
        } = cmd_rx.recv().await.expect("first delivery")
        else {
            panic!("expected WorkflowHandoffReady");
        };
        first_ack.send(Err("disk unavailable".into())).unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
        tokio::task::yield_now().await;

        let crate::session::commands::SessionCommand::WorkflowHandoffReady {
            state: retry,
            respond_to: retry_ack,
            ..
        } = cmd_rx.recv().await.expect("retry delivery")
        else {
            panic!("expected WorkflowHandoffReady retry");
        };
        assert_eq!(retry.run_id, first.run_id);
        assert_eq!(retry.execution_epoch, first.execution_epoch);
        retry_ack.send(Ok(())).unwrap();
        delivery.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn workflow_handoff_retries_past_three_failures_and_caps_backoff() {
        let mut tracker = WorkflowTracker::default();
        let mut state = tracker.start_run(
            "workflow-failed".into(),
            "failed".into(),
            "verify persistent delivery".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        state.status = WorkflowRunStatus::Complete;
        state.turn_handoff = chat_state::WorkflowTurnHandoff::Completion;
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let delivery = tokio::spawn(deliver_workflow_handoff_acknowledged(cmd_tx, state));

        let retry_delays_ms = [100_u64, 200, 400, 800, 1_600, 2_000, 2_000];
        for (attempt, retry_delay_ms) in retry_delays_ms.into_iter().enumerate() {
            let command = if attempt == 0 {
                cmd_rx.recv().await.expect("first workflow delivery")
            } else {
                cmd_rx
                    .try_recv()
                    .expect("workflow retry must be ready after its backoff")
            };
            let crate::session::commands::SessionCommand::WorkflowHandoffReady {
                respond_to, ..
            } = command
            else {
                panic!("expected WorkflowHandoffReady");
            };
            respond_to.send(Err("disk unavailable".into())).unwrap();
            tokio::task::yield_now().await;
            tokio::time::advance(std::time::Duration::from_millis(retry_delay_ms)).await;
            tokio::task::yield_now().await;
        }

        let crate::session::commands::SessionCommand::WorkflowHandoffReady { respond_to, .. } =
            cmd_rx
                .try_recv()
                .expect("delivery must continue after capped backoff")
        else {
            panic!("expected WorkflowHandoffReady after capped backoff");
        };
        respond_to.send(Ok(())).unwrap();
        delivery.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn workflow_handoff_delivery_stops_when_the_mailbox_closes() {
        let mut tracker = WorkflowTracker::default();
        let mut state = tracker.start_run(
            "workflow-closed-mailbox".into(),
            "closed-mailbox".into(),
            "verify mailbox closure".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        state.status = WorkflowRunStatus::Complete;
        state.turn_handoff = chat_state::WorkflowTurnHandoff::Completion;
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let delivery = tokio::spawn(deliver_workflow_handoff_acknowledged(cmd_tx, state));
        let crate::session::commands::SessionCommand::WorkflowHandoffReady { respond_to, .. } =
            cmd_rx.recv().await.expect("first delivery")
        else {
            panic!("expected WorkflowHandoffReady");
        };
        respond_to.send(Err("transient failure".into())).unwrap();
        drop(cmd_rx);
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
        delivery.await.unwrap();
    }

    #[tokio::test]
    async fn launch_completes_and_updates_tracker() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();
        let (run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        let outcome = outcome_rx.await.unwrap();
        assert!(matches!(outcome, WorkflowOutcome::Completed { .. }));
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::Complete
        );
        assert_eq!(state.result_summary.as_deref(), Some("done"));
        assert_eq!(state.max_concurrency, 3);
        assert!(
            dir.path()
                .join("workflows")
                .join(&run_id)
                .join("script.rhai")
                .exists()
        );
        let trajectory = manager.timeline.trajectory().await.unwrap();
        let workflow_rows = trajectory
            .rows
            .iter()
            .filter(|row| row.actor == format!("workflow:{run_id}"))
            .collect::<Vec<_>>();
        assert_eq!(workflow_rows.len(), 2);
        assert_eq!(workflow_rows[0].state, "running");
        assert_eq!(workflow_rows[1].state, "complete");
    }

    #[tokio::test]
    async fn oversized_initial_projection_rolls_back_before_timeline_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();
        let error = manager
            .launch(
                resolved,
                LaunchSpec {
                    objective: "x".repeat(chat_state::MAX_WORKFLOW_INITIAL_MANIFEST_BYTES),
                    ..spec()
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(error, LaunchError::Store(_)));
        assert!(manager.tracker.lock().list().is_empty());
        assert!(manager.active.is_empty());
        let workflow_dirs = std::fs::read_dir(dir.path().join("workflows"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(workflow_dirs.len(), 1);
        assert!(manager.script_copy_for(&workflow_dirs[0]).is_none());
        assert!(
            manager
                .timeline
                .trajectory()
                .await
                .unwrap()
                .rows
                .iter()
                .all(|row| row.class != "workflow")
        );
    }

    #[tokio::test]
    async fn acknowledged_spawn_with_lost_caller_reply_keeps_recovery_source() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let (persistence, mut records) =
            chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
        let cancellation = CancellationToken::new();
        manager.timeline =
            test_timeline_with_persistence(Box::new(persistence), cancellation.clone());
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();

        let mut launch = Box::pin(manager.launch(resolved, spec()));
        let acknowledgement = tokio::select! {
            result = &mut launch => panic!("launch finished before durable Spawned ACK: {result:?}"),
            acknowledgement = records.next_timeline_ack() => acknowledgement.unwrap(),
        };
        let spawn = records
            .drain()
            .into_iter()
            .find_map(|record| match record {
                chat_state::PersistenceRecord::Timeline(event)
                    if matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Workflow(
                            chat_state::WorkflowEvent::Spawned { .. }
                        )
                    ) =>
                {
                    Some(event)
                }
                _ => None,
            })
            .expect("Spawned offered to durable writer");
        let chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
            run_id,
            ..
        }) = &spawn.kind
        else {
            unreachable!()
        };
        let run_id = run_id.clone();

        acknowledgement.send(Ok(())).unwrap();
        cancellation.cancel();
        let error = launch.await.unwrap_err();

        assert!(matches!(error, LaunchError::Timeline(_)));
        assert!(error.to_string().contains(&run_id));
        assert!(error.to_string().contains("uncertain"));
        assert!(manager.store.script_for(&run_id).is_some());
        assert!(manager.tracker.lock().get(&run_id).is_some());
        assert!(
            !dir.path()
                .join("workflows")
                .join(&run_id)
                .join("cleared")
                .exists()
        );
        let replayed = chat_state::Timeline::from_events(vec![*spawn]).unwrap();
        assert!(replayed.workflow_lifecycle(&run_id).is_some());
    }

    #[tokio::test]
    async fn terminal_manifest_failure_still_closes_timeline_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx, _cancels) =
            test_manager_with_persistence(Some(dir.path().to_path_buf()), true);
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();
        let (run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Failed { .. }
        ));
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted
        );
        let trajectory = manager.timeline.trajectory().await.unwrap();
        let last = trajectory
            .rows
            .iter()
            .rfind(|row| row.actor == format!("workflow:{run_id}"))
            .unwrap();
        assert_eq!(last.state, "interrupted");
        assert!(!trajectory.open_workflows.contains(&run_id));
    }

    #[tokio::test]
    async fn executor_resource_failure_interrupts_the_durable_spawn_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let session_directory = test_session_directory(Some(dir.path().to_path_buf()));
        let (persist_tx, mut persist_rx) = mpsc::unbounded_channel();
        let run_root = dir.path().to_path_buf();
        tokio::spawn(async move {
            let mut injected = false;
            while let Some(message) = persist_rx.recv().await {
                if let PersistenceMsg::WorkflowRunStateAndAck {
                    manifest,
                    respond_to,
                    ..
                } = message
                {
                    if !injected && manifest.state.status == WorkflowRunStatus::Active {
                        let scratch = run_root
                            .join("workflows")
                            .join(&manifest.state.run_id)
                            .join("scratch");
                        std::fs::write(scratch, b"not a directory").unwrap();
                        injected = true;
                    }
                    let _ = respond_to.send(Ok(()));
                }
            }
        });
        let store = WorkflowRunStore::new(session_directory.clone(), persist_tx.clone());
        let tracker = Arc::new(parking_lot::Mutex::new(WorkflowTracker::default()));
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let notify = WorkflowNotifySender::new(
            agent_client_protocol::schema::v1::SessionId::new("scratch-failure-test"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            sampler::PreviewEventBudget::default(),
            Arc::new(parking_lot::Mutex::new(None)),
            store.clone(),
        );
        let mut manager = WorkflowManager::new(
            "scratch-failure-test".into(),
            session_directory,
            dir.path().to_path_buf(),
            tracker.clone(),
            store,
            notify,
            mpsc::unbounded_channel().0,
            Arc::new(|_, _, _| {}),
            mpsc::unbounded_channel().0,
            test_timeline(),
            HashMap::new(),
            crate::session::workflow::tracker::test_runtime_route(),
            crate::session::workflow::tracker::WorkflowAgentCatalogSource::for_test(
                dir.path().to_path_buf(),
            ),
        );
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();

        let error = manager.launch(resolved, spec()).await.unwrap_err();

        assert!(error.to_string().contains("executor resources"), "{error}");
        let states = tracker.lock().list();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].status, WorkflowRunStatus::Interrupted);
        assert!(manager.active.is_empty());
        let trajectory = manager.timeline.trajectory().await.unwrap();
        let rows = trajectory
            .rows
            .iter()
            .filter(|row| row.actor == format!("workflow:{}", states[0].run_id))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].state, "running");
        assert_eq!(rows[1].state, "interrupted");
        assert!(!trajectory.open_workflows.contains(&states[0].run_id));
    }

    #[tokio::test]
    async fn launch_persists_explicit_max_concurrency_independently_of_budget() {
        let (mut manager, _rx) = test_manager(None);
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();
        let (run_id, outcome_rx) = manager
            .launch(
                resolved,
                LaunchSpec {
                    max_concurrency: Some(7),
                    agent_budget: Some(29),
                    ..spec()
                },
            )
            .await
            .unwrap();
        let _ = outcome_rx.await.unwrap();
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(state.max_concurrency, 7);
        assert_eq!(state.agent_budget, Some(29));
    }

    #[tokio::test]
    async fn plain_resume_uses_immutable_script_not_edited_projection() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\nawait_user(\"user\", \"pause\");\ncomplete(\"original\");";
        let (run_id, outcome_rx) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::AwaitingUser { .. }
        ));
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().turn_handoff,
            chat_state::WorkflowTurnHandoff::AttentionRequired
        );
        std::fs::write(
            dir.path()
                .join("workflows")
                .join(&run_id)
                .join("script.rhai"),
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"edited\");",
        )
        .unwrap();

        let (_same_id, outcome_rx) = manager
            .launch(
                resolve_inline(
                    "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"caller copy\");"
                        .into(),
                )
                .unwrap(),
                LaunchSpec {
                    resume_run_id: Some(run_id),
                    ..spec()
                },
            )
            .await
            .unwrap();
        match outcome_rx.await.unwrap() {
            WorkflowOutcome::Completed { result } => {
                assert_eq!(result, serde_json::json!("original"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resume_waits_for_terminal_manifest_and_timeline_acknowledgment() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let (persist_tx, mut persist_rx) = mpsc::unbounded_channel();
        manager.store = WorkflowRunStore::new(manager.session_directory.clone(), persist_tx);
        let (ready_tx, ready_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        tokio::spawn(async move {
            let mut barrier = Some((ready_tx, release_rx));
            while let Some(message) = persist_rx.recv().await {
                if let crate::session::persistence::PersistenceMsg::WorkflowRunStateAndAck {
                    manifest,
                    respond_to,
                    ..
                } = message
                {
                    if manifest.state.status.is_resumable()
                        && let Some((ready, release)) = barrier.take()
                    {
                        let _ = ready.send(());
                        let _ = release.await;
                    }
                    let _ = respond_to.send(Ok(()));
                }
            }
        });
        let script = "let meta = #{ name: \"t\", description: \"d\" }; await_user(\"user\", \"pause\"); complete(\"done\");";
        let (run_id, first_outcome) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        ready_rx.await.unwrap();
        assert!(
            manager
                .tracker
                .lock()
                .get(&run_id)
                .unwrap()
                .status
                .is_resumable()
        );
        let resume = manager.launch(
            resolve_inline(script.into()).unwrap(),
            LaunchSpec {
                resume_run_id: Some(run_id),
                ..spec()
            },
        );
        tokio::pin!(resume);
        tokio::select! {
            result = &mut resume => panic!("resume crossed pending terminal: {:?}", result.err()),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {},
        }
        release_tx.send(()).unwrap();
        let (_, second_outcome) = resume.await.unwrap();
        assert!(matches!(
            first_outcome.await.unwrap(),
            WorkflowOutcome::AwaitingUser { .. }
        ));
        assert!(matches!(
            second_outcome.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));
    }

    #[tokio::test]
    async fn resume_propagates_terminal_ack_failure_without_advancing_epoch() {
        for dropped in [false, true] {
            let (mut manager, _rx) = test_manager(None);
            let script =
                "let meta = #{ name: \"t\", description: \"d\" }; await_user(\"user\", \"pause\");";
            let (run_id, outcome) = manager
                .launch(resolve_inline(script.into()).unwrap(), spec())
                .await
                .unwrap();
            outcome.await.unwrap();
            let epoch = manager.tracker.lock().execution_epoch(&run_id);
            let (done_tx, done_rx) = oneshot::channel();
            manager.test_insert_active_run(run_id.clone(), done_rx);
            if dropped {
                drop(done_tx);
            } else {
                done_tx.send(Err("terminal disk failure".into())).unwrap();
            }
            let result = manager
                .launch(
                    resolve_inline(script.into()).unwrap(),
                    LaunchSpec {
                        resume_run_id: Some(run_id.clone()),
                        ..spec()
                    },
                )
                .await;
            assert!(matches!(result, Err(LaunchError::Timeline(_))));
            assert_eq!(manager.tracker.lock().execution_epoch(&run_id), epoch);
        }
    }

    #[tokio::test]
    async fn pause_waits_for_the_watcher_terminal_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let run_id = "wf_pause_eager".to_string();
        manager
            .store
            .register(
                &run_id,
                "let meta = #{ name: \"t\", description: \"d\" };",
                &serde_json::json!({}),
            )
            .unwrap();
        manager.tracker.lock().start_run(
            run_id.clone(),
            "t".into(),
            "obj".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        let (done_tx, done_rx) = oneshot::channel();
        manager.test_insert_active_run(run_id.clone(), done_rx);
        let mut terminal = manager.tracker.lock().get(&run_id).unwrap();
        terminal.status = crate::session::workflow::tracker::WorkflowRunStatus::UserPaused;
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            done_tx.send(Ok(terminal)).unwrap();
        });

        let state = manager.pause(&run_id).await.unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::UserPaused
        );
        assert_eq!(state.turn_handoff, chat_state::WorkflowTurnHandoff::None);
        assert!(!manager.active.contains_key(&run_id));
    }

    #[tokio::test]
    async fn pause_marks_user_paused_and_resume_replays() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\nlet r = agent(\"work\");\ncomplete(r.output);";
        let resolved = resolve_inline(script.into()).unwrap();
        let (run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        use tools::implementations::grow_build::task::types::SubagentEvent;
        let spawn_req = subagent_rx.recv().await.expect("spawn request");
        let SubagentEvent::Spawn(_spawn) = spawn_req else {
            panic!("expected spawn request");
        };
        assert!(manager.pause(&run_id).await.is_ok());
        let outcome = outcome_rx.await.unwrap();
        assert!(matches!(outcome, WorkflowOutcome::Cancelled));
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::UserPaused,
            "pause intent must map Cancelled → UserPaused"
        );
        assert_eq!(state.turn_handoff, chat_state::WorkflowTurnHandoff::None);

        let resolved = resolve_inline(script.into()).unwrap();
        let (_run_id2, outcome_rx) = manager
            .launch(
                resolved,
                LaunchSpec {
                    resume_run_id: Some(run_id.clone()),
                    ..spec()
                },
            )
            .await
            .unwrap();
        let spawn_req = subagent_rx.recv().await.expect("respawned agent");
        use tools::implementations::grow_build::task::types::SubagentResult;
        if let SubagentEvent::Spawn(req) = spawn_req {
            let id = req.id.clone();
            let _ = req.result_tx.send(SubagentResult {
                success: true,
                output: std::sync::Arc::from("resumed output"),
                subagent_id: id,
                ..Default::default()
            });
        } else {
            panic!("expected spawn event");
        }
        let outcome = outcome_rx.await.unwrap();
        match outcome {
            WorkflowOutcome::Completed { result } => {
                assert_eq!(result, serde_json::json!("resumed output"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resume_reconciles_agents_used_from_journal_no_double_charge() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\nlet r = agent(\"work\");\ncomplete(r.output);";
        let (run_id, outcome_rx) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();

        let SubagentEvent::Spawn(_first) = subagent_rx.recv().await.expect("first spawn") else {
            panic!("expected spawn event");
        };
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().agents_used,
            1,
            "the live agent reserves one slot before it spawns"
        );

        assert!(manager.pause(&run_id).await.is_ok());
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Cancelled
        ));
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().agents_used,
            1,
            "cancel tears the host down before the release lands, so the reserved slot leaks in memory"
        );

        let (_resumed_id, outcome_rx) = manager
            .launch(
                resolve_inline(script.into()).unwrap(),
                LaunchSpec {
                    resume_run_id: Some(run_id.clone()),
                    ..spec()
                },
            )
            .await
            .unwrap();
        let SubagentEvent::Spawn(req) = subagent_rx.recv().await.expect("respawned agent") else {
            panic!("expected respawn event");
        };
        let id = req.id.clone();
        let _ = req.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("resumed output"),
            subagent_id: id,
            ..Default::default()
        });
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));

        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().agents_used,
            1,
            "resume reconciles agents_used from the journal (0 journaled) then re-reserves once; \
             without the reconcile the leaked slot double-charges to 2"
        );
    }

    #[tokio::test]
    async fn failed_run_resumes_and_reexecutes_failed_host_call_live() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\n\
                      let content = read_scratch_file(\"data.txt\");\n\
                      complete(content);";
        let (run_id, outcome_rx) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        match outcome_rx.await.unwrap() {
            WorkflowOutcome::Failed { error } => {
                assert!(error.contains("scratch"), "{error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::Failed
        );
        let journal_path = dir
            .path()
            .join("workflows")
            .join(&run_id)
            .join("journal.jsonl");
        assert!(
            std::fs::read_to_string(&journal_path)
                .unwrap()
                .contains("__workflow_host_error"),
            "the uncaught host error must be journaled as a trailing sentinel"
        );

        let scratch = dir.path().join("workflows").join(&run_id).join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(scratch.join("data.txt"), "hello").unwrap();

        let (_same_id, outcome_rx) = manager
            .launch(
                resolve_inline(script.into()).unwrap(),
                LaunchSpec {
                    resume_run_id: Some(run_id.clone()),
                    ..spec()
                },
            )
            .await
            .unwrap();
        match outcome_rx.await.unwrap() {
            WorkflowOutcome::Completed { result } => {
                assert_eq!(
                    result,
                    serde_json::json!("hello"),
                    "the failed host call must go live instead of replaying the sentinel"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::Complete
        );
        assert!(
            !std::fs::read_to_string(&journal_path)
                .unwrap()
                .contains("__workflow_host_error"),
            "the trailing sentinel must be pruned and replaced by the live result"
        );
    }

    #[tokio::test]
    async fn failed_inactive_run_can_be_permanently_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\n\
                      read_scratch_file(\"missing.txt\");";
        let (run_id, outcome_rx) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Failed { .. }
        ));
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::Failed
        );

        assert!(manager.cancel(&run_id).await.is_ok());
        let cancelled = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            cancelled.status,
            crate::session::workflow::tracker::WorkflowRunStatus::Cancelled
        );
        assert_eq!(
            cancelled.turn_handoff,
            chat_state::WorkflowTurnHandoff::None
        );
        let trajectory = manager.timeline.trajectory().await.unwrap();
        let last = trajectory
            .rows
            .iter()
            .rfind(|row| row.actor == format!("workflow:{run_id}"))
            .unwrap();
        assert_eq!(last.state, "cancelled");
        assert!(!trajectory.open_workflows.contains(&run_id));
    }

    #[tokio::test]
    async fn completed_cancelled_and_interrupted_runs_are_not_resumable() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\nlet a = agent(\"step one\");\ncomplete(a.output);";
        let (run_id, outcome_rx) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        let SubagentEvent::Spawn(req) = subagent_rx.recv().await.expect("first spawn") else {
            panic!("expected spawn event");
        };
        let id = req.id.clone();
        let _ = req.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("one"),
            subagent_id: id,
            ..Default::default()
        });
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));

        let state = manager.tracker.lock().get(&run_id).unwrap();
        for (status, turn_handoff) in [
            (
                crate::session::workflow::tracker::WorkflowRunStatus::Complete,
                chat_state::WorkflowTurnHandoff::Completion,
            ),
            (
                crate::session::workflow::tracker::WorkflowRunStatus::Cancelled,
                chat_state::WorkflowTurnHandoff::None,
            ),
            (
                crate::session::workflow::tracker::WorkflowRunStatus::Interrupted,
                chat_state::WorkflowTurnHandoff::Completion,
            ),
        ] {
            let mut restored = state.clone();
            restored.status = status;
            restored.turn_handoff = turn_handoff;
            let original_tracker = manager.tracker.clone();
            manager.tracker = Arc::new(parking_lot::Mutex::new(
                WorkflowTracker::from_snapshot(vec![restored]).unwrap(),
            ));
            let err = manager
                .launch(
                    resolve_inline(script.into()).unwrap(),
                    LaunchSpec {
                        resume_run_id: Some(run_id.clone()),
                        ..spec()
                    },
                )
                .await
                .unwrap_err();
            manager.tracker = original_tracker;
            assert!(
                matches!(err, LaunchError::NotResumable(_)),
                "{status:?}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn shutdown_timeout_marks_active_run_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let run_id = "wf_timeout".to_string();
        manager
            .store
            .register(
                &run_id,
                "let meta = #{ name: \"t\", description: \"d\" };",
                &serde_json::json!({}),
            )
            .unwrap();
        manager.tracker.lock().start_run(
            run_id.clone(),
            "t".into(),
            "obj".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        let (_done_tx, done_rx) = oneshot::channel();
        manager.test_insert_active_run(run_id.clone(), done_rx);

        let result = manager
            .cancel_all_and_drain(std::time::Duration::from_millis(1))
            .await;
        assert_eq!(
            result.unwrap_err(),
            vec![format!("{run_id}: terminal drain timed out")]
        );
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted
        );
        assert!(!state.status.is_paused());
    }

    #[tokio::test]
    async fn shutdown_drain_preserves_terminal_watcher_error() {
        let (mut manager, _rx) = test_manager(None);
        let run_id = "wf_done_error".to_string();
        let (done_tx, done_rx) = oneshot::channel();
        manager.test_insert_active_run(run_id.clone(), done_rx);
        done_tx
            .send(Err("terminal Timeline append failed".into()))
            .unwrap();

        let result = manager
            .cancel_all_and_drain(std::time::Duration::from_secs(1))
            .await;
        assert_eq!(
            result.unwrap_err(),
            vec![format!("{run_id}: terminal Timeline append failed")]
        );
    }

    #[tokio::test]
    async fn shutdown_drain_preserves_closed_terminal_watcher() {
        let (mut manager, _rx) = test_manager(None);
        let run_id = "wf_done_closed".to_string();
        let (done_tx, done_rx) = oneshot::channel::<Result<WorkflowRunState, String>>();
        manager.test_insert_active_run(run_id.clone(), done_rx);
        drop(done_tx);

        let result = manager
            .cancel_all_and_drain(std::time::Duration::from_secs(1))
            .await;
        let failure = result.unwrap_err().pop().expect("closed watcher failure");
        assert!(failure.starts_with(&format!("{run_id}: ")));
        assert!(failure.contains("terminal watcher channel closed"));
    }

    #[tokio::test]
    async fn closed_admission_rejects_new_workflow_before_workspace_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _rx) = test_manager(Some(dir.path().to_path_buf()));
        let generation = manager.close_admission();
        assert_eq!(generation, 1);
        let result = manager
            .launch(
                resolve_inline(
                    "let meta = #{ name: \"rejected\", description: \"no-op\" }; complete(\"no\");"
                        .into(),
                )
                .unwrap(),
                spec(),
            )
            .await;
        assert!(matches!(result, Err(LaunchError::SessionShuttingDown)));
        assert!(dir.path().join("workflows").read_dir().is_err());
    }

    #[tokio::test]
    async fn workflow_spawns_await_to_completion() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        manager.set_next_run_route(
            WorkflowRuntimeRoute::for_test(
                "test-model",
                Some(sampling_types::ReasoningEffort::High),
                sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
            )
            .unwrap(),
        );
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"work\");\n\
             complete(r.output);"
                .into(),
        )
        .unwrap();
        let (_run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        let spawn_req = subagent_rx.recv().await.expect("spawn event");
        let SubagentEvent::Spawn(req) = spawn_req else {
            panic!("expected spawn event");
        };
        assert!(
            req.await_to_completion,
            "workflow agent spawns must disable the ordinary task-tool await budget"
        );
        assert!(
            req.owner.is_workflow(),
            "workflow agent spawns must carry run lifecycle ownership"
        );
        assert_eq!(
            req.runtime_overrides.model_override_provenance,
            tools::implementations::grow_build::task::types::ModelOverrideProvenance::Harness,
            "the Run-owned route is trusted harness provenance"
        );
        assert_eq!(req.runtime_overrides.model.as_deref(), Some("test-model"));
        assert_eq!(
            req.runtime_overrides.model_transport_key.as_ref(),
            Some(&sampling_types::ModelImageInputKey::new(
                "test-model",
                "responses",
                "test-endpoint",
            )),
            "the child must carry the Run's immutable provider route"
        );
        assert_eq!(
            req.runtime_overrides
                .reasoning_effort
                .as_ref()
                .and_then(|effort| effort.as_deref()),
            Some("high")
        );
        let id = req.id.clone();
        let _ = req.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("slow but done"),
            subagent_id: id,
            ..Default::default()
        });
        let outcome = outcome_rx.await.unwrap();
        assert!(matches!(outcome, WorkflowOutcome::Completed { .. }));
    }

    #[tokio::test]
    async fn workflow_run_snapshots_reasoning_disabled() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        manager.set_next_run_route(
            WorkflowRuntimeRoute::for_test(
                "test-model",
                None,
                sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
            )
            .unwrap(),
        );
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"work\");\n\
             complete(r.output);"
                .into(),
        )
        .unwrap();
        let (_run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        let SubagentEvent::Spawn(request) = subagent_rx.recv().await.expect("spawn event") else {
            panic!("expected spawn event");
        };
        assert_eq!(
            request.runtime_overrides.reasoning_effort,
            Some(None),
            "a Run that disabled reasoning must not inherit a later Agent/model default"
        );
        let subagent_id = request.id.clone();
        let _ = request.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("done"),
            subagent_id,
            ..Default::default()
        });
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));
    }

    #[tokio::test]
    async fn session_route_changes_apply_only_to_future_workflow_runs() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let first_route = WorkflowRuntimeRoute::for_test(
            "first-model",
            Some(sampling_types::ReasoningEffort::Low),
            sampling_types::ModelImageInputKey::new("first-model", "responses", "test-endpoint"),
        )
        .unwrap();
        let mut expected_first_route = first_route.clone();
        expected_first_route
            .capture_agent_definitions(&manager.agent_catalog_source)
            .await
            .unwrap();
        manager.set_next_run_route(first_route.clone());
        let completed = || {
            resolve_inline(
                "let meta = #{ name: \"route\", description: \"route snapshot\" };\n\
                 complete(\"done\");"
                    .into(),
            )
            .unwrap()
        };
        let (first_id, first_outcome) = manager.launch(completed(), spec()).await.unwrap();

        let second_route = WorkflowRuntimeRoute::for_test(
            "second-model",
            Some(sampling_types::ReasoningEffort::Max),
            sampling_types::ModelImageInputKey::new("second-model", "responses", "test-endpoint"),
        )
        .unwrap();
        let mut expected_second_route = second_route.clone();
        expected_second_route
            .capture_agent_definitions(&manager.agent_catalog_source)
            .await
            .unwrap();
        manager.set_next_run_route(second_route.clone());
        let (second_id, second_outcome) = manager.launch(completed(), spec()).await.unwrap();
        let _ = first_outcome.await.unwrap();
        let _ = second_outcome.await.unwrap();

        let tracker = manager.tracker.lock();
        assert_eq!(
            tracker.get(&first_id).unwrap().runtime_route,
            expected_first_route
        );
        assert_eq!(
            tracker.get(&second_id).unwrap().runtime_route,
            expected_second_route
        );
    }

    #[tokio::test]
    async fn definition_model_override_does_not_inherit_run_effort() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let explicit_transport = sampling_types::ModelImageInputKey::new(
            "explicit-wire-model",
            "responses",
            "explicit-endpoint",
        );
        manager.set_next_run_route(
            WorkflowRuntimeRoute::for_test(
                "run-model",
                Some(sampling_types::ReasoningEffort::High),
                sampling_types::ModelImageInputKey::new("run-model", "responses", "test-endpoint"),
            )
            .unwrap()
            .with_test_model(
                "explicit-model",
                Some(sampling_types::ReasoningEffort::Low),
                explicit_transport.clone(),
            )
            .unwrap(),
        );
        let resolved = resolve_inline(
            "let meta = #{ name: \"override\", description: \"explicit model\" };\n\
             let r = agent(\"work\", #{ model: \"explicit-model\" });\n\
             complete(r.output);"
                .into(),
        )
        .unwrap();
        let (_run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();
        let SubagentEvent::Spawn(request) = subagent_rx.recv().await.unwrap() else {
            panic!("expected Workflow subagent spawn");
        };
        assert_eq!(
            request.runtime_overrides.model.as_deref(),
            Some("explicit-model")
        );
        assert_eq!(
            request.runtime_overrides.model_override_provenance,
            tools::implementations::grow_build::task::types::ModelOverrideProvenance::Tool,
        );
        assert_eq!(
            request.runtime_overrides.model_transport_key,
            Some(explicit_transport)
        );
        assert_eq!(
            request
                .runtime_overrides
                .reasoning_effort
                .as_ref()
                .and_then(|effort| effort.as_deref()),
            Some("low"),
            "an explicit model keeps its own snapshotted policy instead of inheriting Run high"
        );
        let subagent_id = request.id.clone();
        let _ = request.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("done"),
            subagent_id,
            ..Default::default()
        });
        assert!(matches!(
            outcome_rx.await.unwrap(),
            WorkflowOutcome::Completed { .. }
        ));
    }

    #[tokio::test]
    async fn definition_model_added_after_launch_is_not_admitted_into_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        manager.set_next_run_route(
            WorkflowRuntimeRoute::for_test(
                "run-model",
                None,
                sampling_types::ModelImageInputKey::new("run-model", "responses", "run-endpoint"),
            )
            .unwrap(),
        );
        let resolved = resolve_inline(
            "let meta = #{ name: \"frozen\", description: \"route snapshot\" };\n\
             let r = agent(\"work\", #{ model: \"later-model\" });\n\
             complete(r.output);"
                .into(),
        )
        .unwrap();
        let (_run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        let outcome = outcome_rx.await.unwrap();
        assert!(matches!(
            outcome,
            WorkflowOutcome::Failed { ref error }
                if error.contains("was not present in the Workflow Run route snapshot")
        ));
        assert!(
            subagent_rx.try_recv().is_err(),
            "an out-of-snapshot model must fail before child admission"
        );
    }

    #[tokio::test]
    async fn active_run_admission_is_bounded_per_session() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let script = "let meta = #{ name: \"t\", description: \"d\" };\n\
                      let r = agent(\"work\");\ncomplete(r.output);";
        let mut outcomes = Vec::new();
        let mut spawned = Vec::new();
        for _ in 0..WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION {
            let (_, outcome) = manager
                .launch(resolve_inline(script.into()).unwrap(), spec())
                .await
                .unwrap();
            outcomes.push(outcome);
            spawned.push(subagent_rx.recv().await.expect("spawn event"));
        }
        let error = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap_err();
        assert!(matches!(error, LaunchError::TooManyActiveRuns));
        drop(spawned);
        let _ = manager
            .cancel_all_and_drain(std::time::Duration::from_secs(1))
            .await;
        drop(outcomes);
    }

    #[tokio::test]
    async fn untrusted_workflow_cannot_fork_parent_context() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"work\", #{ fork_context: true });\n\
             complete(r.output);"
                .into(),
        )
        .unwrap();
        let (_run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        match outcome_rx.await.unwrap() {
            WorkflowOutcome::Failed { error } => {
                assert!(error.contains("fork_context is restricted to built-in workflows"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(
            subagent_rx.try_recv().is_err(),
            "rejected fork_context must not reach the coordinator"
        );
    }

    #[tokio::test]
    async fn output_schema_stays_host_side_with_one_corrective_retry() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"scan\", #{ output_schema: #{ \"type\": \"object\", \
             \"required\": [\"ok\"], \"properties\": #{ \"ok\": #{ \"type\": \"boolean\" } } } });\n\
             complete(r.output.ok);"
                .into(),
        )
        .unwrap();
        let (run_id, outcome_rx) = manager
            .launch(
                resolved,
                LaunchSpec {
                    agent_budget: Some(5),
                    ..spec()
                },
            )
            .await
            .unwrap();

        let SubagentEvent::Spawn(req) = subagent_rx.recv().await.expect("first spawn") else {
            panic!("expected spawn event");
        };
        assert!(
            req.runtime_overrides.output_schema.is_none(),
            "schema must not be passed to the child runtime"
        );
        assert!(
            req.prompt.contains("<output-contract>"),
            "prompt must carry the schema contract"
        );
        assert!(req.resume_from.is_none());
        assert_eq!(req.runtime_overrides.output_token_budget, None);
        let first_id = req.id.clone();
        let _ = req.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("All files scanned, nothing found."),
            subagent_id: first_id.clone(),
            child_session_id: first_id.clone(),
            tokens_used: 100,
            output_tokens_used: 100,
            total_tokens_used: 100,
            ..Default::default()
        });

        let SubagentEvent::Spawn(retry) = subagent_rx.recv().await.expect("corrective retry")
        else {
            panic!("expected retry spawn event");
        };
        assert_eq!(retry.resume_from.as_deref(), Some(first_id.as_str()));
        assert!(retry.prompt.contains("did not satisfy the output contract"));
        assert_eq!(retry.runtime_overrides.output_token_budget, None);
        let retry_id = retry.id.clone();
        let _ = retry.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("```json\n{\"ok\": true}\n```"),
            subagent_id: retry_id.clone(),
            child_session_id: retry_id.clone(),
            tokens_used: 50,
            output_tokens_used: 50,
            total_tokens_used: 50,
            ..Default::default()
        });

        let outcome = outcome_rx.await.unwrap();
        match outcome {
            WorkflowOutcome::Completed { result } => {
                assert_eq!(result, serde_json::json!(true));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        let state = manager.tracker.lock().list().into_iter().next().unwrap();
        assert_eq!(state.agents_used, 1, "schema retry is one logical agent");
        assert_eq!(state.agent_budget, Some(5));
        let journal = std::fs::read_to_string(
            dir.path()
                .join("workflows")
                .join(&run_id)
                .join("journal.jsonl"),
        )
        .unwrap();
        let entry = serde_json::from_str::<workflow::JournalEntry>(journal.lines().last().unwrap())
            .unwrap();
        let agent = serde_json::from_value::<workflow::AgentResult>(entry.result).unwrap();
        assert_eq!(agent.agent_id, retry_id);
    }

    #[tokio::test]
    async fn children_spawn_without_output_clamp() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"work\");\ncomplete(r.output);"
                .into(),
        )
        .unwrap();
        let (_run_id, _outcome_rx) = manager.launch(resolved, spec()).await.unwrap();
        let SubagentEvent::Spawn(req) = subagent_rx.recv().await.expect("spawn") else {
            panic!("expected spawn");
        };
        assert_eq!(req.runtime_overrides.output_token_budget, None);
        let id = req.id.clone();
        let _ = req.result_tx.send(SubagentResult {
            success: true,
            output: std::sync::Arc::from("done"),
            subagent_id: id.clone(),
            child_session_id: id,
            output_tokens_used: 1,
            ..Default::default()
        });
    }

    #[tokio::test]
    async fn cancellation_uses_run_owned_cancel_event_without_parent_detach() {
        use tools::implementations::grow_build::task::types::{
            SubagentCancelTarget, SubagentEvent,
        };

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx, cancels) =
            test_manager_with_cancels(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"work\");\ncomplete(r.output);"
                .into(),
        )
        .unwrap();
        let (run_id, outcome_rx) = manager
            .launch(
                resolved,
                LaunchSpec {
                    agent_budget: Some(100),
                    ..spec()
                },
            )
            .await
            .unwrap();
        let SubagentEvent::Spawn(req) = subagent_rx.recv().await.expect("spawn") else {
            panic!("expected spawn");
        };
        assert!(req.owner.is_workflow());
        assert!(manager.cancel(&run_id).await.is_ok());
        let _ = outcome_rx.await;
        assert!(
            req.cancel_token.is_cancelled(),
            "run cancel must cancel the child token, not silently detach the receiver"
        );
        assert!(
            cancels.lock().iter().any(|target| matches!(
                target,
                SubagentCancelTarget::WorkflowRunId(id) if id == &run_id
            )),
            "cancellation must emit an explicit run-owned cancel event"
        );
    }

    #[tokio::test]
    async fn retired_workflow_child_response_cannot_complete_a_later_run() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx, _) =
            test_manager_with_cancels(Some(dir.path().to_path_buf()));
        let script = r#"let meta = #{ name: "t", description: "d" };
                      let r = agent("work");
                      complete(r.output);"#;
        let (retired_run_id, retired_outcome) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        let SubagentEvent::Spawn(retired_child) =
            subagent_rx.recv().await.expect("retired child spawn")
        else {
            panic!("expected retired child spawn");
        };
        let retired_child_id = retired_child.request.id.clone();
        let late_response = retired_child.result_tx;

        let terminal = manager.cancel(&retired_run_id).await.unwrap();
        assert_eq!(terminal.status, WorkflowRunStatus::Cancelled);
        assert!(matches!(
            retired_outcome.await.unwrap(),
            WorkflowOutcome::Cancelled
        ));
        assert_eq!(
            manager.tracker.lock().get(&retired_run_id).unwrap().status,
            WorkflowRunStatus::Cancelled,
            "the old execution is terminal before its retained child response is released"
        );

        let (new_run_id, new_outcome) = manager
            .launch(resolve_inline(script.into()).unwrap(), spec())
            .await
            .unwrap();
        assert_ne!(new_run_id, retired_run_id);
        let SubagentEvent::Spawn(new_child) =
            subagent_rx.recv().await.expect("new run child spawn")
        else {
            panic!("expected new run child spawn");
        };
        assert_ne!(new_child.request.id, retired_child_id);

        assert!(
            late_response
                .send(SubagentResult {
                    success: true,
                    output: std::sync::Arc::from("stale retired output"),
                    subagent_id: retired_child_id.clone(),
                    child_session_id: retired_child_id,
                    ..Default::default()
                })
                .is_err(),
            "retirement drops the old result receiver; a late child response cannot be reassigned"
        );
        tokio::pin!(new_outcome);
        assert!(
            matches!(
                new_outcome.as_mut().get_mut().try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "a late old response must not complete the new Run"
        );
        assert!(
            subagent_rx.try_recv().is_err(),
            "late completion must not restart the retired workflow operation"
        );

        let new_child_id = new_child.request.id.clone();
        new_child
            .result_tx
            .send(SubagentResult {
                success: true,
                output: std::sync::Arc::from("new run output"),
                subagent_id: new_child_id.clone(),
                child_session_id: new_child_id.clone(),
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(
            new_outcome.await.unwrap(),
            WorkflowOutcome::Completed { result } if result == serde_json::json!("new run output")
        ));

        let new_journal = std::fs::read_to_string(
            dir.path()
                .join("workflows")
                .join(&new_run_id)
                .join("journal.jsonl"),
        )
        .unwrap();
        let entry = serde_json::from_str::<workflow::JournalEntry>(
            new_journal.lines().last().expect("new run journal entry"),
        )
        .unwrap();
        let agent = serde_json::from_value::<workflow::AgentResult>(entry.result).unwrap();
        assert_eq!(agent.agent_id, new_child_id);
        assert_eq!(agent.output, "new run output");
        assert!(
            !new_journal.contains("stale retired output"),
            "a retired child result must not be written to a later Run journal"
        );
        assert!(
            manager.tracker.lock().get(&retired_run_id).is_some(),
            "late response does not remove the retired run record"
        );
        assert_eq!(
            manager.tracker.lock().get(&retired_run_id).unwrap().status,
            WorkflowRunStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn backgrounded_stub_fails_loudly() {
        use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};

        let dir = tempfile::tempdir().unwrap();
        let (mut manager, mut subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\n\
             let r = agent(\"work\");\n\
             complete(r.output);"
                .into(),
        )
        .unwrap();
        let (run_id, outcome_rx) = manager.launch(resolved, spec()).await.unwrap();

        let spawn_req = subagent_rx.recv().await.expect("spawn event");
        let SubagentEvent::Spawn(req) = spawn_req else {
            panic!("expected spawn event");
        };
        let id = req.id.clone();
        let _ = req.result_tx.send(SubagentResult {
            backgrounded: true,
            subagent_id: id,
            ..Default::default()
        });
        let outcome = outcome_rx.await.unwrap();
        match outcome {
            WorkflowOutcome::Failed { error } => {
                assert!(
                    error.contains("auto-backgrounded"),
                    "distinct engine-bug message expected, got: {error}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::Failed
        );
    }
}
