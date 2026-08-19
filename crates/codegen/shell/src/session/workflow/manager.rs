use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use workflow::{Journal, WorkflowOutcome, WorkflowRunParams};

use super::host_service::{
    DiagnosticHook, HostDrainOutcome, WorkflowHostParams, spawn_workflow_host_service,
};
use super::notify::WorkflowNotifySender;
use super::registry::{ResolvedWorkflow, WorkflowSource};
use super::store::WorkflowRunStore;
use super::tracker::WorkflowTracker;

pub(crate) const WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION: usize = 4;
pub(crate) const WORKFLOW_DEFAULT_AGENT_BUDGET: u64 = workflow::DEFAULT_AGENT_BUDGET;

struct ActiveRun {
    cancel: CancellationToken,
    pause_intent: Arc<AtomicBool>,
    done: oneshot::Receiver<()>,
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
    session_dir: Option<PathBuf>,
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
    active: HashMap<String, ActiveRun>,
    retiring: Vec<(String, oneshot::Receiver<()>)>,
}

impl WorkflowManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        session_id: String,
        session_dir: Option<PathBuf>,
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
    ) -> Self {
        Self {
            session_id,
            session_dir,
            cwd,
            tracker,
            store,
            notify,
            subagent_event_tx,
            diagnostics,
            session_cmd_tx,
            timeline,
            templates,
            active: HashMap::new(),
            retiring: Vec::new(),
        }
    }

    pub(crate) fn tracker(&self) -> Arc<parking_lot::Mutex<WorkflowTracker>> {
        self.tracker.clone()
    }

    pub(crate) async fn launch(
        &mut self,
        resolved: ResolvedWorkflow,
        spec: LaunchSpec,
    ) -> Result<(String, oneshot::Receiver<WorkflowOutcome>), LaunchError> {
        self.reap_terminal_runs();
        if self.active.len().saturating_add(self.retiring.len())
            >= WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION
        {
            return Err(LaunchError::TooManyActiveRuns);
        }

        let definition_id = resolved.definition_id.clone();
        let definition_scope = resolved.scope;
        let definition_hash = resolved.content_hash.clone();
        let definition_private = resolved.private;
        let allow_fork_context = resolved.source == WorkflowSource::Builtin;
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
                let journal = match self
                    .session_dir
                    .as_ref()
                    .zip(existing.journal_path.as_ref())
                {
                    Some((session_dir, relative)) => {
                        let expected = format!("workflows/{run_id}/journal.jsonl");
                        if relative != &expected {
                            return Err(LaunchError::Journal(
                                "persisted journal path does not match its workflow run".into(),
                            ));
                        }
                        Journal::load(session_dir.join(relative))
                            .map_err(|e| LaunchError::Journal(e.to_string()))?
                    }
                    None => Journal::new(None),
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
                let journal_path = self.session_dir.as_ref().map(|d| d.join(&journal_rel));
                let journal = Journal::new(journal_path);
                let state = self.tracker.lock().start_run(
                    run_id.clone(),
                    resolved.meta.name,
                    spec.objective.clone(),
                    resolved.meta.phases,
                    Some(agent_budget),
                    self.session_dir.as_ref().map(|_| journal_rel),
                );
                let state = {
                    let mut tracker = self.tracker.lock();
                    tracker.set_definition_provenance(
                        &run_id,
                        definition_id,
                        definition_scope,
                        definition_hash,
                        definition_private,
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
                private: state.private,
            }
        };
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
                let _ = self.store.persist_now(&interrupted);
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

        if let Err(error) = self.store.persist_now(&state) {
            let interrupted = self.tracker.lock().interrupt(
                &run_id,
                "workflow state persistence failed before execution; start a new run",
            );
            if let Some(interrupted) = interrupted {
                if let Err(persist_error) = self.store.persist_now(&interrupted) {
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
        let scratch_dir = self
            .session_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join("workflows")
            .join(&run_id)
            .join("scratch");

        let (host_service, host_drained) = spawn_workflow_host_service(
            WorkflowHostParams {
                run_id: run_id.clone(),
                cwd: self.cwd.clone(),
                scratch_dir,
                tracker: self.tracker.clone(),
                store: self.store.clone(),
                notify: self.notify.clone(),
                subagent_event_tx: self.subagent_event_tx.clone(),
                parent_session_id: self.session_id.clone(),
                allow_fork_context,
                templates: self.templates.clone(),
                diagnostics: self.diagnostics.clone(),
                cancel: cancel.clone(),
                max_concurrency: state.max_concurrency,
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
        let completion_session_dir = self.session_dir.clone();
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
                    && !state.private
                    && let (Some(session_dir), Some(definition_id), Some(definition_hash)) = (
                        completion_session_dir.as_deref(),
                        state.definition_id.as_ref(),
                        state.definition_hash.as_deref(),
                    )
                    && let Ok(mut workspace) =
                        super::workspace::WorkflowWorkspace::open(session_dir, &completion_cwd)
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
                    let _ = done_tx.send(());
                    let _ = outcome_tx.send(WorkflowOutcome::Failed {
                        error: format!(
                            "workflow Timeline terminal could not be committed: {error}"
                        ),
                    });
                    return;
                }
                notify.broadcast(&state, elapsed, 0, true);
                if state.status.is_completion_reportable() {
                    let _ = session_cmd_tx.send(
                        crate::session::commands::SessionCommand::WorkflowCompletionTurn {
                            run_id: watcher_run_id.clone(),
                            revision: state.revision,
                            outcome: outcome.clone(),
                        },
                    );
                }
            }
            let _ = done_tx.send(());
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
        let store = WorkflowRunStore::new(session_dir.clone(), persist_tx.clone());
        let notify = super::notify::WorkflowNotifySender::new(
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            store.clone(),
        );
        let manager = Arc::new(tokio::sync::Mutex::new(WorkflowManager::new(
            "test-session".into(),
            session_dir,
            std::env::temp_dir(),
            tracker.clone(),
            store,
            notify,
            mpsc::unbounded_channel().0,
            Arc::new(|_, _, _| {}),
            mpsc::unbounded_channel().0,
            test_timeline(),
            std::collections::HashMap::new(),
        )));
        (manager, tracker)
    }

    fn reap_terminal_runs(&mut self) {
        let terminal: Vec<String> = self
            .active
            .keys()
            .filter(|run_id| {
                self.tracker
                    .lock()
                    .get(run_id)
                    .is_some_and(|state| state.status.is_terminal())
            })
            .cloned()
            .collect();
        for run_id in terminal {
            if let Some(run) = self.active.remove(&run_id) {
                self.retiring.push((run_id.to_owned(), run.done));
            }
        }

        self.retiring.retain_mut(|(_, done)| match done.try_recv() {
            Ok(()) | Err(oneshot::error::TryRecvError::Closed) => false,
            Err(oneshot::error::TryRecvError::Empty) => true,
        });
    }

    fn reap_if_terminal(&mut self, run_id: &str) -> bool {
        let terminal = self
            .tracker
            .lock()
            .get(run_id)
            .is_some_and(|s| s.status.is_terminal());
        if terminal && let Some(run) = self.active.remove(run_id) {
            self.retiring.push((run_id.to_owned(), run.done));
        }
        terminal
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

    pub(crate) fn pause(&mut self, run_id: &str) -> bool {
        if self.reap_if_terminal(run_id) {
            return false;
        }
        let Some(run) = self.active.remove(run_id) else {
            return false;
        };
        run.pause_intent.store(true, Ordering::Relaxed);
        run.cancel.cancel();
        let _ = self.cancel_children_for_run(run_id);
        self.retiring.push((run_id.to_owned(), run.done));
        let paused = {
            let mut tracker = self.tracker.lock();
            let active = tracker
                .get(run_id)
                .is_some_and(|state| !state.status.is_terminal());
            if active {
                let state = tracker.pause_user(run_id, None);
                state.map(|state| (state, tracker.elapsed_ms(run_id)))
            } else {
                None
            }
        };
        if let Some((state, elapsed)) = paused {
            self.notify.emit(&state, elapsed, 0);
        }
        true
    }

    pub(crate) async fn cancel(&mut self, run_id: &str) -> bool {
        self.reap_terminal_runs();
        if let Some(run) = self.active.remove(run_id) {
            run.cancel.cancel();
            let _ = self.cancel_children_for_run(run_id);
            self.retiring.push((run_id.to_owned(), run.done));
            let state = {
                let mut tracker = self.tracker.lock();
                match tracker.get(run_id) {
                    Some(state) if !state.status.is_terminal() => {
                        tracker.apply_outcome(run_id, &WorkflowOutcome::Cancelled)
                    }
                    _ => None,
                }
            };
            if state.is_some() {
                let (state, elapsed) = {
                    let tracker = self.tracker.lock();
                    (
                        tracker.get(run_id).expect("run still tracked"),
                        tracker.elapsed_ms(run_id),
                    )
                };
                self.notify.emit(&state, elapsed, 0);
            }
            return true;
        }
        let _ = self.cancel_children_for_run(run_id);
        let (execution_epoch, elapsed) = {
            let tracker = self.tracker.lock();
            let Some(state) = tracker.get(run_id) else {
                return false;
            };
            if !state.status.is_resumable() {
                return false;
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
            return false;
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
                true
            }
            None => false,
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
        let mut pending: Vec<(Option<String>, oneshot::Receiver<()>)> = active
            .into_iter()
            .map(|(run_id, run)| (Some(run_id), run.done))
            .collect();
        pending.extend(
            self.retiring
                .drain(..)
                .map(|(run_id, done)| (Some(run_id), done)),
        );

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
                if let Err(error) = self.store.persist_now(&state) {
                    tracing::error!(%run_id, %error, "failed to persist workflow shutdown interruption");
                }
                let elapsed = self.tracker.lock().elapsed_ms(run_id);
                self.notify.broadcast(&state, elapsed, 0, true);
            }
        }
        Err(timed_out)
    }

    #[cfg(test)]
    pub(crate) fn test_insert_active_run(&mut self, run_id: String, done: oneshot::Receiver<()>) {
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

    pub(crate) fn script_copy_path(&self, run_id: &str) -> Option<std::path::PathBuf> {
        self.store.script_copy_path(run_id)
    }

    pub(crate) fn args_copy_for(&self, run_id: &str) -> serde_json::Value {
        self.store
            .args_for(run_id)
            .unwrap_or(serde_json::Value::Null)
    }
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
            while let Some(message) = persist_rx.recv().await {
                if let PersistenceMsg::WorkflowRunStateAndAck { respond_to, .. } = message {
                    let result = if reject_manifest_ack {
                        Err(std::io::Error::other("manifest disk failure"))
                    } else {
                        Ok(())
                    };
                    let _ = respond_to.send(result);
                }
            }
        });
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let store = WorkflowRunStore::new(session_dir.clone(), persist_tx.clone());
        let notify = WorkflowNotifySender::new(
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            store.clone(),
        );
        let tracker = Arc::new(parking_lot::Mutex::new(WorkflowTracker::default()));
        let manager = WorkflowManager::new(
            "test-session".into(),
            session_dir,
            std::env::temp_dir(),
            tracker,
            store,
            notify,
            subagent_tx,
            Arc::new(|_, _, _| {}),
            mpsc::unbounded_channel().0,
            test_timeline(),
            HashMap::new(),
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
            manager.script_copy_path(&run_id).unwrap(),
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
    async fn pause_eagerly_marks_user_paused() {
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
        );
        let (_done_tx, done_rx) = oneshot::channel();
        manager.test_insert_active_run(run_id.clone(), done_rx);

        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::Active,
        );
        assert!(manager.pause(&run_id));
        let state = manager.tracker.lock().get(&run_id).unwrap();
        assert_eq!(
            state.status,
            crate::session::workflow::tracker::WorkflowRunStatus::UserPaused,
            "pause() must eagerly mark UserPaused so status is not still Active"
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
        assert!(manager.pause(&run_id));
        assert_eq!(
            manager.tracker.lock().get(&run_id).unwrap().status,
            crate::session::workflow::tracker::WorkflowRunStatus::UserPaused,
            "pause() must mark UserPaused immediately"
        );
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

        assert!(manager.pause(&run_id));
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

        assert!(manager.cancel(&run_id).await);
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
            tools::implementations::grow_build::task::types::ModelOverrideProvenance::Tool,
            "script model overrides are untrusted tool provenance"
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
    async fn retiring_runs_still_consume_session_admission() {
        let dir = tempfile::tempdir().unwrap();
        let (mut manager, _subagent_rx) = test_manager(Some(dir.path().to_path_buf()));
        let mut done_senders = Vec::new();
        for index in 0..WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION {
            let (done_tx, done_rx) = oneshot::channel();
            manager
                .retiring
                .push((format!("retiring-{index}"), done_rx));
            done_senders.push(done_tx);
            assert_eq!(manager.retiring.len(), index + 1);
        }

        let resolved = resolve_inline(
            "let meta = #{ name: \"t\", description: \"d\" };\ncomplete(\"done\");".into(),
        )
        .unwrap();
        assert!(matches!(
            manager.launch(resolved, spec()).await.unwrap_err(),
            LaunchError::TooManyActiveRuns
        ));

        done_senders.pop().unwrap().send(()).unwrap();
        manager.reap_terminal_runs();
        assert_eq!(
            manager.retiring.len(),
            WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION - 1
        );
        assert!(
            manager.active.len().saturating_add(manager.retiring.len())
                < WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION
        );
        drop(done_senders);
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
        assert!(manager.cancel(&run_id).await);
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
