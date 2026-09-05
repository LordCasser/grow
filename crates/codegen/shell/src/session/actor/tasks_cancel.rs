//! Prompt-task plumbing for `SessionActor` (`AgentTask`, `TaskSlot`,
//! `run_task`, turn guards) and the cancel paths.

use super::*;
use futures_util::FutureExt as _;

tokio::task_local! {
    /// Immutable provider-admission generation captured when a foreground
    /// owner is installed. Stop advances the session generation, so any old
    /// turn that survives until its next await can only present the retired
    /// value and is rejected at the provider boundary.
    static TURN_USAGE_EPOCH: u64;
    /// Admission handshake owned by the task publisher. Stop holds the same
    /// step-control gate until this sender reports that TurnStarted is durable,
    /// so a visible foreground owner can never be cancelled as a phantom turn.
    static TURN_DURABLE_START_ACK: std::cell::RefCell<Option<oneshot::Sender<bool>>>;
}

pub(super) fn turn_usage_epoch_or(current: u64) -> u64 {
    TURN_USAGE_EPOCH.try_with(|epoch| *epoch).unwrap_or(current)
}

pub(super) fn signal_durable_turn_start(started: bool) {
    let _ = TURN_DURABLE_START_ACK.try_with(|slot| {
        if let Some(ack) = slot.borrow_mut().take() {
            let _ = ack.send(started);
        }
    });
}

pub(super) struct TurnSubagentScopeGuard {
    current_prompt_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    prompt_id: String,
}

impl TurnSubagentScopeGuard {
    pub(super) fn new(
        current_prompt_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
        prompt_id: String,
    ) -> Self {
        Self {
            current_prompt_id,
            prompt_id,
        }
    }
}

impl Drop for TurnSubagentScopeGuard {
    fn drop(&mut self) {
        let mut current_prompt_id = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned");
        if current_prompt_id.as_deref() == Some(self.prompt_id.as_str()) {
            *current_prompt_id = None;
        }
    }
}

/// RAII guard that stores `false` into an `is_turn_active` flag on drop.
/// Guarantees the flag is cleared on all exit paths (early returns, errors, panics).
pub(super) struct TurnActiveGuard(Option<Arc<std::sync::atomic::AtomicBool>>);

impl TurnActiveGuard {
    pub(super) fn activate(flag: Option<&Arc<std::sync::atomic::AtomicBool>>) -> Self {
        if let Some(f) = flag {
            f.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Self(flag.cloned())
    }
}

impl Drop for TurnActiveGuard {
    fn drop(&mut self) {
        if let Some(flag) = &self.0 {
            flag.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

pub(crate) struct AgentTask {
    pub(crate) prompt_id: String,
    pub(crate) origin: PromptOrigin,
    pub(crate) turn_kind: crate::session::TurnKind,
    pub(crate) turn_start_ms: u64,
    pub(crate) usage_epoch: u64,
    pub(crate) handle: tokio::task::AbortHandle,
    /// Closed atomically with the final same-turn steering drain.
    pub(crate) steering_open: bool,
}

impl AgentTask {
    pub(super) fn new_prompt(
        session: Arc<SessionActor>,
        prompt_id: String,
        input_ids: Vec<String>,
        origin: PromptOrigin,
        host_command: Option<crate::session::HostCommandInvocation>,
        notification_ids: Vec<String>,
        turn_kind: crate::session::TurnKind,
        input: Vec<ContentBlock>,
        admitted_behavior: tool_types::BehaviorId,
        client_identifier: Option<String>,
        screen_mode: Option<String>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        start_gate: Option<oneshot::Receiver<()>>,
        durable_start_ack: Option<oneshot::Sender<bool>>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
        persist_ack: Option<oneshot::Sender<()>>,
    ) -> Self {
        let pid = prompt_id.clone();
        let usage_epoch = session
            .goal_usage_window
            .owner_epoch(&session.session_id_string());
        Self {
            prompt_id,
            origin: origin.clone(),
            turn_kind,
            turn_start_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            usage_epoch,
            handle: tokio::task::spawn_local(async move {
                // Admission can install foreground ownership before async
                // tool-resource publication. This one-shot keeps the task
                // dormant until every turn-scoped capability is ready.
                if let Some(start_gate) = start_gate
                    && start_gate.await.is_err()
                {
                    return;
                }
                let task = TURN_DURABLE_START_ACK.scope(
                    std::cell::RefCell::new(durable_start_ack),
                    TURN_USAGE_EPOCH.scope(
                        usage_epoch,
                        run_task(
                            session.clone(),
                            input,
                            admitted_behavior,
                            client_identifier,
                            screen_mode,
                            verbatim,
                            json_schema,
                            pid,
                            input_ids,
                            origin,
                            notification_ids,
                            turn_kind,
                            completion_tx,
                            persist_ack,
                        ),
                    ),
                );
                if let Some(invocation) = host_command {
                    HOST_COMMAND_INVOCATION.scope(invocation, task).await
                } else {
                    task.await
                }
            })
            .abort_handle(),
            steering_open: true,
        }
    }

    fn abort(&self) {
        if !self.handle.is_finished() {
            self.handle.abort();
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

/// Holds at most one spawned task; arming a new one aborts the previous.
/// `Cell` interior mutability because `SessionActor` is `!Send` (single-threaded
/// LocalSet). Backs both the deferred user-message prefix (`take`s the handle to
/// await its result) and the idle-notification debounce (`cancel`s it).
pub(crate) struct TaskSlot<T> {
    handle: std::cell::Cell<Option<tokio::task::JoinHandle<T>>>,
}

impl<T> TaskSlot<T> {
    pub(crate) fn new() -> Self {
        Self {
            handle: std::cell::Cell::new(None),
        }
    }

    /// Abort any pending task and store the new one.
    pub(crate) fn arm(&self, handle: tokio::task::JoinHandle<T>) {
        if let Some(old) = self.handle.take() {
            old.abort();
        }
        self.handle.set(Some(handle));
    }

    /// Take the pending task handle (e.g. to await its result).
    pub(crate) fn take(&self) -> Option<tokio::task::JoinHandle<T>> {
        self.handle.take()
    }

    /// Abort and drop any pending task (e.g. because a new turn started).
    pub(crate) fn cancel(&self) {
        if let Some(old) = self.handle.take() {
            old.abort();
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.handle.take().is_some_and(|handle| {
            let running = !handle.is_finished();
            self.handle.set(Some(handle));
            running
        })
    }

    /// Abort the current owner and observe its terminal before returning.
    /// Dropping a JoinHandle only detaches it, which is never a shutdown
    /// barrier for a task that can still publish persistence or usage facts.
    pub(crate) async fn abort_and_join(&self) -> Result<(), tokio::task::JoinError> {
        let Some(handle) = self.take() else {
            return Ok(());
        };
        handle.abort();
        match handle.await {
            Ok(_) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Counts finite session-owned activities that run outside the foreground
/// turn/control owner. Admission closes exactly once at teardown; a permit is
/// acquired synchronously before `spawn_local`, eliminating spawn-before-flag
/// races. The final owner drop wakes both explicit drain waiters and the main
/// loop's graceful-shutdown readiness check.
#[derive(Clone)]
pub(crate) struct SessionActivityTracker {
    inner: Arc<SessionActivityInner>,
}

struct SessionActivityInner {
    accepting: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicUsize,
    changed: tokio::sync::Notify,
}

pub(crate) struct SessionActivityPermit {
    inner: Arc<SessionActivityInner>,
    label: &'static str,
}

impl SessionActivityTracker {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(SessionActivityInner {
                accepting: std::sync::atomic::AtomicBool::new(true),
                active: std::sync::atomic::AtomicUsize::new(0),
                changed: tokio::sync::Notify::new(),
            }),
        }
    }

    pub(crate) fn try_start(&self, label: &'static str) -> Option<SessionActivityPermit> {
        use std::sync::atomic::Ordering;
        if !self.inner.accepting.load(Ordering::Acquire) {
            return None;
        }
        self.inner.active.fetch_add(1, Ordering::AcqRel);
        if !self.inner.accepting.load(Ordering::Acquire) {
            if self.inner.active.fetch_sub(1, Ordering::AcqRel) == 1 {
                self.inner.changed.notify_waiters();
            }
            return None;
        }
        Some(SessionActivityPermit {
            inner: Arc::clone(&self.inner),
            label,
        })
    }

    /// Register nested work already owned by a foreground or detached activity.
    /// This deliberately remains available after top-level admission closes:
    /// an admitted owner and the inline SessionEnd finalizer must be able to
    /// finish their Sideband ledger before the final persistence barrier.
    pub(crate) fn start_nested(&self, label: &'static str) -> SessionActivityPermit {
        self.inner
            .active
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        SessionActivityPermit {
            inner: Arc::clone(&self.inner),
            label,
        }
    }

    pub(crate) fn close_admission(&self) {
        self.inner
            .accepting
            .store(false, std::sync::atomic::Ordering::Release);
        self.inner.changed.notify_waiters();
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.inner.active.load(std::sync::atomic::Ordering::Acquire) == 0
    }

    pub(crate) async fn changed(&self) {
        self.inner.changed.notified().await;
    }

    pub(crate) async fn wait_idle(&self) {
        loop {
            let changed = self.inner.changed.notified();
            if self.is_idle() {
                return;
            }
            changed.await;
        }
    }
}

impl Drop for SessionActivityPermit {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        let previous = self.inner.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "session activity permit underflow");
        if previous == 1 {
            self.inner.changed.notify_waiters();
        }
        tracing::trace!(activity = self.label, "session activity owner released");
    }
}

async fn run_task(
    session: Arc<SessionActor>,
    input: Vec<ContentBlock>,
    admitted_behavior: tool_types::BehaviorId,
    client_identifier: Option<String>,
    screen_mode: Option<String>,
    verbatim: bool,
    json_schema: Option<serde_json::Value>,
    prompt_id: String,
    input_ids: Vec<String>,
    origin: PromptOrigin,
    notification_ids: Vec<String>,
    turn_kind: crate::session::TurnKind,
    completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    persist_ack: Option<oneshot::Sender<()>>,
) {
    let result = std::panic::AssertUnwindSafe(session.handle_prompt(
        &prompt_id,
        input_ids,
        origin,
        notification_ids,
        turn_kind,
        input,
        admitted_behavior,
        client_identifier,
        screen_mode,
        verbatim,
        json_schema,
        persist_ack,
    ))
    .catch_unwind()
    .await;
    let result = match result {
        Ok(result) => result,
        Err(payload) => {
            let panic_message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            tracing::error!(
                prompt_id,
                panic = panic_message,
                "foreground turn owner panicked"
            );
            let panic_message = panic_message.chars().take(1_024).collect::<String>();
            let recovery = std::panic::AssertUnwindSafe(
                session.recover_panicked_turn(&prompt_id, &panic_message),
            )
            .catch_unwind()
            .await;
            match recovery {
                Ok(Ok(())) => Err(acp::Error::internal_error()
                    .data(format!("foreground turn task panicked: {panic_message}"))),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(crate::session::commands::fatal_turn_boundary_error(
                    "panic recovery",
                    format!("turn {prompt_id} panicked again while closing its causal scopes"),
                )),
            }
        }
    };
    let _ = completion_tx.send((prompt_id, result));
}

impl SessionActor {
    /// Convert an unexpected foreground-owner panic into the same durable
    /// child/Step/Turn terminal chain used by ordinary errors. Explicit Stop
    /// aborts the whole wrapper and remains owned by `cancel_running_task`;
    /// this recovery is only reached when `handle_prompt` unwinds by itself.
    pub(in crate::session::actor) async fn recover_panicked_turn(
        &self,
        prompt_id: &str,
        panic_message: &str,
    ) -> Result<(), acp::Error> {
        let _step_control_guard = self.step_control_gate.lock().await;
        let (owns_terminalization, was_settling) = {
            let mut state = self.state.lock().await;
            if state.foreground.settling_identity(prompt_id).is_some() {
                (true, true)
            } else {
                (state.foreground.begin_terminalization(prompt_id), false)
            }
        };
        if !owns_terminalization {
            return Err(crate::session::commands::fatal_turn_boundary_error(
                "panic recovery ownership",
                format!("turn {prompt_id} lost foreground ownership after its owner panicked"),
            ));
        }

        // Never project a second completion after the canonical terminal was
        // already committed. The original `PromptTurnResult` was lost to the
        // unwind, so ending the session is safer than inventing a conflicting
        // UI outcome for a turn whose durable outcome is already authoritative.
        if self.events.current_turn().is_none() {
            return if was_settling {
                Err(crate::session::commands::fatal_turn_boundary_error(
                    "post-terminal panic",
                    format!("turn {prompt_id} panicked after its durable terminal was committed"),
                ))
            } else {
                Err(crate::session::commands::fatal_turn_boundary_error(
                    "panic recovery start",
                    format!("turn {prompt_id} panicked before durable turn admission"),
                ))
            };
        }

        self.goal_usage_window
            .advance_owner_epoch(&self.session_id_string());
        self.settle_goal_usage_for_owner_failure()
            .await
            .map_err(|error| {
                crate::session::commands::fatal_turn_boundary_error("panic usage settlement", error)
            })?;
        self.compaction.cancel.request_cancel();
        self.cancel_background_compaction("foreground_owner_panicked")
            .await?;
        self.chat_state_handle
            .settle_open_compaction_durably("foreground_owner_panicked")
            .await
            .map_err(|error| {
                crate::session::commands::fatal_turn_boundary_error(
                    "panic compaction terminal",
                    error.to_string(),
                )
            })?;
        self.cancel_running_turn_subagents(prompt_id);
        if self.startup_hints.is_subagent {
            self.agent
                .borrow()
                .tool_bridge()
                .kill_foreground_commands_by_owner(&self.session_info.id.0)
                .await;
        } else {
            self.agent
                .borrow()
                .tool_bridge()
                .kill_foreground_commands()
                .await;
        }
        self.emit_turn_ended(
            crate::session::events::TurnOutcomeLabel::Error,
            chat_state::TurnTerminal {
                stop_reason: "error".into(),
                completion_kind: "foreground_owner_panicked".into(),
            },
            None,
            Some(serde_json::json!({
                "reason": "foreground_owner_panicked",
                "panic": panic_message,
            })),
        )
        .await
        .map_err(|error| {
            crate::session::commands::fatal_turn_boundary_error("panic terminal", error.to_string())
        })?;
        if let Some(extension) = &self.idle_prompt_extension {
            extension.on_turn_failed();
        }
        self.flush_to_disk().await;
        let current_prompt_index = self.current_turn_number.get() as usize;
        self.file_state_tracker
            .end_prompt(&self.tool_context.fs, current_prompt_index)
            .await;
        if let Some(rewind_point) = self
            .file_state_tracker
            .get_rewind_point(current_prompt_index)
            .await
        {
            let _ = self
                .notifications
                .persistence_tx
                .send(PersistenceMsg::RewindPoint(rewind_point));
        }
        Ok(())
    }

    /// Arm a turn-terminal fence after a lifecycle or Behavior transition has
    /// become durable while a producer still owns the foreground. Callers hold
    /// `step_control_gate`, so StepStarted observes either the old state or the
    /// armed preemption, never the new authority without its terminal fence.
    pub(super) async fn arm_terminal_preemption_if_running(&self) {
        let mut state = self.state.lock().await;
        if state.foreground.regular().is_some() {
            state.terminal_preemption_pending = true;
        }
    }

    /// Turn-scoped: soft cancel / max-turns only (not user Stop).
    /// `parent_prompt_id` is the authoritative turn id from the turn runner.
    pub(super) fn cancel_running_turn_subagents(&self, parent_prompt_id: &str) {
        self.cancel_subagents_for_prompt_id(parent_prompt_id);
    }

    /// User Stop with cancel_subagents: all non-workflow session children.
    /// Uses the session-bound backend API so cancel never wildcards other sessions.
    pub(super) fn cancel_all_session_subagents(&self) {
        if let Some(event_tx) = self.tool_context.subagent_event_tx.clone() {
            use tools::implementations::grow_build::task::backend::ChannelBackend;
            let backend = ChannelBackend::for_session(event_tx, self.session_id_string());
            let _ = backend.request_cancel_parent_session(tokio::sync::oneshot::channel().0);
        }
    }

    /// Re-open Task spawns for this session after a prior user Stop.
    pub(super) fn open_subagent_spawn_admission(&self) {
        if let Some(event_tx) = self.tool_context.subagent_event_tx.clone() {
            use tools::implementations::grow_build::task::backend::ChannelBackend;
            let backend = ChannelBackend::for_session(event_tx, self.session_id_string());
            let _ = backend.open_spawn_admission();
        }
    }

    fn cancel_subagents_for_prompt_id(&self, parent_prompt_id: &str) {
        if let Some(event_tx) = self.tool_context.subagent_event_tx.clone() {
            use tools::implementations::grow_build::task::types::{
                SubagentCancelRequest, SubagentCancelTarget, SubagentEvent,
            };
            let _ = event_tx.send(SubagentEvent::Cancel(SubagentCancelRequest {
                parent_session_id: Some(self.session_id_string()),
                target: SubagentCancelTarget::ParentPromptId(parent_prompt_id.to_string()),
                respond_to: tokio::sync::oneshot::channel().0,
            }));
        }
    }

    /// End only the foreground turn after a Goal control successfully pauses,
    /// clears, revises, creates, or enters Goal state. Children owned by that
    /// exact prompt are cancelled so stale Goal work cannot keep editing after
    /// clear/edit; unrelated session children and background tasks survive.
    pub(super) async fn cancel_turn_for_goal_control(
        &self,
        control: &super::slash_exec::GoalControlCancellation,
        replay_buffer: &mut crate::agent::update_chunk_merge::ReplayBuffer,
    ) -> Result<(), acp::Error> {
        let parent_prompt_id = {
            let state = self.state.lock().await;
            let Some(turn) = state.foreground.regular() else {
                return Ok(());
            };
            turn.prompt_id.clone()
        };
        self.cancel_subagents_for_prompt_id(&parent_prompt_id);
        if let Some(notification) = replay_buffer.flush() {
            self.emit_buffered(notification).await;
        }
        self.cancel_running_task(false, false, false, Some(control.trigger.to_string()))
            .await?;
        if let Some((goal_id, definition_revision)) = control.retired_goal_owner.as_ref() {
            // The first Goal-owner sweep happens while the foreground future is
            // still alive. Repeat the terminal half after abort: a background
            // run whose request crossed the TerminalActor mailbox but whose
            // handle notification was cancelled is now observable by its
            // immutable `goal_id` snapshot.
            self.sweep_goal_owned_terminal_work(goal_id, *definition_revision)
                .await;
        }
        Ok(())
    }

    /// Auto-pause an active Goal ONLY on an explicit user "Pause goal"
    /// intent (`pause_goal: true` from the Goal interrupt panel). Every other
    /// cancel — plain Esc/Ctrl+C outside Goal, StopTurnOnly /
    /// StopTurnAndSubagents, subagent teardown, lifecycle shutdown — leaves an
    /// active Goal untouched. The command loop wakes the idle arbiter after
    /// foreground/FIFO settlement, so autonomous execution can resume without
    /// waiting for another user input.
    pub(super) async fn maybe_auto_pause_goal_on_cancel(&self, pause_goal: bool) {
        if pause_goal {
            self.auto_pause_goal_if_active(crate::session::goal_tracker::GoalPauseReason::User)
                .await;
        }
    }

    pub(super) async fn cancel_running_task(
        &self,
        cancel_subagents: bool,
        kill_background_tasks: bool,
        rewind_if_pristine: bool,
        trigger: Option<String>,
    ) -> Result<(), acp::Error> {
        let suppress_task_wakes = trigger.as_deref() == Some("ctrl_c");
        // Linearize foreground classification with Step/control transitions.
        // The guard remains held through a real turn's terminal transaction;
        // non-turn owners return without touching the prompt FIFO.
        let _step_control_guard = self.step_control_gate.lock().await;
        #[derive(Clone, Copy)]
        enum NonTurnForeground {
            Idle,
            ApplyingControl,
            Settling,
            Compaction,
        }
        let non_turn = {
            let state = self.state.lock().await;
            match &state.foreground {
                ForegroundState::RegularTurn(_) => None,
                ForegroundState::Idle => Some(NonTurnForeground::Idle),
                ForegroundState::ApplyingControl => Some(NonTurnForeground::ApplyingControl),
                ForegroundState::Settling { .. } => Some(NonTurnForeground::Settling),
                ForegroundState::Compaction => Some(NonTurnForeground::Compaction),
            }
        };
        if let Some(non_turn) = non_turn {
            self.cancel_background_compaction("cancelled_by_stop")
                .await?;
            if matches!(non_turn, NonTurnForeground::Compaction) {
                self.compaction.cancel.request_cancel();
            }
            if cancel_subagents {
                self.cancel_all_session_subagents();
            }
            if kill_background_tasks {
                if self.startup_hints.is_subagent {
                    self.agent
                        .borrow()
                        .tool_bridge()
                        .kill_all_background_tasks_by_owner(&self.session_info.id.0)
                        .await;
                } else {
                    self.agent
                        .borrow()
                        .tool_bridge()
                        .kill_all_background_tasks()
                        .await;
                }
            }
            tracing::debug!(
                foreground = match non_turn {
                    NonTurnForeground::Idle => "idle",
                    NonTurnForeground::ApplyingControl => "applying_control",
                    NonTurnForeground::Settling => "settling",
                    NonTurnForeground::Compaction => "compaction",
                },
                "Stop preserved the prompt FIFO because no regular turn owned it"
            );
            return Ok(());
        }
        // Close the current provider-admission epoch before cancellation is
        // delivered. A sampler/Sideband that has not crossed its network edge
        // yet is rejected even if its task observes Stop later; the next turn
        // only waits for attempts that were already admitted in the old epoch.
        self.goal_usage_window
            .advance_owner_epoch(&self.session_id_string());
        // Abort in-flight `/compact` or auto-compact generation (stream select +
        // pre-replace guard). Safe when no compact is running.
        self.compaction.cancel.request_cancel();
        self.cancel_background_compaction("cancelled_by_stop")
            .await?;
        // Linearize Stop against the causal Step boundary before aborting the
        // producer. A control transaction owns this gate from its durable
        // append through live-state swap and authoritative terminal receipt.
        // The append-only UI projection is repairable after the gate opens. If
        // preparation has not claimed the gate yet, abort leaves the desired
        // control queued for the idle drain; if it has, Stop waits until the
        // control can no longer be mistaken for in-flight.
        {
            let state = self.state.lock().await;
            if let Some(task) = state.foreground.regular() {
                task.abort();
            }
        }
        let already_settling = {
            let mut state = self.state.lock().await;
            let settling = matches!(&state.foreground, ForegroundState::Settling { .. });
            if settling {
                state.terminal_preemption_pending = false;
            }
            settling
        };
        if already_settling {
            if trigger.as_deref() == Some("shutdown") {
                self.events.wait_for_causal_idle().await;
            }
            tracing::debug!(
                "Stop arrived after terminalization was admitted; preserving the existing terminal"
            );
            return Ok(());
        }
        if suppress_task_wakes {
            let mut state = self.state.try_lock().expect("session state is actor-owned");
            state.notifications_suppressed = true;
            ::diagnostics::unified_log::info(
                "shell.task_wake.cancel_barrier",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({
                    "ctrl_c": true,
                    "state": state.notifications_suppressed,
                })),
            );
            drop(state);
            if let Some(is_turn_active) = &self.tool_context.is_turn_active {
                is_turn_active.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }

        // Unified-log processing marker (counterpart of `shell.cancel.received`
        // in `MvpAgent::cancel`): records which prompt the cancel lands on so
        // a stuck "Cancelling…" can be attributed to delivery vs. processing.
        // A pin snapshot only — the authoritative cancel identity is captured
        // from `running_task.prompt_id` under the state lock below, because
        // `current_prompt_id` is cleared early (turn scope guard drop /
        // `handle_completion`) while the finished front and its task slot are
        // still queued; keying the durable `TurnCompleted` on the pin alone
        // loses the terminal (and its `cancelTrigger`) in that window.
        let pinned_prompt_id = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned")
            .clone();
        {
            ::diagnostics::unified_log::info(
                "shell.cancel.processing",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({
                    "prompt_id": &pinned_prompt_id,
                    "cancel_subagents": cancel_subagents,
                    "kill_background_tasks": kill_background_tasks,
                    "rewind_if_pristine": rewind_if_pristine,
                    "trigger": trigger,
                })),
            );
        }

        // Compaction::Started is a causal child of the active Step. Aborting
        // the foreground future can otherwise strand it open and make the
        // following StepEnded/TurnEnded invalid. The Timeline owner decides
        // atomically whether a committed Summary+Replacement is Completed or
        // whether the interrupted transaction is Failed.
        self.chat_state_handle
            .settle_open_compaction_durably("cancelled_by_stop")
            .await
            .map_err(|error| {
                crate::session::commands::fatal_turn_boundary_error(
                    "compaction cancellation terminal",
                    error.to_string(),
                )
            })?;

        if cancel_subagents {
            // Then cancel every non-workflow session child (incl. prior turns)
            // and close spawn admission until the next turn opens it.
            self.cancel_all_session_subagents();
        }

        if !rewind_if_pristine {
            self.signals_handle().record_cancellation();
        }

        // Kill all running foreground terminal processes after aborting the
        // producer, so no new command can enter behind this sweep.
        // Each TerminalBackend implementation knows how to kill its own processes.
        // Background tasks are left alive for interactive sessions but killed
        // during subagent teardown (kill_background_tasks = true).
        //
        if self.startup_hints.is_subagent {
            // Subagent: only kill foreground processes owned by this session,
            // not the parent's or sibling's on the shared backend.
            self.agent
                .borrow()
                .tool_bridge()
                .kill_foreground_commands_by_owner(&self.session_info.id.0)
                .await;
        } else {
            self.agent
                .borrow()
                .tool_bridge()
                .kill_foreground_commands()
                .await;
        }

        if kill_background_tasks {
            if self.startup_hints.is_subagent {
                // Subagent teardown: only kill tasks owned by this session,
                // not the parent's or sibling's tasks on the shared backend.
                self.agent
                    .borrow()
                    .tool_bridge()
                    .kill_all_background_tasks_by_owner(&self.session_info.id.0)
                    .await;
            } else {
                self.agent
                    .borrow()
                    .tool_bridge()
                    .kill_all_background_tasks()
                    .await;
            }
        }

        let total_tokens = self.chat_state_handle.get_projected_tokens().await;
        let (running_task, pending_inputs, rewound_input, had_queued_user_prompt) = {
            let mut state = self.state.lock().await;
            state.terminal_preemption_pending = false;
            debug_assert!(
                pinned_prompt_id.is_none()
                    || state.running_prompt_id().is_none()
                    || state.running_prompt_id() == pinned_prompt_id.as_deref(),
                "current_prompt_id pin disagrees with running_task identity"
            );

            let rewound_input = if rewind_if_pristine && state.rewindable {
                if let Some(task) = state.foreground.take_regular() {
                    task.abort();
                }
                state.notifications_suppressed = false;
                ::diagnostics::unified_log::info(
                    "shell.task_wake.gate_cleared",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({ "reason": "rewind" })),
                );
                state.rewindable = false;
                state.pending_inputs.pop_front()
            } else {
                None
            };
            let running_task = if rewound_input.is_some() {
                None
            } else {
                state.foreground.take_regular()
            };

            // Decide which queued inputs get resolved with `Cancelled` now vs.
            // preserved for the post-cancel drain:
            //
            // * rewind: the front was already popped above; respond to nothing.
            // * hard teardown (`kill_background_tasks`, the subagent-shutdown
            //   path that sends `Shutdown` next): drain the WHOLE queue — there
            //   is no point starting the next prompt and draining resolves every
            //   queued input's `respond_to` cleanly.
            // * normal cancel: remove the running turn; only Ctrl+C also removes
            //   queued task/workflow completion wakes. Preserve real user prompts
            //   and unrelated synthetic entries so `maybe_start_running_task` can
            //   promote the next genuine user turn.
            //   The cancelling client does not pull any prompt back into its
            //   input — the server queue is the single source of truth for what
            //   runs next. Previously every cancel did `std::mem::take`,
            //   discarding the whole queue server-side; because no broadcast
            //   followed, clients kept a stale mirror and the queue only visibly
            //   vanished on the next prompt's (now-empty) broadcast.
            //
            // The in-flight turn is always `pending_inputs.front()`
            // (`maybe_start_running_task` promotes the front WITHOUT popping it;
            // `handle_completion` pops it at turn end) — so index 0 is the slot
            // whose `respond_to` the client is awaiting and whose spinner is
            // showing. We ALWAYS resolve it with `Cancelled`, regardless of
            // whether `running_task` is currently `Some`: in the narrow windows
            // where the front has no live task (e.g. a completion was just
            // dequeued and the next prompt not yet promoted, or a cancel races
            // ahead of `maybe_start_running_task`), gating on `running_task`
            // would drop the front's `respond_to`, hanging the client's
            // `session/prompt` forever (the TUI spinner never returns to idle).
            let pending_inputs = if rewound_input.is_some() {
                VecDeque::new()
            } else if kill_background_tasks {
                std::mem::take(&mut state.pending_inputs)
            } else {
                let mut kept = VecDeque::with_capacity(state.pending_inputs.len());
                let mut cancelled = VecDeque::new();
                for (idx, item) in std::mem::take(&mut state.pending_inputs)
                    .into_iter()
                    .enumerate()
                {
                    let is_running_turn = idx == 0;
                    if is_running_turn {
                        cancelled.push_back(item);
                    } else if suppress_task_wakes
                        && matches!(
                            &item.origin,
                            super::PromptOrigin::TaskCompleted { .. }
                                | super::PromptOrigin::WorkflowHandoff { .. }
                                | super::PromptOrigin::PlanHandoff { .. }
                        )
                    {
                        Self::respond_removed_prompt(item.respond_to);
                    } else {
                        kept.push_back(item);
                    }
                }
                state.pending_inputs = kept;
                cancelled
            };
            // Whether a user prompt remains queued behind the just-cancelled
            // turn. It distinguishes the next turn's redirect kind for
            // diagnostics: `queued_after_cancel` (a queued prompt is promoted)
            // vs `cancel_then_send` (the user types a fresh prompt). Synthetic
            // inputs (auto-wake / nudges) are not user redirects.
            let had_queued_user_prompt = state
                .pending_inputs
                .iter()
                .any(|i| !i.origin.is_synthetic());
            // NOTE: `current_prompt_id` is deliberately NOT cleared here —
            // cancel usage attribution must snapshot the ledger against the
            // live pin first; it is cleared below, right after the
            // `finalize_usage_from_outcome` / `snapshot_prompt_usage` call.
            (
                running_task,
                pending_inputs,
                rewound_input,
                had_queued_user_prompt,
            )
        };
        // Authoritative cancel identity: the task actually torn down. The pin
        // is only a fallback for windows where a cancel raced ahead of the
        // promote (no task yet) — see the capture comment above.
        let cancelled_prompt_id = running_task
            .as_ref()
            .map(|t| t.prompt_id.clone())
            .or(pinned_prompt_id);
        // `cancelled_prompt_id` is moved by `emit_turn_completed` below; keep a
        // copy for the observe-only `StopCancelled` hook dispatched at the end
        // of the cancel (after every client-facing resolution).
        let stop_cancelled_prompt_id = cancelled_prompt_id.clone();
        let cancelled_identity = running_task
            .as_ref()
            .map(|task| (task.origin.clone(), task.turn_kind))
            .or_else(|| {
                pending_inputs
                    .front()
                    .map(|input| (input.origin.clone(), input.turn_kind))
            });

        // Abort drops the wait tool's select future, so transfer or retire its
        // reservations while the authoritative prompt identity is still
        // available. This closes cancel-vs-completion without synthesizing a
        // second result for the original tool call.
        if let Some(prompt_id) = cancelled_prompt_id.as_deref() {
            if kill_background_tasks {
                self.completion_delivery.consume_turn_waits(prompt_id);
            } else {
                self.completion_delivery.defer_turn_waits(prompt_id);
            }
        }

        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentPromptIdResource(
                    String::new(),
                ),
            )
            .await;
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::update_goal::GoalDelegationSnapshotResource::default(),
            )
            .await;
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::update_goal::GoalMutationAuthorityResource::default(),
            )
            .await;
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentSubagentOwnerResource::default(),
            )
            .await;
        // Cancellation aborts the in-turn goal loop before its post-loop
        // cleanup runs, so clear the goal-loop flag here too.
        self.set_goal_loop_active(false);

        self.events.cancel_active_tool();
        // A cancel is not complete until its turn terminal is durable. Pristine
        // cancel then appends a Rewind selection, so the abandoned prompt is
        // hidden from Surface while remaining visible in the causal ledger.
        let cancellation_context = Some(serde_json::json!({
            "trigger": trigger.as_deref(),
            "pristine": rewound_input.is_some(),
        }));
        let cancelled_turn = self.events.current_turn();
        let had_active_turn = cancelled_turn.is_some();
        let current_prompt_index = self.current_turn_number.get() as usize;
        let mut terminal_error = self
            .emit_turn_ended(
                crate::session::events::TurnOutcomeLabel::Cancelled,
                chat_state::TurnTerminal {
                    stop_reason: "cancelled".into(),
                    completion_kind: "cancelled".into(),
                },
                Some(crate::session::events::CancellationCategory::MidTurnAbort),
                cancellation_context,
            )
            .await
            .err();
        if terminal_error.is_none()
            && rewound_input.is_none()
            && let (Some(prompt_id), Some(turn)) =
                (stop_cancelled_prompt_id.as_deref(), cancelled_turn)
            && let Err(error) = self
                .dispatch_observe_hook(
                    ::hooks::event::HookEventName::StopCancelled,
                    chat_state::HookCause::Turn { turn },
                    ::hooks::event::HookPayload::StopCancelled {
                        reason: crate::session::events::CancellationCategory::MidTurnAbort,
                        trigger: trigger.as_deref().map(::hooks::event::clip_cancel_trigger),
                    },
                    Some(prompt_id.to_owned()),
                )
                .await
        {
            terminal_error = Some(error);
        }
        if terminal_error.is_none()
            && rewound_input.is_none()
            && let Some(prompt_id) = cancelled_prompt_id.as_ref()
        {
            // Turn::Ended is the durable user-visible boundary. Project it to
            // clients immediately instead of keeping the TUI in `Cancelling`
            // while post-turn rewind snapshots and diagnostic usage settle.
            // The foreground admission fence remains held until this method
            // returns, and the later PromptResponse merges the usage metadata
            // into this exact prompt without finalizing it a second time.
            self.emit_turn_completed(
                prompt_id.clone(),
                cancelled_identity
                    .as_ref()
                    .map(|(origin, turn_kind)| (origin, *turn_kind)),
                &Ok(acp::StopReason::Cancelled),
                None,
                trigger.as_deref(),
            )
            .await;
        }
        if terminal_error.is_none() && rewound_input.is_none() {
            // Mark the next real user prompt as following a mid-turn abort so
            // replay/analytics/the model can see the user stopped this turn.
            self.events.set_prior_interrupt_category(
                crate::session::events::CancellationCategory::MidTurnAbort,
            );
            // Arm a one-shot `<system-reminder>` for the next real user turn,
            // but only when the abort leaves the model with NO other signal:
            // the partial assistant text is discarded out-of-band, so the only
            // remaining cue is the dangling-tool-call repair. If a tool call is
            // committed but unanswered — a tool mid-execution, OR a turn parked
            // on a permission prompt (where no tool is marked active yet) — the
            // next-turn repair already emits a "cancelled" tool-result, so we
            // skip the reminder to avoid a duplicate signal. Gating on the
            // actual dangling state (not `had_active_tool`) covers the
            // permission-prompt and partial-parallel-call cases too.
            if !self.chat_state_handle.has_dangling_tool_calls().await {
                self.events.set_pending_interrupt_reminder();
            }
            // Shared `redirect_kind` for the data pipeline: the next user turn's
            // `turn_started` records HOW the user redirected after this abort.
            self.events
                .set_prior_redirect_kind(if had_queued_user_prompt {
                    crate::session::events::RedirectKind::QueuedAfterCancel
                } else {
                    crate::session::events::RedirectKind::CancelThenSend
                });
        }

        if let Some(running_task) = running_task {
            running_task.abort();
        }
        if let Some(is_turn_active) = &self.tool_context.is_turn_active {
            is_turn_active.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        // The aborted turn's `BlockingWaitGuard`s drop asynchronously (they
        // live in tool futures owned by the drainer task / subagent spawn
        // task). Until they do, `queue_input` would read a stale depth > 0 and
        self.tool_context.blocking_wait_depth.reset();
        self.flush_pending_system_reminders().await;

        // Aborting the producer bypasses `handle_prompt`'s ordinary epilogue.
        // Retire the same session-local authorities here so cancellation cannot
        // leave the idle detector or file tracker attached to a dead turn.
        if had_active_turn {
            if let Some(extension) = &self.idle_prompt_extension {
                extension.on_turn_failed();
            }
            // `cancel_running_task` executes inside `run_session`. Do not call
            // `flush_to_disk` here: replay flushing is routed back through
            // that same actor loop and waits for its acknowledgement, so the
            // actor would wait on itself until the five-second timeout. The
            // actor-owned replay buffer is flushed before entering this
            // method; Timeline terminal persistence above remains the
            // authoritative cancellation barrier.
            self.file_state_tracker
                .end_prompt(&self.tool_context.fs, current_prompt_index)
                .await;
            if rewound_input.is_none()
                && let Some(rewind_point) = self
                    .file_state_tracker
                    .get_rewind_point(current_prompt_index)
                    .await
            {
                let _ = self
                    .notifications
                    .persistence_tx
                    .send(PersistenceMsg::RewindPoint(rewind_point));
            }
        }

        // No multi-second drain here (actor loop would block RecordSubagentUsage).
        // Same UsageDrainOutcome policy as freeze via finalize_usage_from_outcome.
        let cancelled_usage = if rewound_input.is_none() {
            if let Some(ref prompt_id) = cancelled_prompt_id {
                // Usage is diagnostic metadata, not part of the durable
                // cancellation boundary. A busy coordinator must not hold the
                // client in `Canceling` indefinitely after Turn::Ended is on
                // disk; timeout folds the turn as usage-incomplete.
                let reply = tokio::time::timeout(
                    std::time::Duration::from_millis(250),
                    self.outstanding_reply_for_prompt(prompt_id),
                )
                .await
                .unwrap_or_else(|_| {
                    tracing::warn!(prompt_id, "timed out querying cancelled-turn usage");
                    None
                });
                let outcome =
                    super::turn::UsageDrainOutcome::from_outstanding_reply(reply.as_ref());
                self.finalize_usage_from_outcome(prompt_id, outcome).await
            } else if !pending_inputs.is_empty() {
                self.snapshot_prompt_usage().await
            } else {
                None
            }
        } else {
            None
        };
        {
            let mut current_prompt_id = self
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned");
            *current_prompt_id = None;
        }
        if let Some(error) = terminal_error {
            tracing::error!(%error, "cancel stopped because its Timeline terminal was not durable");
            let boundary_error =
                crate::session::commands::fatal_turn_boundary_error("terminal", error.to_string());
            if let Some(input) = rewound_input {
                let _ = input.respond_to.send(Err(boundary_error.clone()));
            }
            for input in pending_inputs {
                let _ = input.respond_to.send(Err(boundary_error.clone()));
            }
            let queued = {
                let mut state = self.state.lock().await;
                std::mem::take(&mut state.pending_inputs)
            };
            for input in queued {
                let _ = input.respond_to.send(Err(boundary_error.clone()));
            }
            return Err(boundary_error);
        }

        if let Some(input) = rewound_input {
            let current_prompt_index = self.chat_state_handle.get_prompt_index().await;
            let target_prompt_index = current_prompt_index.saturating_sub(1);
            if let Err(error) = self
                .chat_state_handle
                .rewind_durably(target_prompt_index)
                .await
            {
                let _ = input
                    .respond_to
                    .send(Err(acp::Error::internal_error().data(format!(
                        "cancel terminal was committed but rewind was not: {error}"
                    ))));
                return Ok(());
            }
            self.file_state_tracker
                .truncate_from(target_prompt_index)
                .await;
            let _ = input.respond_to.send(Ok(PromptTurnOk {
                stop_reason: acp::StopReason::Cancelled,
                total_tokens,
                turn_snapshot: None,
                completion_kind: PromptCompletionKind::Rewound,
                structured_output: None,
                usage: None,
            }));
            return Ok(());
        }

        for (idx, input) in pending_inputs.into_iter().enumerate() {
            // Running turn is idx 0; queued prompts never spent tokens.
            let is_running_turn = idx == 0;
            if !is_running_turn
                && let Err(error) = self
                    .dismiss_input_ids(
                        input.input_ids.clone(),
                        chat_state::InputDismissReason::SessionClosing,
                    )
                    .await
            {
                let _ = input
                    .respond_to
                    .send(Err(acp::Error::internal_error().data(format!(
                        "cancel could not durably dismiss queued input: {error}"
                    ))));
                continue;
            }
            let _ = input
                .respond_to
                .send(Ok(PromptTurnOk {
                    stop_reason: acp::StopReason::Cancelled,
                    total_tokens,
                    turn_snapshot: None,
                    completion_kind: PromptCompletionKind::Cancelled {
                        // Preserve the cancellation category so local turn
                        // diagnostics match the terminal notification.
                        category: Some(crate::session::events::CancellationCategory::MidTurnAbort),
                        // Thread the trigger on the running turn only (idx 0);
                        // MvpAgent stamps it on the `PromptResponse` `_meta`.
                        context: if is_running_turn {
                            trigger
                                .clone()
                                .map(|t| crate::session::commands::CancellationContext {
                                    trigger: Some(t),
                                    ..Default::default()
                                })
                        } else {
                            None
                        },
                    },
                    structured_output: None,
                    usage: if is_running_turn {
                        cancelled_usage.clone()
                    } else {
                        None
                    },
                }))
                .ok();
        }

        Ok(())
    }
}

#[cfg(test)]
mod task_slot_tests {
    // Exercises the shared `TaskSlot<T>` primitive that backs both the deferred
    // prefix and the idle-notification debounce: arm / take / cancel / re-arm.
    use super::{TURN_USAGE_EPOCH, TaskSlot, turn_usage_epoch_or};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Spawn a 60s task that bumps `fired` by `by`, armed into `slot`.
    fn arm_counter(slot: &TaskSlot<()>, fired: &Arc<AtomicUsize>, by: usize) {
        let f = Arc::clone(fired);
        slot.arm(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            f.fetch_add(by, Ordering::SeqCst);
        }));
    }

    #[tokio::test]
    async fn foreground_usage_epoch_is_immutable_inside_the_turn_task() {
        assert_eq!(turn_usage_epoch_or(9), 9);
        TURN_USAGE_EPOCH
            .scope(4, async {
                assert_eq!(turn_usage_epoch_or(9), 4);
                tokio::task::yield_now().await;
                assert_eq!(turn_usage_epoch_or(10), 4);
            })
            .await;
        assert_eq!(turn_usage_epoch_or(10), 10);
    }

    /// A task left armed runs to completion once its delay elapses.
    #[tokio::test(start_paused = true)]
    async fn armed_task_fires_after_delay() {
        let fired = Arc::new(AtomicUsize::new(0));
        let slot: TaskSlot<()> = TaskSlot::new();
        arm_counter(&slot, &fired, 1);

        let task = slot.take().expect("task armed");
        tokio::time::advance(Duration::from_secs(61)).await;
        let _ = task.await;

        assert_eq!(fired.load(Ordering::SeqCst), 1, "armed task must fire");
    }

    /// `cancel()` aborts a still-pending task (the new-user-prompt reset path).
    #[tokio::test(start_paused = true)]
    async fn cancel_aborts_pending_task() {
        let fired = Arc::new(AtomicUsize::new(0));
        let slot: TaskSlot<()> = TaskSlot::new();
        arm_counter(&slot, &fired, 1);

        tokio::time::advance(Duration::from_secs(30)).await;
        slot.cancel();
        assert!(slot.take().is_none(), "cancel must clear the slot");
        tokio::time::advance(Duration::from_secs(120)).await;
        tokio::task::yield_now().await;

        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "cancelled task must not fire"
        );
    }

    /// Arming a new task aborts the previous one so only the latest fires.
    #[tokio::test(start_paused = true)]
    async fn rearm_aborts_previous_task() {
        let fired = Arc::new(AtomicUsize::new(0));
        let slot: TaskSlot<()> = TaskSlot::new();
        arm_counter(&slot, &fired, 1);
        arm_counter(&slot, &fired, 10);

        let task = slot.take().expect("task armed");
        tokio::time::advance(Duration::from_secs(61)).await;
        let _ = task.await;
        tokio::task::yield_now().await;

        assert_eq!(
            fired.load(Ordering::SeqCst),
            10,
            "only the re-armed task fires"
        );
    }
}
