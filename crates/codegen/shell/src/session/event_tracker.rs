use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Instant;

use chat_state::{
    ObservationEvent, RequestEvent, RequestUsage, StepEvent, StepId, TimelineEventKind,
    TimelineWriteError, ToolEvent, TurnEvent, TurnId,
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
pub struct EventTracker {
    timeline: chat_state::ChatStateHandle,
    turn_ended_emitted: Cell<bool>,
    current_turn: Cell<Option<TurnId>>,
    turn_started_at: Cell<Option<Instant>>,
    active_step: Cell<Option<(StepId, Instant)>>,
    active_requests: RefCell<BTreeMap<String, ActiveRequest>>,
    active_tools: RefCell<BTreeMap<String, ActiveTool>>,
    turn_tool_count: Cell<u32>,
    prior_interrupt_category: Cell<Option<CancellationCategory>>,
    prior_redirect_kind: Cell<Option<RedirectKind>>,
    pending_interrupt_reminder: Cell<bool>,
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
            timeline,
            turn_ended_emitted: Cell::new(false),
            current_turn: Cell::new(None),
            turn_started_at: Cell::new(None),
            active_step: Cell::new(None),
            active_requests: RefCell::new(BTreeMap::new()),
            active_tools: RefCell::new(BTreeMap::new()),
            turn_tool_count: Cell::new(0),
            prior_interrupt_category: Cell::new(None),
            prior_redirect_kind: Cell::new(None),
            pending_interrupt_reminder: Cell::new(false),
        }
    }

    /// Convert the shell producer vocabulary into either a typed causal
    /// fact or a log-only Timeline observation. The vocabulary remains useful
    /// at producer sites, but it no longer owns a second persistence rail.
    pub fn emit(&self, event: Event) {
        match &event {
            Event::TurnStarted {
                origin,
                model_id,
                conversation_message_count,
                prompt_index,
                prompt_text,
                redirect_kind,
                ..
            } => {
                // Prompt indexes are rewindable UI coordinates, not durable
                // identities. A Timeline turn id must never be reused.
                let id = TurnId(uuid::Uuid::now_v7().as_u128() as u64);
                self.current_turn.set(Some(id));
                self.turn_started_at.set(Some(Instant::now()));
                self.timeline
                    .record_timeline_event(TimelineEventKind::Turn(TurnEvent::Started {
                        id,
                        origin: origin.clone(),
                        model_id: model_id.clone(),
                        input_message_count: *conversation_message_count,
                        prompt_index: *prompt_index,
                        prompt_text: prompt_text.clone(),
                        redirect_kind: redirect_kind.map(|value| json_string(&value)),
                    }));
            }
            Event::LoopStarted { loop_index } => self.start_step(*loop_index),
            Event::ToolCompleted {
                tool_name,
                duration_ms,
                outcome,
                tool_call_id,
                ..
            } if self
                .active_tools
                .borrow_mut()
                .remove(tool_call_id)
                .is_some() =>
            {
                self.timeline.record_timeline_event(TimelineEventKind::Tool(
                    ToolEvent::Completed {
                        call_id: tool_call_id.clone(),
                        name: tool_name.clone(),
                        outcome: json_string(outcome),
                        duration_ms: *duration_ms,
                        details: None,
                    },
                ));
            }
            _ => self.record_observation(event),
        }
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
        if let Some((previous, started)) = self.active_step.take() {
            self.timeline
                .record_timeline_event(TimelineEventKind::Step(StepEvent::Ended {
                    id: previous,
                    outcome: "continued".into(),
                    duration_ms: started.elapsed().as_millis() as u64,
                }));
        }
        let Some(turn) = self.current_turn.get() else {
            tracing::error!(index, "step started without an active turn");
            return;
        };
        let id = StepId { turn, index };
        self.active_step.set(Some((id, Instant::now())));
        self.timeline
            .record_timeline_event(TimelineEventKind::Step(StepEvent::Started { id }));
    }

    pub fn begin_turn(&self) {
        self.turn_ended_emitted.set(false);
        self.turn_tool_count.set(0);
        self.active_requests.borrow_mut().clear();
        self.active_tools.borrow_mut().clear();
        self.active_step.set(None);
    }

    pub async fn emit_turn_ended(
        &self,
        outcome: TurnOutcomeLabel,
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
            self.timeline
                .record_timeline_event_durably(TimelineEventKind::Step(StepEvent::Ended {
                    id: step,
                    outcome: json_string(&outcome),
                    duration_ms: started.elapsed().as_millis() as u64,
                }))
                .await?;
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
                cancellation_category: category.map(|value| json_string(&value)),
                details: context,
            }))
            .await?;
        self.turn_ended_emitted.set(true);
        self.current_turn.set(None);
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

    pub async fn request_started(
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

    pub fn request_event(&self, event: RequestEvent) {
        let id = match &event {
            RequestEvent::FirstToken { id } | RequestEvent::Retrying { id, .. } => id,
            _ => {
                tracing::error!("request_event only accepts non-terminal request events");
                return;
            }
        };
        if !self.active_requests.borrow().contains_key(id) {
            tracing::debug!(request_id = id, "ignored late request progress event");
            return;
        }
        self.timeline
            .record_timeline_event(TimelineEventKind::Request(event));
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
        self.emit(Event::PhaseChanged {
            phase: super::event_types::Phase::PermissionPrompt,
        });
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
        self.emit(Event::PhaseChanged {
            phase: super::event_types::Phase::ToolExecution,
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

    #[tokio::test]
    async fn request_lifecycle_keeps_one_scope_until_terminal() {
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
            Box::new(chat_state::NullChatPersistence),
            event_tx,
            tokio_util::sync::CancellationToken::new(),
        );
        let tracker = EventTracker::new(handle.clone());
        tracker.begin_turn();
        tracker.emit(Event::TurnStarted {
            session_id: "session".into(),
            turn_number: 1,
            origin: "user".into(),
            model_id: "model".into(),
            yolo_mode: false,
            conversation_message_count: 0,
            prompt_index: Some(0),
            prompt_text: Some("prompt".into()),
            session_relationship: super::super::event_types::SessionRelationship::Primary,
            schema_version: super::super::event_types::EVENT_SCHEMA_VERSION.into(),
            redirect_kind: None,
        });
        tracker.emit(Event::LoopStarted { loop_index: 0 });
        tracker
            .request_started("request".into(), "model".into(), 0, 0)
            .await
            .unwrap();
        tracker.request_event(RequestEvent::FirstToken {
            id: "request".into(),
        });
        tracker.request_event(RequestEvent::Retrying {
            id: "request".into(),
            attempt: 1,
            max_retries: 2,
            reason: "transient".into(),
        });
        assert!(tracker.request_completed("request", Some(3), RequestUsage::default(), 1,));
        tracker
            .emit_turn_ended(TurnOutcomeLabel::Completed, None, None)
            .await
            .unwrap();

        let snapshot = handle.trajectory().await.unwrap();
        let request_rows = snapshot
            .rows
            .iter()
            .filter(|row| row.category == "request")
            .collect::<Vec<_>>();
        assert_eq!(request_rows.len(), 4);
        assert!(request_rows.iter().all(|row| row.turn_id.is_some()));
        assert!(request_rows.iter().all(|row| row.step_index == Some(0)));
        assert_eq!(request_rows.last().unwrap().state, "completed");
    }
}
