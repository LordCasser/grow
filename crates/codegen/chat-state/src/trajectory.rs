//! Read-only Trajectory projection built exclusively from Timeline events.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    CompactionEvent, MessageEvent, ObservationEvent, RecoveryEvent, RequestEvent, StepEvent,
    SurfaceId, SurfaceOp, Timeline, TimelineEvent, TimelineEventKind, ToolEvent, TurnEvent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceVisibility {
    Current,
    Shadowed,
    LogOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryRow {
    pub seq: u64,
    pub at_ms: i64,
    pub category: String,
    pub name: String,
    pub state: String,
    pub visibility: SurfaceVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub summary: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectorySnapshot {
    pub schema_version: u8,
    pub event_count: usize,
    pub current_surface_items: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_step: Option<u32>,
    pub open_requests: Vec<String>,
    pub open_tools: Vec<String>,
    pub rows: Vec<TrajectoryRow>,
}

impl Timeline {
    pub fn trajectory(&self) -> TrajectorySnapshot {
        let current_surface = self.current_surface_ids().iter().copied().collect();
        let mut request_scopes = BTreeMap::<String, (String, u32)>::new();
        let mut tool_scopes = BTreeMap::<String, (String, u32)>::new();
        let rows = self
            .events()
            .iter()
            .map(|event| {
                match &event.kind {
                    TimelineEventKind::Request(RequestEvent::Started {
                        id, turn, step, ..
                    }) => {
                        request_scopes.insert(id.clone(), (turn.0.to_string(), step.index));
                    }
                    TimelineEventKind::Tool(ToolEvent::Started {
                        call_id,
                        turn,
                        step,
                        ..
                    }) => {
                        tool_scopes.insert(call_id.clone(), (turn.0.to_string(), step.index));
                    }
                    _ => {}
                }
                row(event, &current_surface, &request_scopes, &tool_scopes)
            })
            .collect();
        TrajectorySnapshot {
            schema_version: crate::TIMELINE_SCHEMA_VERSION,
            event_count: self.events().len(),
            current_surface_items: self.surface_len(),
            active_turn: self.active_turn().map(|id| id.0.to_string()),
            active_step: self.active_step().map(|id| id.index),
            open_requests: self.open_request_ids().map(str::to_owned).collect(),
            open_tools: self.open_tool_call_ids().map(str::to_owned).collect(),
            rows,
        }
    }
}

fn row(
    event: &TimelineEvent,
    current_surface: &BTreeSet<SurfaceId>,
    request_scopes: &BTreeMap<String, (String, u32)>,
    tool_scopes: &BTreeMap<String, (String, u32)>,
) -> TrajectoryRow {
    let (category, name, state, turn_id, step_index, correlation_id, duration_ms, summary) =
        describe(&event.kind, request_scopes, tool_scopes);
    let visibility = match &event.kind {
        TimelineEventKind::Messages(messages) => {
            let has_current = (0..messages.items.len()).any(|item| {
                u32::try_from(item).is_ok_and(|item| {
                    current_surface.contains(&SurfaceId {
                        event: event.seq,
                        item,
                    })
                })
            });
            if has_current {
                SurfaceVisibility::Current
            } else {
                SurfaceVisibility::Shadowed
            }
        }
        _ => SurfaceVisibility::LogOnly,
    };
    TrajectoryRow {
        seq: event.seq.get(),
        at_ms: event.at_ms,
        category,
        name,
        state,
        visibility,
        turn_id,
        step_index,
        correlation_id,
        duration_ms,
        summary,
        details: serde_json::to_value(&event.kind).unwrap_or(serde_json::Value::Null),
    }
}

#[allow(clippy::type_complexity)]
fn describe(
    kind: &TimelineEventKind,
    request_scopes: &BTreeMap<String, (String, u32)>,
    tool_scopes: &BTreeMap<String, (String, u32)>,
) -> (
    String,
    String,
    String,
    Option<String>,
    Option<u32>,
    Option<String>,
    Option<u64>,
    String,
) {
    match kind {
        TimelineEventKind::Messages(event) => describe_message(event),
        TimelineEventKind::Turn(event) => match event {
            TurnEvent::Started {
                id,
                origin,
                model_id,
                ..
            } => tuple(
                "turn",
                "turn",
                "started",
                Some(id.0.to_string()),
                None,
                Some(id.0.to_string()),
                None,
                format!("{origin} · {model_id}"),
            ),
            TurnEvent::Ended {
                id,
                outcome,
                duration_ms,
                ..
            } => tuple(
                "turn",
                "turn",
                "ended",
                Some(id.0.to_string()),
                None,
                Some(id.0.to_string()),
                Some(*duration_ms),
                outcome.clone(),
            ),
        },
        TimelineEventKind::Step(event) => match event {
            StepEvent::Started { id } => tuple(
                "step",
                "model_tool_loop",
                "started",
                Some(id.turn.0.to_string()),
                Some(id.index),
                Some(format!("{}:{}", id.turn.0, id.index)),
                None,
                format!("step {}", id.index),
            ),
            StepEvent::Ended {
                id,
                outcome,
                duration_ms,
            } => tuple(
                "step",
                "model_tool_loop",
                "ended",
                Some(id.turn.0.to_string()),
                Some(id.index),
                Some(format!("{}:{}", id.turn.0, id.index)),
                Some(*duration_ms),
                outcome.clone(),
            ),
        },
        TimelineEventKind::Request(event) => describe_request(event, request_scopes),
        TimelineEventKind::Tool(event) => describe_tool(event, tool_scopes),
        TimelineEventKind::Compaction(event) => describe_compaction(event),
        TimelineEventKind::Recovery(RecoveryEvent {
            action,
            correlation_id,
            reason,
            ..
        }) => tuple(
            "recovery",
            action,
            "recorded",
            None,
            None,
            correlation_id.clone(),
            None,
            reason.clone(),
        ),
        TimelineEventKind::Observation(ObservationEvent {
            scope,
            name,
            turn,
            step,
            ..
        }) => tuple(
            "observation",
            name,
            "recorded",
            turn.map(|id| id.0.to_string()),
            step.map(|id| id.index),
            None,
            None,
            scope.clone(),
        ),
    }
}

fn describe_message(event: &MessageEvent) -> ReturnTuple {
    let text = event
        .items
        .iter()
        .map(|item| item.text_content())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let summary = truncate(&text, 240);
    let state = match event.surface {
        SurfaceOp::Append => "appended",
        SurfaceOp::Replace { .. } => "replaced",
    };
    tuple(
        "message",
        &format!("{:?}", event.cause).to_lowercase(),
        state,
        None,
        None,
        None,
        None,
        if summary.is_empty() {
            format!("{} item(s)", event.items.len())
        } else {
            summary
        },
    )
}

fn describe_request(event: &RequestEvent, scopes: &BTreeMap<String, (String, u32)>) -> ReturnTuple {
    match event {
        RequestEvent::Started {
            id,
            turn,
            step,
            model_id,
            ..
        } => tuple(
            "request",
            "model_request",
            "started",
            Some(turn.0.to_string()),
            Some(step.index),
            Some(id.clone()),
            None,
            model_id.clone(),
        ),
        RequestEvent::FirstToken { id } => {
            let (turn, step) = scope(scopes, id);
            tuple(
                "request",
                "model_request",
                "first_token",
                turn,
                step,
                Some(id.clone()),
                None,
                "first content token".into(),
            )
        }
        RequestEvent::Retrying {
            id,
            attempt,
            max_retries,
            reason,
        } => {
            let (turn, step) = scope(scopes, id);
            tuple(
                "request",
                "model_request",
                "retrying",
                turn,
                step,
                Some(id.clone()),
                None,
                format!("{attempt}/{max_retries} · {}", truncate(reason, 180)),
            )
        }
        RequestEvent::Completed {
            id,
            duration_ms,
            time_to_first_token_ms,
            ..
        } => {
            let (turn, step) = scope(scopes, id);
            tuple(
                "request",
                "model_request",
                "completed",
                turn,
                step,
                Some(id.clone()),
                Some(*duration_ms),
                time_to_first_token_ms
                    .map(|ttft| format!("ttft {ttft} ms"))
                    .unwrap_or_else(|| "completed".into()),
            )
        }
        RequestEvent::Failed {
            id,
            duration_ms,
            error_kind,
            message,
            ..
        } => {
            let (turn, step) = scope(scopes, id);
            tuple(
                "request",
                "model_request",
                "failed",
                turn,
                step,
                Some(id.clone()),
                Some(*duration_ms),
                format!("{error_kind} · {}", truncate(message, 180)),
            )
        }
        RequestEvent::Cancelled {
            id,
            duration_ms,
            reason,
        } => {
            let (turn, step) = scope(scopes, id);
            tuple(
                "request",
                "model_request",
                "cancelled",
                turn,
                step,
                Some(id.clone()),
                Some(*duration_ms),
                reason.clone(),
            )
        }
    }
}

fn describe_tool(event: &ToolEvent, scopes: &BTreeMap<String, (String, u32)>) -> ReturnTuple {
    match event {
        ToolEvent::Started {
            call_id,
            turn,
            step,
            name,
            ..
        } => tuple(
            "tool",
            name,
            "started",
            Some(turn.0.to_string()),
            Some(step.index),
            Some(call_id.clone()),
            None,
            name.clone(),
        ),
        ToolEvent::Completed {
            call_id,
            name,
            outcome,
            duration_ms,
            ..
        } => {
            let (turn, step) = scope(scopes, call_id);
            tuple(
                "tool",
                name,
                "completed",
                turn,
                step,
                Some(call_id.clone()),
                Some(*duration_ms),
                outcome.clone(),
            )
        }
    }
}

fn scope(scopes: &BTreeMap<String, (String, u32)>, id: &str) -> (Option<String>, Option<u32>) {
    scopes
        .get(id)
        .map(|(turn, step)| (Some(turn.clone()), Some(*step)))
        .unwrap_or((None, None))
}

fn describe_compaction(event: &CompactionEvent) -> ReturnTuple {
    match event {
        CompactionEvent::Started {
            id,
            source_items,
            prompt_index,
        } => tuple(
            "compaction",
            "compaction",
            "started",
            None,
            None,
            Some(id.clone()),
            None,
            format!("prompt {prompt_index} · {source_items} source items"),
        ),
        CompactionEvent::Completed {
            id,
            source_items,
            result_items,
            duration_ms,
        } => tuple(
            "compaction",
            "compaction",
            "completed",
            None,
            None,
            Some(id.clone()),
            Some(*duration_ms),
            format!("{source_items} → {result_items} items"),
        ),
        CompactionEvent::Failed {
            id,
            duration_ms,
            error,
        } => tuple(
            "compaction",
            "compaction",
            "failed",
            None,
            None,
            Some(id.clone()),
            Some(*duration_ms),
            truncate(error, 180),
        ),
    }
}

type ReturnTuple = (
    String,
    String,
    String,
    Option<String>,
    Option<u32>,
    Option<String>,
    Option<u64>,
    String,
);

#[allow(clippy::too_many_arguments)]
fn tuple(
    category: &str,
    name: &str,
    state: &str,
    turn_id: Option<String>,
    step_index: Option<u32>,
    correlation_id: Option<String>,
    duration_ms: Option<u64>,
    summary: String,
) -> ReturnTuple {
    (
        category.into(),
        name.into(),
        state.into(),
        turn_id,
        step_index,
        correlation_id,
        duration_ms,
        summary,
    )
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use sampling_types::ConversationItem;

    use super::*;
    use crate::MessageCause;

    #[test]
    fn projection_marks_replaced_messages_shadowed() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("old")]).unwrap();
        timeline
            .replace_all(
                vec![ConversationItem::user("current")],
                MessageCause::Rewind,
            )
            .unwrap();
        let snapshot = timeline.trajectory();
        assert_eq!(snapshot.rows[0].visibility, SurfaceVisibility::Shadowed);
        assert_eq!(snapshot.rows[1].visibility, SurfaceVisibility::Current);
    }

    #[test]
    fn projection_carries_request_scope_to_terminal_rows() {
        let mut timeline = Timeline::default();
        let turn = crate::TurnId(9);
        let step = crate::StepId { turn, index: 2 };
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                origin: "user".into(),
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: Some(0),
                prompt_text: Some("prompt".into()),
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Request(RequestEvent::Started {
                id: "request".into(),
                turn,
                step,
                model_id: "model".into(),
                input_message_count: 0,
                tool_count: 0,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Request(RequestEvent::Completed {
                id: "request".into(),
                duration_ms: 4,
                time_to_first_token_ms: Some(2),
                usage: crate::RequestUsage::default(),
                response_message_count: 1,
            }))
            .unwrap();

        let terminal = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(terminal.turn_id.as_deref(), Some("9"));
        assert_eq!(terminal.step_index, Some(2));
    }
}
