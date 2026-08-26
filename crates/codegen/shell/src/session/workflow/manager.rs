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
use super::tracker::{WorkflowRunState, WorkflowRunStatus, WorkflowRuntimeRoute, WorkflowTracker};

pub(crate) const WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION: usize = 4;
pub(crate) const WORKFLOW_DEFAULT_AGENT_BUDGET: u64 = workflow::DEFAULT_AGENT_BUDGET;

struct ActiveRun {
    cancel: CancellationToken,
    pause_intent: Arc<AtomicBool>,
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
    active: HashMap<String, ActiveRun>,
}

impl WorkflowManager {
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
            active: HashMap::new(),
        }
    }

    /// Update the launch default after a durably committed session model
    /// transition. Runs already tracked are intentionally unaffected.
    pub(crate) fn set_next_run_route(&mut self, route: WorkflowRuntimeRoute) {
        route
            .validate()
            .expect("session model selection must provide a valid Workflow route");
        self.next_run_route = route;
    }

    pub(crate) fn tracker(&self) -> Arc<parking_lot::Mutex<WorkflowTracker>> {
        self.tracker.clone()
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
                    duration_ms: elapsed,
                    message: interrupted.pause_message.clone(),
                },
            ))
            .await
        {
            Ok(_) => self.notify.broadcast(&interrupted, elapsed, 0, true),
            Err(error) => {
                tracing::error!(%run_id, %error, "failed to close rejected Workflow launch in Timeline");
            }
        }
    }

    pub(crate) async fn launch(
        &mut self,
        resolved: ResolvedWorkflow,
        spec: LaunchSpec,
    ) -> Result<(String, oneshot::Receiver<WorkflowOutcome>), LaunchError> {
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
                if !existing.status.is_resumable() {
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
                    self.next_run_route.clone(),
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
        let lifecycle = if resumed {
            chat_state::WorkflowEvent::Resumed {
                run_id: run_id.clone(),
                execution_epoch,
            }
        } else {
            chat_state::WorkflowEvent::Spawned {
                run_id: run_id.clone(),
                execution_epoch,
                name: state.name.clone(),
                objective: state.objective.clone(),
            }
        };
        if let Err(error) = self.store.validate_persistable(&state) {
            if !resumed {
                self.tracker.lock().clear_run(&run_id);
                self.store.remove(&run_id);
            }
            return Err(LaunchError::Store(format!(
                "workflow state cannot be persisted: {error}"
            )));
        }
        if let Err(error) = self
            .timeline
            .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(lifecycle))
            .await
        {
            if !resumed {
                self.tracker.lock().clear_run(&run_id);
                self.store.remove(&run_id);
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
            if let Some(interrupted) = self.tracker.lock().interrupt(
                &run_id,
                format!("workflow journal could not prepare resume: {message}"),
            ) {
                let _ = self.store.persist_ack(&interrupted).await;
            }
            let _ = self
                .timeline
                .record_timeline_event_durably(chat_state::TimelineEventKind::Workflow(
                    chat_state::WorkflowEvent::Ended {
                        run_id: run_id.clone(),
                        execution_epoch,
                        status: chat_state::WorkflowExecutionStatus::Interrupted,
                        duration_ms: 0,
                        message: Some(message.clone()),
                    },
                ))
                .await;
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
                            duration_ms: 0,
                            message: Some(error.to_string()),
                        },
                    ))
                    .await
                {
                    tracing::error!(run_id = %run_id, %timeline_error, "failed to close rejected workflow execution in Timeline");
                }
            }
            return Err(LaunchError::Store(error.to_string()));
        }
        self.notify
            .emit(&state, self.tracker.lock().elapsed_ms(&run_id), 0);

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
            let host_drain =
                tokio::time::timeout(std::time::Duration::from_secs(25), host_drained).await;
            let drain_failed = !matches!(host_drain, Ok(Ok(HostDrainOutcome::Drained)));
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
                } else if pause_intent.load(Ordering::Relaxed)
                    && matches!(
                        outcome,
                        WorkflowOutcome::Cancelled | WorkflowOutcome::Paused { .. }
                    )
                {
                    tracker.pause_user(&watcher_run_id, None)
                } else {
                    tracker.apply_outcome(&watcher_run_id, &outcome)
                }
            };
            if let Some(mut state) = state {
                let mut manifest_persisted = true;
                if let Err(error) = store.persist_ack(&state).await {
                    tracing::warn!(run_id = %watcher_run_id, %error, "workflow terminal manifest was not durably written");
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
                    && let Ok(mut workspace) = super::workspace::WorkflowWorkspace::open_in_session(
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
                            status: state.status.to_timeline(),
                            duration_ms: elapsed,
                            message: state.pause_message.clone(),
                        },
                    ))
                    .await
                {
                    tracing::error!(run_id = %watcher_run_id, %error, manifest_persisted, "workflow terminal Timeline boundary was rejected");
                    let message =
                        format!("workflow Timeline terminal could not be committed: {error}");
                    let _ = done_tx.send(Err(message.clone()));
                    let _ = outcome_tx.send(WorkflowOutcome::Failed { error: message });
                    return;
                }
                notify.broadcast(&state, elapsed, 0, true);
                if state.status.is_completion_reportable() {
                    tokio::spawn(deliver_workflow_completion_acknowledged(
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
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
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
        if state.status != WorkflowRunStatus::UserPaused {
            return Err(format!(
                "workflow {run_id} settled as {} instead of user_paused",
                state.status.as_str()
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
                    duration_ms: elapsed,
                    message: Some("cancelled while no execution was active".into()),
                },
            ))
            .await
        {
            tracing::error!(%run_id, %error, "refusing to cancel inactive Workflow without a durable Timeline close");
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
                }
                let (state, elapsed) = {
                    let tracker = self.tracker.lock();
                    (
                        tracker.get(run_id).expect("run still tracked"),
                        tracker.elapsed_ms(run_id),
                    )
                };
                self.notify.emit(&state, elapsed, 0);
                tokio::spawn(deliver_workflow_completion_acknowledged(
                    self.session_cmd_tx.clone(),
                    state.clone(),
                ));
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
        let mut pending: Vec<(
            Option<String>,
            oneshot::Receiver<Result<WorkflowRunState, String>>,
        )> = active
            .into_iter()
            .map(|(run_id, run)| (Some(run_id), run.done))
            .collect();

        let mut timed_out = Vec::new();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut pending = pending.into_iter();
        while let Some((run_id, done)) = pending.next() {
            if tokio::time::timeout_at(deadline, done).await.is_err() {
                if let Some(run_id) = run_id {
                    timed_out.push(run_id);
                }
                timed_out.extend(pending.filter_map(|(run_id, _)| run_id));
                break;
            }
        }
        if timed_out.is_empty() {
            return Ok(());
        }

        tracing::warn!(
            run_ids = ?timed_out,
            "workflow shutdown drain timed out; marking runs interrupted"
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
                self.notify.broadcast(&state, elapsed, 0, true);
            }
        }
        Err(timed_out)
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

/// Give SessionActor a bounded opportunity to durably admit the model-visible
/// receipt. The Workflow lifecycle barrier is deliberately independent:
/// waiting for the actor here would deadlock a pause/cancel command that is
/// itself awaiting the manager's terminal barrier. A restored session replays
/// the same immutable run/epoch identity when this handoff cannot complete.
async fn deliver_workflow_completion_acknowledged(
    session_cmd_tx: mpsc::UnboundedSender<crate::session::commands::SessionCommand>,
    state: WorkflowRunState,
) {
    const MAX_ATTEMPTS: usize = 3;

    for attempt in 1..=MAX_ATTEMPTS {
        let (respond_to, admitted) = oneshot::channel();
        if session_cmd_tx
            .send(
                crate::session::commands::SessionCommand::WorkflowCompleted {
                    state: state.clone(),
                    respond_to,
                },
            )
            .is_err()
        {
            tracing::warn!(run_id = %state.run_id, "workflow completion delivery stopped because the session mailbox closed");
            return;
        }
        let error = match admitted.await {
            Ok(Ok(())) => return,
            Ok(Err(error)) => error,
            Err(error) => format!("acknowledgement dropped: {error}"),
        };
        if attempt == MAX_ATTEMPTS {
            tracing::error!(
                run_id = %state.run_id,
                %error,
                attempts = MAX_ATTEMPTS,
                "workflow completion receipt admission exhausted; durable run remains recoverable"
            );
            return;
        }
        tracing::warn!(
            run_id = %state.run_id,
            %error,
            attempt,
            max_attempts = MAX_ATTEMPTS,
            "workflow completion receipt admission failed; retrying"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
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
fn test_timeline() -> chat_state::ChatStateHandle {
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
        Box::new(chat_state::NullTimelinePersistence),
        mpsc::unbounded_channel().0,
        CancellationToken::new(),
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
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
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

    #[tokio::test]
    async fn workflow_completion_retries_until_actor_admission_ack() {
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
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let delivery = tokio::spawn(deliver_workflow_completion_acknowledged(
            cmd_tx,
            state.clone(),
        ));

        let crate::session::commands::SessionCommand::WorkflowCompleted {
            state: first,
            respond_to: first_ack,
            ..
        } = cmd_rx.recv().await.expect("first delivery")
        else {
            panic!("expected WorkflowCompleted");
        };
        first_ack.send(Err("disk unavailable".into())).unwrap();

        let crate::session::commands::SessionCommand::WorkflowCompleted {
            state: retry,
            respond_to: retry_ack,
            ..
        } = cmd_rx.recv().await.expect("retry delivery")
        else {
            panic!("expected WorkflowCompleted retry");
        };
        assert_eq!(retry.run_id, first.run_id);
        assert_eq!(retry.execution_epoch, first.execution_epoch);
        retry_ack.send(Ok(())).unwrap();
        delivery.await.unwrap();
    }

    #[tokio::test]
    async fn workflow_completion_exhaustion_releases_the_delivery_task() {
        let mut tracker = WorkflowTracker::default();
        let mut state = tracker.start_run(
            "workflow-failed".into(),
            "failed".into(),
            "verify bounded delivery".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::test_runtime_route(),
        );
        state.status = WorkflowRunStatus::Complete;
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let delivery = tokio::spawn(deliver_workflow_completion_acknowledged(cmd_tx, state));

        for _ in 0..3 {
            let crate::session::commands::SessionCommand::WorkflowCompleted { respond_to, .. } =
                cmd_rx.recv().await.expect("bounded workflow delivery")
            else {
                panic!("expected WorkflowCompleted");
            };
            respond_to.send(Err("disk unavailable".into())).unwrap();
        }

        tokio::time::timeout(std::time::Duration::from_secs(1), delivery)
            .await
            .expect("workflow delivery task must retire after bounded retries")
            .unwrap();
        assert!(cmd_rx.try_recv().is_err());
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
            .filter(|row| row.actor == format!("workflow:{run_id}"))
            .next_back()
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
            agent_client_protocol::SessionId::new("scratch-failure-test"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
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
            WorkflowOutcome::Paused { .. }
        ));
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
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::Cancelled
        );
        let trajectory = manager.timeline.trajectory().await.unwrap();
        let last = trajectory
            .rows
            .iter()
            .filter(|row| row.actor == format!("workflow:{run_id}"))
            .next_back()
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
        for status in [
            crate::session::workflow::tracker::WorkflowRunStatus::Complete,
            crate::session::workflow::tracker::WorkflowRunStatus::Cancelled,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted,
        ] {
            let mut restored = state.clone();
            restored.status = status;
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
        assert_eq!(result.unwrap_err(), vec![run_id.clone()]);
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::Interrupted
        );
        assert!(!state.status.is_paused());
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
        manager.set_next_run_route(second_route.clone());
        let (second_id, second_outcome) = manager.launch(completed(), spec()).await.unwrap();
        let _ = first_outcome.await.unwrap();
        let _ = second_outcome.await.unwrap();

        let tracker = manager.tracker.lock();
        assert_eq!(tracker.get(&first_id).unwrap().runtime_route, first_route);
        assert_eq!(tracker.get(&second_id).unwrap().runtime_route, second_route);
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
