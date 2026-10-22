use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Instant;

use chat_state::session_runtime::{RuntimeCheckpoint, SuspendedOperationState};
use chat_state::{
    ObservationEvent, RequestEvent, RequestUsage, StepEvent, StepId, TimelineEventKind,
    TimelineWriteError, ToolEvent, TurnEvent, TurnId, TurnTerminal,
};

use super::event_types::{CancellationCategory, Event, RedirectKind, TurnOutcomeLabel};

#[derive(Debug, Clone)]
struct ActiveTool {
    name: String,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct ActiveRequest {
    started_at: Instant,
    cancellation_reason: Option<String>,
}

/// In-memory coordination for the currently executing turn.
///
/// Durable facts are written through `ChatStateHandle`; this type deliberately
/// owns no file writer and no diagnostic log. Its cells only coordinate
/// cancellation and single-terminal emission on the session actor thread.
#[derive(Clone)]
pub(crate) struct EventTracker {
    inner: std::rc::Rc<EventTrackerInner>,
}

pub(crate) struct EventTrackerInner {
    timeline: chat_state::ChatStateHandle,
    causal_gate: tokio::sync::Mutex<()>,
    turn_ended_emitted: Cell<bool>,
    current_turn: Cell<Option<TurnId>>,
    current_goal_id: RefCell<Option<String>>,
    turn_started_at: Cell<Option<Instant>>,
    active_step: Cell<Option<(StepId, Instant)>>,
    next_step_index: Cell<u32>,
    active_requests: RefCell<BTreeMap<String, ActiveRequest>>,
    active_tools: RefCell<BTreeMap<String, ActiveTool>>,
    turn_tool_count: Cell<u32>,
    prior_interrupt_category: Cell<Option<CancellationCategory>>,
    prior_redirect_kind: Cell<Option<RedirectKind>>,
    pending_interrupt_reminder: Cell<bool>,
}

impl std::ops::Deref for EventTracker {
    type Target = EventTrackerInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::fmt::Debug for EventTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventTracker")
            .field("turn_ended_emitted", &self.turn_ended_emitted.get())
            .field("current_turn", &self.current_turn.get())
            .field("active_step", &self.active_step.get().map(|(id, _)| id))
            .field("active_requests", &self.active_requests.borrow().keys())
            .field("active_tools", &self.active_tools.borrow().keys())
            .field("turn_tool_count", &self.turn_tool_count.get())
            .finish()
    }
}

impl EventTracker {
    pub fn new(timeline: chat_state::ChatStateHandle) -> Self {
        Self {
            inner: std::rc::Rc::new(EventTrackerInner {
                timeline,
                causal_gate: tokio::sync::Mutex::new(()),
                turn_ended_emitted: Cell::new(false),
                current_turn: Cell::new(None),
                current_goal_id: RefCell::new(None),
                turn_started_at: Cell::new(None),
                active_step: Cell::new(None),
                next_step_index: Cell::new(0),
                active_requests: RefCell::new(BTreeMap::new()),
                active_tools: RefCell::new(BTreeMap::new()),
                turn_tool_count: Cell::new(0),
                prior_interrupt_category: Cell::new(None),
                prior_redirect_kind: Cell::new(None),
                pending_interrupt_reminder: Cell::new(false),
            }),
        }
    }

    pub fn current_turn(&self) -> Option<TurnId> {
        self.current_turn.get()
    }

    pub fn active_step_id(&self) -> Option<StepId> {
        self.active_step.get().map(|(id, _)| id)
    }

    pub fn active_tool_ids(&self) -> Vec<String> {
        self.active_tools.borrow().keys().cloned().collect()
    }

    /// Close provider request scopes before writing a suspension checkpoint.
    /// Turn and step scopes remain open so the same durable turn can resume.
    pub async fn settle_requests_for_suspension(&self) {
        let _causal = self.causal_gate.lock().await;
        self.close_active_requests("session_suspended");
    }

    /// Reconstruct only the in-memory cursor represented by a validated
    /// runtime checkpoint. This deliberately emits no Timeline starts: the
    /// durable Turn, Step, and Tool facts already exist in the checkpoint's
    /// source history.
    pub fn restore_checkpoint(
        &self,
        checkpoint: &RuntimeCheckpoint,
        goal_id: Option<String>,
        tool_names: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if self.current_turn.get().is_some()
            || self.active_step.get().is_some()
            || !self.active_requests.borrow().is_empty()
            || !self.active_tools.borrow().is_empty()
        {
            return Err("cannot restore a checkpoint over active tracker state".into());
        }

        let turn = checkpoint.turn.as_ref();
        match (turn, checkpoint.step) {
            (None, Some(_)) => return Err("checkpoint step has no active turn".into()),
            (Some(turn), Some(step))
                if step.turn != turn.turn_id
                    || (step.index != u32::MAX && turn.next_step_index <= step.index) =>
            {
                return Err("checkpoint step does not match its turn cursor".into());
            }
            _ => {}
        }
        let tool_count = turn
            .map(|turn| {
                u32::try_from(turn.tool_calls_used)
                    .map_err(|_| "checkpoint tool count exceeds tracker capacity".to_string())
            })
            .transpose()?
            .unwrap_or(0);

        let mut active_tools = BTreeMap::new();
        if let Some(turn) = turn {
            for operation in &turn.operations {
                if !matches!(
                    &operation.state,
                    SuspendedOperationState::AwaitChild
                        | SuspendedOperationState::AwaitUser
                        | SuspendedOperationState::AwaitPermission
                ) {
                    continue;
                }
                let Some(name) = tool_names.get(&operation.call_id) else {
                    return Err(format!(
                        "checkpoint has no original Timeline tool name for {}",
                        operation.call_id
                    ));
                };
                if name.trim().is_empty() {
                    return Err(format!(
                        "checkpoint has an empty original Timeline tool name for {}",
                        operation.call_id
                    ));
                }
                if active_tools
                    .insert(
                        operation.call_id.clone(),
                        ActiveTool {
                            name: name.clone(),
                            started_at: Instant::now(),
                        },
                    )
                    .is_some()
                {
                    return Err("checkpoint repeats an active tool call identity".into());
                }
            }
        }

        self.current_turn.set(turn.map(|turn| turn.turn_id));
        *self.current_goal_id.borrow_mut() = turn.and(goal_id);
        self.turn_started_at.set(turn.map(|_| Instant::now()));
        self.active_step
            .set(checkpoint.step.map(|step| (step, Instant::now())));
        self.next_step_index
            .set(turn.map_or(0, |turn| turn.next_step_index));
        *self.active_tools.borrow_mut() = active_tools;
        self.turn_tool_count.set(tool_count);
        self.turn_ended_emitted.set(false);
        Ok(())
    }

    pub async fn wait_for_causal_idle(&self) {
        let _causal = self.causal_gate.lock().await;
    }

    /// Goal epoch captured by the active durable `Turn::Started`, if any.
    /// This remains available after the Goal lifecycle itself becomes
    /// terminal so an asynchronously-settled descendant charge can still
    /// fence the exact turn that was admitted under that Goal.
    pub fn current_goal_id(&self) -> Option<String> {
        self.current_goal_id.borrow().clone()
    }

    /// Convert the shell producer vocabulary into either a typed causal fact
    /// or a log-only Timeline observation. Tool and request lifecycle facts
    /// use their dedicated methods instead of passing through this vocabulary.
    pub fn emit(&self, event: Event) {
        match &event {
            Event::TurnStarted { .. } => {
                tracing::error!("TurnStarted must go through the durable start_turn boundary")
            }
            Event::LoopStarted { loop_index } => self.start_step(*loop_index),
            _ => self.record_observation(event),
        }
    }

    /// Persist the causal turn boundary before any turn work is admitted.
    pub async fn start_turn(&self, event: Event) -> Result<(), TimelineWriteError> {
        let tracker = self.clone();
        tokio::task::spawn_local(async move {
            let _causal = tracker.causal_gate.lock().await;
            tracker.start_turn_transaction(event).await
        })
        .await
        .map_err(|_| TimelineWriteError::AcknowledgementLost)?
    }

    async fn start_turn_transaction(&self, event: Event) -> Result<(), TimelineWriteError> {
        let Event::TurnStarted {
            prompt_id,
            input_ids,
            identity,
            model_id,
            conversation_message_count,
            prompt_index,
            prompt_text,
            input_kind,
            redirect_kind,
            ..
        } = event
        else {
            return Err(missing_boundary("start_turn requires TurnStarted"));
        };
        let prompt_index =
            prompt_index.ok_or_else(|| missing_boundary("TurnStarted requires prompt_index"))?;
        let prompt_text =
            prompt_text.ok_or_else(|| missing_boundary("TurnStarted requires prompt_text"))?;
        let goal_id = identity.goal_id.clone();
        let id = TurnId(uuid::Uuid::now_v7().as_u128() as u64);
        self.timeline
            .record_timeline_event_durably(TimelineEventKind::Turn(TurnEvent::Started {
                id,
                prompt_id,
                input_ids,
                identity,
                model_id,
                input_message_count: conversation_message_count,
                prompt_index,
                prompt_text,
                input_kind,
                redirect_kind: redirect_kind.map(|value| json_string(&value)),
            }))
            .await?;
        self.current_turn.set(Some(id));
        *self.current_goal_id.borrow_mut() = goal_id;
        self.turn_started_at.set(Some(Instant::now()));
        Ok(())
    }

    fn record_observation(&self, event: Event) {
        let mut value = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
        let name = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("shell_event")
            .to_owned();
        if let Some(object) = value.as_object_mut() {
            object.remove("type");
        }
        self.timeline
            .record_timeline_event(TimelineEventKind::Observation(ObservationEvent {
                scope: "shell".into(),
                name,
                turn: self.current_turn.get(),
                step: self.active_step.get().map(|(id, _)| id),
                data: (!value.is_null()).then_some(value),
            }));
    }

    fn start_step(&self, index: u32) {
        if let Some((active, _)) = self.active_step.get() {
            tracing::error!(?active, index, "step started before the active step ended");
            return;
        }
        let Some(turn) = self.current_turn.get() else {
            tracing::error!(index, "step started without an active turn");
            return;
        };
        let id = StepId { turn, index };
        self.active_step.set(Some((id, Instant::now())));
        self.next_step_index.set(index.saturating_add(1));
        self.timeline
            .record_timeline_event(TimelineEventKind::Step(StepEvent::Started { id }));
    }

    /// End the current sampling/tool step before any control transition that
    /// can govern the next model request is committed. The event is buffered;
    /// a following durable Control or turn terminal flushes it in mailbox
    /// order, preserving the causal boundary without adding an fsync to every
    /// ordinary tool loop.
    pub fn end_step(&self, outcome: &str) -> bool {
        let Some((id, started)) = self.active_step.take() else {
            return false;
        };
        self.timeline
            .record_timeline_event(TimelineEventKind::Step(StepEvent::Ended {
                id,
                outcome: outcome.into(),
                duration_ms: started.elapsed().as_millis() as u64,
            }));
        true
    }

    pub fn has_active_step(&self) -> bool {
        self.active_step.get().is_some()
    }

    pub fn next_step_index(&self) -> u32 {
        self.next_step_index.get()
    }

    pub fn begin_turn(&self) {
        self.turn_ended_emitted.set(false);
        self.turn_tool_count.set(0);
        self.active_requests.borrow_mut().clear();
        self.active_tools.borrow_mut().clear();
        self.active_step.set(None);
        self.next_step_index.set(0);
    }

    pub async fn emit_turn_ended(
        &self,
        outcome: TurnOutcomeLabel,
        terminal: TurnTerminal,
        category: Option<CancellationCategory>,
        context: Option<serde_json::Value>,
    ) -> Result<(), TimelineWriteError> {
        let tracker = self.clone();
        tokio::task::spawn_local(async move {
            let _causal = tracker.causal_gate.lock().await;
            tracker
                .emit_turn_ended_transaction(outcome, terminal, category, context)
                .await
        })
        .await
        .map_err(|_| TimelineWriteError::AcknowledgementLost)?
    }

    async fn emit_turn_ended_transaction(
        &self,
        outcome: TurnOutcomeLabel,
        terminal: TurnTerminal,
        category: Option<CancellationCategory>,
        context: Option<serde_json::Value>,
    ) -> Result<(), TimelineWriteError> {
        if self.turn_ended_emitted.get() {
            return Ok(());
        }
        let Some(turn) = self.current_turn.get() else {
            tracing::debug!("ignoring terminal event without an active turn");
            return Ok(());
        };
        if !self.active_requests.borrow().is_empty() {
            self.close_active_requests("turn_ended_before_request_terminal");
        }
        if !self.active_tools.borrow().is_empty() {
            self.close_active_tools("outcome_unknown", true);
        }
        if let Some((step, started)) = self.active_step.get() {
            // The immediately following Turn::Ended is the terminal durable
            // barrier for this ordered actor mailbox. Buffer the step terminal
            // first so one file+directory sync commits both boundaries instead
            // of making Stop pay two durability round trips.
            self.timeline
                .record_timeline_event(TimelineEventKind::Step(StepEvent::Ended {
                    id: step,
                    outcome: json_string(&outcome),
                    duration_ms: started.elapsed().as_millis() as u64,
                }));
            self.active_step.set(None);
        }
        let duration_ms = self
            .turn_started_at
            .get()
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);
        self.timeline
            .record_timeline_event_durably(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: json_string(&outcome),
                duration_ms,
                tool_count: self.turn_tool_count.get(),
                terminal,
                cancellation_category: category.map(|value| json_string(&value)),
                details: context,
            }))
            .await?;
        self.turn_ended_emitted.set(true);
        self.current_turn.set(None);
        self.current_goal_id.borrow_mut().take();
        self.turn_started_at.set(None);
        Ok(())
    }

    /// Persist a tool start before execution is allowed to begin.
    pub async fn tool_started(
        &self,
        name: String,
        call_id: String,
        input: Option<serde_json::Value>,
    ) -> Result<(), TimelineWriteError> {
        let tracker = self.clone();
        tokio::task::spawn_local(async move {
            let _causal = tracker.causal_gate.lock().await;
            tracker.tool_started_transaction(name, call_id, input).await
        })
        .await
        .map_err(|_| TimelineWriteError::AcknowledgementLost)?
    }

    async fn tool_started_transaction(
        &self,
        name: String,
        call_id: String,
        input: Option<serde_json::Value>,
    ) -> Result<(), TimelineWriteError> {
        let Some(turn) = self.current_turn.get() else {
            return Err(missing_boundary("tool start has no active turn"));
        };
        let Some((step, _)) = self.active_step.get() else {
            return Err(missing_boundary("tool start has no active step"));
        };
        self.timeline
            .record_timeline_event_durably(TimelineEventKind::Tool(ToolEvent::Started {
                call_id: call_id.clone(),
                turn,
                step,
                name: name.clone(),
                input,
            }))
            .await?;
        self.active_tools.borrow_mut().insert(
            call_id,
            ActiveTool {
                name,
                started_at: Instant::now(),
            },
        );
        self.turn_tool_count
            .set(self.turn_tool_count.get().saturating_add(1));
        Ok(())
    }

    /// Persist the terminal fact for a tool whose external effect has already
    /// returned. The caller must not begin the next model step or declare the
    /// tool loop complete until this acknowledgement completes.
    pub async fn tool_completed_durably(
        &self,
        call_id: &str,
        outcome: String,
        details: Option<serde_json::Value>,
    ) -> Result<(), TimelineWriteError> {
        let tracker = self.clone();
        let call_id = call_id.to_owned();
        tokio::task::spawn_local(async move {
            let _causal = tracker.causal_gate.lock().await;
            tracker
                .tool_completed_transaction(call_id, outcome, details)
                .await
        })
        .await
        .map_err(|_| TimelineWriteError::AcknowledgementLost)?
    }

    async fn tool_completed_transaction(
        &self,
        call_id: String,
        outcome: String,
        details: Option<serde_json::Value>,
    ) -> Result<(), TimelineWriteError> {
        let Some(tool) = self.active_tools.borrow_mut().remove(&call_id) else {
            return Err(missing_boundary("tool completion has no durable start"));
        };
        self.timeline
            .record_timeline_event_durably(TimelineEventKind::Tool(ToolEvent::Completed {
                call_id,
                name: tool.name,
                outcome,
                duration_ms: tool.started_at.elapsed().as_millis() as u64,
                details,
            }))
            .await?;
        Ok(())
    }

    pub async fn request_started(
        &self,
        id: String,
        model_id: String,
        input_message_count: usize,
        tool_count: usize,
    ) -> Result<(), TimelineWriteError> {
        let tracker = self.clone();
        tokio::task::spawn_local(async move {
            let _causal = tracker.causal_gate.lock().await;
            tracker
                .request_started_transaction(id, model_id, input_message_count, tool_count)
                .await
        })
        .await
        .map_err(|_| TimelineWriteError::AcknowledgementLost)?
    }

    async fn request_started_transaction(
        &self,
        id: String,
        model_id: String,
        input_message_count: usize,
        tool_count: usize,
    ) -> Result<(), TimelineWriteError> {
        let Some(turn) = self.current_turn.get() else {
            return Err(missing_boundary("request start has no active turn"));
        };
        let Some((step, _)) = self.active_step.get() else {
            return Err(missing_boundary("request start has no active step"));
        };
        self.timeline
            .record_timeline_event_durably(TimelineEventKind::Request(RequestEvent::Started {
                id: id.clone(),
                turn,
                step,
                model_id,
                input_message_count,
                tool_count,
            }))
            .await?;
        self.active_requests.borrow_mut().insert(
            id,
            ActiveRequest {
                started_at: Instant::now(),
                cancellation_reason: None,
            },
        );
        Ok(())
    }

    pub fn request_cancel_requested(&self, id: &str, reason: &str) {
        if let Some(request) = self.active_requests.borrow_mut().get_mut(id) {
            request.cancellation_reason = Some(reason.to_owned());
        }
    }

    pub fn request_completed(
        &self,
        id: &str,
        time_to_first_token_ms: Option<u64>,
        usage: RequestUsage,
        response_message_count: usize,
        attempt: u32,
        provider_terminal: Option<sampling_types::ProviderTerminal>,
    ) -> bool {
        let Some(request) = self.active_requests.borrow_mut().remove(id) else {
            return false;
        };
        self.timeline
            .record_timeline_event(TimelineEventKind::Request(RequestEvent::Completed {
                id: id.to_owned(),
                duration_ms: request.started_at.elapsed().as_millis() as u64,
                time_to_first_token_ms,
                usage,
                response_message_count,
                attempt,
                provider_terminal,
            }));
        true
    }

    pub fn request_failed(
        &self,
        id: &str,
        error_kind: &str,
        message: &str,
        retryable: bool,
    ) -> bool {
        let Some(request) = self.active_requests.borrow_mut().remove(id) else {
            return false;
        };
        let duration_ms = request.started_at.elapsed().as_millis() as u64;
        let event = match request.cancellation_reason {
            Some(reason) => RequestEvent::Cancelled {
                id: id.to_owned(),
                duration_ms,
                reason,
            },
            None => RequestEvent::Failed {
                id: id.to_owned(),
                duration_ms,
                error_kind: error_kind.to_owned(),
                message: message.to_owned(),
                retryable,
            },
        };
        self.timeline
            .record_timeline_event(TimelineEventKind::Request(event));
        true
    }

    fn close_active_requests(&self, reason: &str) {
        let active = std::mem::take(&mut *self.active_requests.borrow_mut());
        for (id, request) in active {
            self.timeline
                .record_timeline_event(TimelineEventKind::Request(RequestEvent::Cancelled {
                    id,
                    duration_ms: request.started_at.elapsed().as_millis() as u64,
                    reason: reason.to_owned(),
                }));
        }
    }

    pub fn tool_count_this_turn(&self) -> u32 {
        self.turn_tool_count.get()
    }

    pub fn has_active_tool(&self) -> bool {
        !self.active_tools.borrow().is_empty()
    }

    pub fn cancel_active_tool(&self) {
        self.close_active_tools("cancelled", false);
    }

    fn close_active_tools(&self, outcome: &str, recovered: bool) {
        let active = std::mem::take(&mut *self.active_tools.borrow_mut());
        for (call_id, tool) in active {
            self.timeline
                .record_timeline_event(TimelineEventKind::Tool(ToolEvent::Completed {
                    call_id,
                    name: tool.name,
                    outcome: outcome.into(),
                    duration_ms: tool.started_at.elapsed().as_millis() as u64,
                    details: Some(serde_json::json!({ "recovered": recovered })),
                }));
        }
    }

    pub fn set_prior_interrupt_category(&self, category: CancellationCategory) {
        if super::events::prior_turn_interrupt_from_cancellation(category).is_some() {
            self.prior_interrupt_category.set(Some(category));
        }
    }

    pub fn take_prior_interrupt_category(&self) -> Option<CancellationCategory> {
        self.prior_interrupt_category.take()
    }

    pub fn set_prior_redirect_kind(&self, kind: RedirectKind) {
        self.prior_redirect_kind.set(Some(kind));
    }

    pub fn take_prior_redirect_kind(&self) -> Option<RedirectKind> {
        self.prior_redirect_kind.take()
    }

    pub fn set_pending_interrupt_reminder(&self) {
        self.pending_interrupt_reminder.set(true);
    }

    pub fn take_pending_interrupt_reminder(&self) -> bool {
        self.pending_interrupt_reminder.replace(false)
    }

    pub fn permission_requested(&self, tool_name: &str) -> Instant {
        self.emit(Event::PermissionRequested {
            tool_name: tool_name.to_string(),
        });
        Instant::now()
    }

    pub fn permission_resolved(
        &self,
        tool_name: &str,
        decision: super::event_types::PermissionDecision,
        start: Instant,
    ) {
        self.emit(Event::PermissionResolved {
            tool_name: tool_name.to_string(),
            decision,
            wait_ms: start.elapsed().as_millis() as u64,
        });
    }
}

fn json_string(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn missing_boundary(message: &'static str) -> TimelineWriteError {
    TimelineWriteError::Persistence(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_step_boundaries_keep_monotonic_indices() {
        let tracker = EventTracker::new(chat_state::ChatStateHandle::noop());
        tracker.current_turn.set(Some(TurnId(7)));

        tracker.start_step(0);
        assert!(tracker.has_active_step());
        assert_eq!(tracker.next_step_index(), 1);
        tracker.start_step(1);
        assert_eq!(
            tracker.next_step_index(),
            1,
            "an active step cannot be replaced implicitly"
        );

        assert!(tracker.end_step("continued"));
        tracker.start_step(tracker.next_step_index());
        assert_eq!(tracker.next_step_index(), 2);
    }

    fn test_identity() -> chat_state::TurnIdentity {
        chat_state::TurnIdentity {
            goal_definition_revision: None,
            origin: "test".into(),
            turn_kind: "internal".into(),
            goal_id: None,
            stage_id: None,
        }
    }

    fn test_runtime_checkpoint() -> RuntimeCheckpoint {
        let turn_id = TurnId(77);
        RuntimeCheckpoint {
            frontier: chat_state::EventSeq::new(10),
            runtime_source_hash: "a".repeat(64),
            turn: Some(chat_state::session_runtime::TurnContinuation {
                turn_id,
                prompt_id: "prompt-77".into(),
                prompt_index: 2,
                origin: "user".into(),
                next_step_index: 4,
                tool_turns_used: 3,
                tool_calls_used: 5,
                stop_continuations_used: 0,
                output_schema: None,
                resume_point: chat_state::session_runtime::TurnResumePoint::Model,
                task_output_usage: None,
                accepted_response: None,
                structured_output_retries: 0,
                sampling_attempts_used: 1,
                sampling_max_attempts: Some(4),
                sampling_replay_safe: true,
                sampling_deadline_ms: None,
                operations: vec![
                    chat_state::session_runtime::SuspendedOperation {
                        call_id: "await-child".into(),
                        state: SuspendedOperationState::AwaitChild,
                        dependency_id: Some("child-session".into()),
                    },
                    chat_state::session_runtime::SuspendedOperation {
                        call_id: "await-user".into(),
                        state: SuspendedOperationState::AwaitUser,
                        dependency_id: Some("question-1".into()),
                    },
                    chat_state::session_runtime::SuspendedOperation {
                        call_id: "completed".into(),
                        state: SuspendedOperationState::Completed,
                        dependency_id: None,
                    },
                ],
            }),
            step: Some(StepId {
                turn: turn_id,
                index: 3,
            }),
        }
    }

    fn test_chat_state() -> chat_state::ChatStateHandle {
        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        chat_state::ChatStateActor::spawn(
            vec![],
            sampling_types::SamplingConfig {
                base_url: "http://localhost".into(),
                model: "model".into(),
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                env_http_headers: Default::default(),
                query_params: Default::default(),
                context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            Box::new(chat_state::NullTimelinePersistence),
            event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restoring_checkpoint_rebuilds_cursor_without_emitting_start_events() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let handle = test_chat_state();
                let tracker = EventTracker::new(handle.clone());
                let checkpoint = test_runtime_checkpoint();
                let names = BTreeMap::from([
                    ("await-child".into(), "delegate".into()),
                    ("await-user".into(), "ask_user".into()),
                    ("completed".into(), "read_file".into()),
                ]);

                tracker
                    .restore_checkpoint(&checkpoint, Some("goal-1".into()), &names)
                    .unwrap();

                assert_eq!(tracker.current_turn(), Some(TurnId(77)));
                assert_eq!(tracker.current_goal_id().as_deref(), Some("goal-1"));
                assert_eq!(tracker.active_step_id(), checkpoint.step);
                assert_eq!(tracker.next_step_index(), 4);
                assert_eq!(tracker.tool_count_this_turn(), 5);
                assert_eq!(
                    tracker.active_tool_ids(),
                    vec!["await-child".to_string(), "await-user".to_string()]
                );
                assert!(handle.timeline_events().await.unwrap().is_empty());
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn suspension_settlement_closes_requests_without_ending_turn_or_step() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let handle = test_chat_state();
                let tracker = EventTracker::new(handle.clone());
                tracker
                    .start_turn(Event::TurnStarted {
                        prompt_id: "fixture-prompt".into(),
                        session_id: "session".into(),
                        turn_number: 1,
                        input_ids: Vec::new(),
                        identity: test_identity(),
                        model_id: "model".into(),
                        permission_mode: diagnostics::enums::PermissionMode::Ask,
                        conversation_message_count: 0,
                        prompt_index: Some(0),
                        prompt_text: Some("prompt".into()),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        session_relationship:
                            super::super::event_types::SessionRelationship::Primary,
                        schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
                        redirect_kind: None,
                    })
                    .await
                    .unwrap();
                let turn_id = tracker.current_turn().unwrap();
                tracker.emit(Event::LoopStarted { loop_index: 0 });
                tracker
                    .request_started("request-1".into(), "model".into(), 0, 0)
                    .await
                    .unwrap();

                tracker.settle_requests_for_suspension().await;

                let events = handle.timeline_events().await.unwrap();
                let serialized: Vec<_> = events
                    .iter()
                    .map(|event| serde_json::to_value(event).unwrap())
                    .collect();
                assert!(serialized.iter().any(|event| {
                    event["type"] == "request"
                        && event["event"]["state"] == "cancelled"
                        && event["event"]["id"] == "request-1"
                        && event["event"]["reason"] == "session_suspended"
                }));
                assert!(serialized.iter().all(|event| {
                    !(((event["type"] == "turn" || event["type"] == "step")
                        && event["event"]["state"] == "ended")
                        || (event["type"] == "tool" && event["event"]["state"] == "completed"))
                }));
                assert!(tracker.active_requests.borrow().is_empty());
                assert_eq!(tracker.current_turn(), Some(turn_id));
                assert_eq!(
                    tracker.active_step_id(),
                    Some(StepId {
                        turn: turn_id,
                        index: 0,
                    })
                );
            })
            .await;
    }

    #[test]
    fn checkpoint_restore_rejects_conflicting_tracker_state_and_missing_tool_names() {
        let checkpoint = test_runtime_checkpoint();
        let tracker = EventTracker::new(chat_state::ChatStateHandle::noop());
        tracker.current_turn.set(Some(TurnId(9)));
        assert!(
            tracker
                .restore_checkpoint(&checkpoint, None, &BTreeMap::new())
                .is_err()
        );
        assert_eq!(tracker.current_turn(), Some(TurnId(9)));

        let empty = EventTracker::new(chat_state::ChatStateHandle::noop());
        assert!(
            empty
                .restore_checkpoint(&checkpoint, None, &BTreeMap::new())
                .is_err()
        );
        assert_eq!(empty.current_turn(), None, "failed restore must be atomic");
        assert_eq!(empty.active_step_id(), None);
        assert!(empty.active_tool_ids().is_empty());
    }

    fn terminal(stop_reason: &str, completion_kind: &str) -> TurnTerminal {
        TurnTerminal {
            source: chat_state::TurnTerminalSource::Host,
            stop_reason: stop_reason.into(),
            completion_kind: completion_kind.into(),
        }
    }

    #[test]
    fn prior_interrupt_markers_are_one_shot_and_survive_begin_turn() {
        let t = EventTracker::new(chat_state::ChatStateHandle::noop());
        assert_eq!(t.take_prior_interrupt_category(), None);
        assert!(t.take_prior_redirect_kind().is_none());
        assert!(!t.take_pending_interrupt_reminder());

        t.set_prior_interrupt_category(CancellationCategory::MidTurnAbort);
        assert_eq!(
            t.take_prior_interrupt_category(),
            Some(CancellationCategory::MidTurnAbort)
        );
        assert_eq!(t.take_prior_interrupt_category(), None);

        t.set_prior_interrupt_category(CancellationCategory::PermissionRejected);
        t.set_prior_redirect_kind(RedirectKind::CancelThenSend);
        t.set_pending_interrupt_reminder();
        t.begin_turn();
        assert_eq!(
            t.take_prior_interrupt_category(),
            Some(CancellationCategory::PermissionRejected)
        );
        assert!(matches!(
            t.take_prior_redirect_kind(),
            Some(RedirectKind::CancelThenSend)
        ));
        assert!(t.take_pending_interrupt_reminder());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_lifecycle_keeps_one_scope_until_terminal() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
                let handle = chat_state::ChatStateActor::spawn(
                    vec![],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".into(),
                        model: "model".into(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(chat_state::NullTimelinePersistence),
                    event_tx,
                    tokio_util::sync::CancellationToken::new(),
                );
                let tracker = EventTracker::new(handle.clone());
                tracker.begin_turn();
                tracker
                    .start_turn(Event::TurnStarted {
                        prompt_id: "fixture-prompt".into(),
                        session_id: "session".into(),
                        turn_number: 1,
                        input_ids: Vec::new(),
                        identity: test_identity(),
                        model_id: "model".into(),
                        permission_mode: diagnostics::enums::PermissionMode::Ask,
                        conversation_message_count: 0,
                        prompt_index: Some(0),
                        prompt_text: Some("prompt".into()),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        session_relationship:
                            super::super::event_types::SessionRelationship::Primary,
                        schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
                        redirect_kind: None,
                    })
                    .await
                    .unwrap();
                tracker.emit(Event::LoopStarted { loop_index: 0 });
                tracker
                    .request_started("request".into(), "model".into(), 0, 0)
                    .await
                    .unwrap();
                handle
                    .record_timeline_event_durably(TimelineEventKind::Request(
                        RequestEvent::Retrying {
                            id: "request".into(),
                            attempt: 1,
                            max_retries: 2,
                            reason: "transient".into(),
                        },
                    ))
                    .await
                    .unwrap();
                assert!(tracker.request_completed(
                    "request",
                    Some(3),
                    RequestUsage::default(),
                    1,
                    0,
                    None,
                ));
                tracker
                    .emit_turn_ended(
                        TurnOutcomeLabel::Completed,
                        terminal("end_turn", "completed"),
                        None,
                        None,
                    )
                    .await
                    .unwrap();

                let snapshot = handle.trajectory().await.unwrap();
                let request_rows = snapshot
                    .rows
                    .iter()
                    .filter(|row| row.kind.starts_with("request."))
                    .collect::<Vec<_>>();
                assert_eq!(request_rows.len(), 3);
                assert!(request_rows.iter().all(|row| row.turn_id.is_some()));
                assert!(request_rows.iter().all(|row| row.step_index == Some(0)));
                assert_eq!(request_rows.last().unwrap().state, "completed");
                assert!(
                    snapshot
                        .rows
                        .iter()
                        .all(|row| row.kind != "observation.phase_changed"
                            && row.kind != "observation.first_token"),
                    "stream progress belongs to live updates, not Timeline"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tool_completion_waits_for_durable_terminal_ack() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
                // Turn start, step start, and tool start form the admitted prefix. Keep
                // the tool terminal acknowledgement under explicit test control.
                let (persistence, mut persisted) =
                    chat_state::MockTimelinePersistence::new_with_manual_timeline_ack_after(3);
                let handle = chat_state::ChatStateActor::spawn(
                    vec![],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".into(),
                        model: "model".into(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(persistence),
                    event_tx,
                    tokio_util::sync::CancellationToken::new(),
                );
                let tracker = EventTracker::new(handle.clone());
                tracker.begin_turn();
                tracker
                    .start_turn(Event::TurnStarted {
                        prompt_id: "fixture-prompt".into(),
                        session_id: "session".into(),
                        turn_number: 1,
                        input_ids: Vec::new(),
                        identity: test_identity(),
                        model_id: "model".into(),
                        permission_mode: diagnostics::enums::PermissionMode::Ask,
                        conversation_message_count: 0,
                        prompt_index: Some(0),
                        prompt_text: Some("prompt".into()),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        session_relationship:
                            super::super::event_types::SessionRelationship::Primary,
                        schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
                        redirect_kind: None,
                    })
                    .await
                    .unwrap();
                tracker.emit(Event::LoopStarted { loop_index: 0 });
                tracker
                    .tool_started(
                        "read_file".into(),
                        "call-1".into(),
                        Some(serde_json::json!({ "path": "README.md" })),
                    )
                    .await
                    .unwrap();

                let completion = tracker.tool_completed_durably("call-1", "success".into(), None);
                let acknowledge_terminal = async {
                    persisted
                        .next_timeline_ack()
                        .await
                        .expect("tool terminal must request a durable acknowledgement")
                        .send(Ok(()))
                        .unwrap();
                };
                let (completed, ()) = tokio::join!(completion, acknowledge_terminal);
                completed.unwrap();
                assert!(!tracker.has_active_tool());

                let snapshot = handle.trajectory().await.unwrap();
                let tool_rows = snapshot
                    .rows
                    .iter()
                    .filter(|row| row.kind.starts_with("tool."))
                    .collect::<Vec<_>>();
                assert_eq!(tool_rows.len(), 2);
                assert_eq!(tool_rows[0].kind, "tool.call");
                assert_eq!(tool_rows[1].kind, "tool.result");
                assert_eq!(tool_rows[1].state, "completed");
                assert!(
                    snapshot
                        .rows
                        .iter()
                        .all(|row| !row.kind.starts_with("observation.tool_")),
                    "tool lifecycle has one typed Timeline rail"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn transient_failure_retries_exact_turn_terminal_without_duplicate() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence, mut persisted) =
                    chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
                let handle = chat_state::ChatStateActor::spawn(
                    vec![],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".into(),
                        model: "model".into(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(persistence),
                    event_tx,
                    tokio_util::sync::CancellationToken::new(),
                );
                let tracker = EventTracker::new(handle.clone());
                tracker.begin_turn();
                let start = tracker.start_turn(Event::TurnStarted {
                    prompt_id: "fixture-prompt".into(),
                    session_id: "session".into(),
                    turn_number: 1,
                    input_ids: Vec::new(),
                    identity: test_identity(),
                    model_id: "model".into(),
                    permission_mode: diagnostics::enums::PermissionMode::Ask,
                    conversation_message_count: 0,
                    prompt_index: Some(0),
                    prompt_text: Some("prompt".into()),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    session_relationship: super::super::event_types::SessionRelationship::Primary,
                    schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
                    redirect_kind: None,
                });
                let accept_start = async {
                    persisted
                        .next_timeline_ack()
                        .await
                        .unwrap()
                        .send(Ok(()))
                        .unwrap();
                };
                let (started, ()) = tokio::join!(start, accept_start);
                started.unwrap();

                let first_end = tracker.emit_turn_ended(
                    TurnOutcomeLabel::Cancelled,
                    terminal("cancelled", "cancelled"),
                    Some(CancellationCategory::MidTurnAbort),
                    None,
                );
                let reject_then_accept_end = async {
                    persisted
                        .next_timeline_ack()
                        .await
                        .unwrap()
                        .send(Err(std::io::Error::other("simulated disk failure")))
                        .unwrap();
                    persisted
                        .next_timeline_ack()
                        .await
                        .expect("exact terminal retry acknowledgement")
                        .send(Ok(()))
                        .unwrap();
                };
                let (ended, ()) = tokio::join!(first_end, reject_then_accept_end);
                ended.unwrap();
                let snapshot = handle.trajectory().await.unwrap();
                assert!(snapshot.active_turn.is_none());
                assert_eq!(
                    snapshot
                        .rows
                        .iter()
                        .filter(|row| row.kind.starts_with("turn.") && row.state == "cancelled")
                        .count(),
                    1,
                    "retry must commit one terminal, not duplicate it"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn aborted_turn_start_waiter_cannot_strand_an_open_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence, mut persisted) =
                    chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
                let handle = chat_state::ChatStateActor::spawn(
                    vec![],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".into(),
                        model: "model".into(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(persistence),
                    event_tx,
                    tokio_util::sync::CancellationToken::new(),
                );
                let tracker = EventTracker::new(handle.clone());
                tracker.begin_turn();
                let start_tracker = tracker.clone();
                let start_waiter = tokio::task::spawn_local(async move {
                    start_tracker
                        .start_turn(Event::TurnStarted {
                            prompt_id: "fixture-prompt".into(),
                            session_id: "session".into(),
                            turn_number: 1,
                            input_ids: Vec::new(),
                            identity: test_identity(),
                            model_id: "model".into(),
                            permission_mode: diagnostics::enums::PermissionMode::Ask,
                            conversation_message_count: 0,
                            prompt_index: Some(0),
                            prompt_text: Some("prompt".into()),
                            input_kind: chat_state::TurnInputKind::Prompt,
                            session_relationship:
                                super::super::event_types::SessionRelationship::Primary,
                            schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
                            redirect_kind: None,
                        })
                        .await
                });
                let start_ack = persisted.next_timeline_ack().await.unwrap();
                start_waiter.abort();
                assert!(start_waiter.await.unwrap_err().is_cancelled());
                start_ack.send(Ok(())).unwrap();

                let end_tracker = tracker.clone();
                let end = tokio::task::spawn_local(async move {
                    end_tracker
                        .emit_turn_ended(
                            TurnOutcomeLabel::Cancelled,
                            terminal("cancelled", "cancelled"),
                            Some(CancellationCategory::MidTurnAbort),
                            None,
                        )
                        .await
                });
                let end_ack = persisted.next_timeline_ack().await.unwrap();
                end_ack.send(Ok(())).unwrap();
                end.await.unwrap().unwrap();

                let snapshot = handle.trajectory().await.unwrap();
                assert!(snapshot.active_turn.is_none());
                assert_eq!(
                    snapshot
                        .rows
                        .iter()
                        .filter(|row| row.kind.starts_with("turn."))
                        .count(),
                    2
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_terminal_writers_commit_the_admitted_outcome_once() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence, mut persisted) =
                    chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
                let handle = chat_state::ChatStateActor::spawn(
                    vec![],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".into(),
                        model: "model".into(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(persistence),
                    event_tx,
                    tokio_util::sync::CancellationToken::new(),
                );
                let tracker = EventTracker::new(handle.clone());
                tracker.begin_turn();
                let start_tracker = tracker.clone();
                let start = tokio::task::spawn_local(async move {
                    start_tracker
                        .start_turn(Event::TurnStarted {
                            prompt_id: "fixture-prompt".into(),
                            session_id: "session".into(),
                            turn_number: 1,
                            input_ids: Vec::new(),
                            identity: test_identity(),
                            model_id: "model".into(),
                            permission_mode: diagnostics::enums::PermissionMode::Ask,
                            conversation_message_count: 0,
                            prompt_index: Some(0),
                            prompt_text: Some("prompt".into()),
                            input_kind: chat_state::TurnInputKind::Prompt,
                            session_relationship:
                                super::super::event_types::SessionRelationship::Primary,
                            schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
                            redirect_kind: None,
                        })
                        .await
                });
                let start_ack = persisted.next_timeline_ack().await.unwrap();
                start_ack.send(Ok(())).unwrap();
                start.await.unwrap().unwrap();

                let completed_tracker = tracker.clone();
                let completed = tokio::task::spawn_local(async move {
                    completed_tracker
                        .emit_turn_ended(
                            TurnOutcomeLabel::Completed,
                            terminal("end_turn", "completed"),
                            None,
                            None,
                        )
                        .await
                });
                let completed_ack = persisted.next_timeline_ack().await.unwrap();
                let cancelled_tracker = tracker.clone();
                let cancelled = tokio::task::spawn_local(async move {
                    cancelled_tracker
                        .emit_turn_ended(
                            TurnOutcomeLabel::Cancelled,
                            terminal("cancelled", "cancelled"),
                            Some(CancellationCategory::MidTurnAbort),
                            None,
                        )
                        .await
                });
                completed_ack.send(Ok(())).unwrap();
                completed.await.unwrap().unwrap();
                cancelled.await.unwrap().unwrap();
                assert!(
                    tokio::time::timeout(
                        std::time::Duration::from_millis(20),
                        persisted.next_timeline_ack()
                    )
                    .await
                    .is_err(),
                    "the second terminal writer must not enqueue another durable event"
                );

                let snapshot = handle.trajectory().await.unwrap();
                assert!(snapshot.active_turn.is_none());
                assert_eq!(
                    snapshot
                        .rows
                        .iter()
                        .filter(|row| row.kind.starts_with("turn.") && row.state == "completed")
                        .count(),
                    1
                );
                assert_eq!(
                    snapshot
                        .rows
                        .iter()
                        .filter(|row| row.kind.starts_with("turn.") && row.state == "cancelled")
                        .count(),
                    0
                );
            })
            .await;
    }
}
