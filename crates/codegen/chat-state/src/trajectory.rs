//! Read-only Trajectory projection built exclusively from Timeline events.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    CompactionEvent, ControlContextLayer, ControlEvent, HookEvent, InputAdmissionDecision,
    InputEvent, MessageEvent, NotificationEvent, NotificationSource, ObservationEvent,
    RecoveryEvent, RequestEvent, SessionTitleEvent, SessionTitleSource, SidebandSpawnEvent,
    StepEvent, SubagentEvent, SubagentResultEvent, SubagentSeedEvent, SurfaceId, SurfaceOp,
    Timeline, TimelineEvent, TimelineEventKind, ToolEvent, TurnEvent, WorkflowEvent,
};

/// Wire schema for the read-only Trajectory projection.
///
/// This is intentionally independent from the Timeline event schema: changing
/// a debug projection must not pretend that the durable ledger format changed.
pub const TRAJECTORY_SCHEMA_VERSION: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceVisibility {
    Current,
    Shadowed,
    LogOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrajectoryIssueSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryRow {
    pub entry_id: String,
    pub seq: u64,
    /// Stable causal parent in a merged multi-ledger view.
    ///
    /// Root-ledger rows have no parent. Every row projected from a child or
    /// Sideband ledger points at the exact spawn entry that owns its span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_entry_id: Option<String>,
    /// Lexicographic position in the merged causal tree.
    ///
    /// A root event at seq 7 has `[7]`; a child event at seq 3 spawned by it
    /// has `[7, 3]`; recursively derived ledgers extend the path. This one
    /// field is both the deterministic nesting order and the depth source.
    pub nesting_path: Vec<u64>,
    pub at_ms: i64,
    pub layer: String,
    pub actor: String,
    pub class: String,
    pub producer: String,
    pub kind: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_severity: Option<TrajectoryIssueSeverity>,
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
    pub open_workflows: Vec<String>,
    pub rows: Vec<TrajectoryRow>,
}

impl Timeline {
    pub fn trajectory(&self) -> TrajectorySnapshot {
        let mut projector = TrajectoryProjector::default();
        for event in self.events() {
            projector.accept(event);
        }
        projector.snapshot(self)
    }
}

/// Incremental read model for long-running Timeline ledgers.
///
/// Surface replacements update the visibility of earlier message rows through
/// their stable `SurfaceId`s, so appending one event never requires replaying
/// the full ledger. Rows retained by the projector are deliberately payload-free;
/// canonical details are hydrated from the Timeline only when a snapshot is
/// requested.
#[derive(Debug, Clone, Default)]
pub struct TrajectoryProjector {
    rows: Vec<TrajectoryRow>,
    dirty_rows: BTreeSet<usize>,
    request_scopes: BTreeMap<String, (String, u32)>,
    tool_scopes: BTreeMap<String, (String, u32, String)>,
    surface_rows: BTreeMap<SurfaceId, usize>,
    current_items_per_row: Vec<usize>,
    control_snapshot: Option<serde_json::Value>,
    active_turn: Option<String>,
    active_step: Option<u32>,
    pending_control_rows: BTreeMap<ControlContextLayer, (SurfaceId, usize)>,
}

impl TrajectoryProjector {
    pub fn accept(&mut self, event: &TimelineEvent) {
        match &event.kind {
            TimelineEventKind::Turn(TurnEvent::Started { id, .. }) => {
                self.active_turn = Some(id.0.to_string());
                self.active_step = None;
            }
            TimelineEventKind::Step(StepEvent::Started { id }) => {
                self.active_turn = Some(id.turn.0.to_string());
                self.active_step = Some(id.index);
            }
            _ => {}
        }
        match &event.kind {
            TimelineEventKind::Request(RequestEvent::Started { id, turn, step, .. }) => {
                self.request_scopes
                    .insert(id.clone(), (turn.0.to_string(), step.index));
            }
            TimelineEventKind::Tool(ToolEvent::Started {
                call_id,
                turn,
                step,
                name,
                ..
            }) => {
                self.tool_scopes.insert(
                    call_id.clone(),
                    (turn.0.to_string(), step.index, name.clone()),
                );
            }
            _ => {}
        }

        if let TimelineEventKind::Messages(MessageEvent {
            surface: SurfaceOp::Replace { shadowed, .. },
            ..
        }) = &event.kind
        {
            for id in shadowed {
                if let Some(&row_index) = self.surface_rows.get(id) {
                    let remaining = &mut self.current_items_per_row[row_index];
                    *remaining = remaining.saturating_sub(1);
                    if *remaining == 0 {
                        self.rows[row_index].visibility = SurfaceVisibility::Shadowed;
                        self.dirty_rows.insert(row_index);
                    }
                }
            }
        }

        let row_index = self.rows.len();
        let current_items = match &event.kind {
            TimelineEventKind::Messages(messages) => {
                for item in 0..messages.items.len() {
                    if let Ok(item) = u32::try_from(item) {
                        self.surface_rows.insert(
                            SurfaceId {
                                event: event.seq,
                                item,
                            },
                            row_index,
                        );
                    }
                }
                messages.items.len()
            }
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                input: Some(_), ..
            }) => {
                self.surface_rows.insert(
                    SurfaceId {
                        event: event.seq,
                        item: 0,
                    },
                    row_index,
                );
                1
            }
            TimelineEventKind::Input(InputEvent::Consumed { .. }) => {
                self.surface_rows.insert(
                    SurfaceId {
                        event: event.seq,
                        item: 0,
                    },
                    row_index,
                );
                1
            }
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                input: None, ..
            }) => 0,
            TimelineEventKind::Input(InputEvent::Handled { .. }) => 0,
            TimelineEventKind::Notification(NotificationEvent::Dismissed { .. }) => 0,
            TimelineEventKind::Control(ControlEvent { model_contexts, .. }) => {
                let mut current = 0;
                for (item, context) in model_contexts.iter().enumerate() {
                    if !crate::timeline::control_transition_waits_for_boundary(
                        self.active_turn.is_some(),
                        self.active_step.is_some(),
                        context,
                    ) {
                        self.surface_rows.insert(
                            SurfaceId {
                                event: event.seq,
                                item: item as u32,
                            },
                            row_index,
                        );
                        current += 1;
                    }
                }
                current
            }
            _ => 0,
        };
        self.current_items_per_row.push(current_items);
        let mut projected = row(
            event,
            if current_items == 0 {
                SurfaceVisibility::LogOnly
            } else {
                SurfaceVisibility::Current
            },
            &self.request_scopes,
            &self.tool_scopes,
        );
        if let TimelineEventKind::Control(control) = &event.kind {
            projected.summary = describe_control_event(self.control_snapshot.as_ref(), control);
            self.control_snapshot = Some(control.snapshot.clone());
        }
        if projected.turn_id.is_none() {
            projected.turn_id.clone_from(&self.active_turn);
        }
        if projected.step_index.is_none() {
            projected.step_index = self.active_step;
        }
        self.rows.push(projected);
        self.dirty_rows.insert(row_index);
        match &event.kind {
            TimelineEventKind::Control(ControlEvent { model_contexts, .. }) => {
                for (item, context) in model_contexts.iter().enumerate() {
                    if crate::timeline::control_transition_waits_for_boundary(
                        self.active_turn.is_some(),
                        self.active_step.is_some(),
                        context,
                    ) {
                        self.pending_control_rows.insert(
                            context.layer,
                            (
                                SurfaceId {
                                    event: event.seq,
                                    item: item as u32,
                                },
                                row_index,
                            ),
                        );
                    }
                }
            }
            TimelineEventKind::Turn(TurnEvent::Ended { .. }) => {
                for (_layer, (id, pending_row)) in std::mem::take(&mut self.pending_control_rows) {
                    self.surface_rows.insert(id, pending_row);
                    self.current_items_per_row[pending_row] += 1;
                    self.rows[pending_row].visibility = SurfaceVisibility::Current;
                    self.dirty_rows.insert(pending_row);
                }
                self.active_turn = None;
                self.active_step = None;
            }
            TimelineEventKind::Step(StepEvent::Ended { .. }) => {
                for layer in [
                    ControlContextLayer::AgentRole,
                    ControlContextLayer::GoalDefinition,
                ] {
                    if let Some((id, pending_row)) = self.pending_control_rows.remove(&layer) {
                        self.surface_rows.insert(id, pending_row);
                        self.current_items_per_row[pending_row] += 1;
                        self.rows[pending_row].visibility = SurfaceVisibility::Current;
                        self.dirty_rows.insert(pending_row);
                    }
                }
                self.active_step = None;
            }
            _ => {}
        }
    }

    pub fn rows(&self) -> &[TrajectoryRow] {
        &self.rows
    }

    /// Projector rows changed since the last materialized Trajectory refresh.
    /// New rows and the small set of earlier rows affected by Surface/control
    /// transitions share this one delta channel.
    pub fn dirty_row_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.dirty_rows.iter().copied()
    }

    pub fn clear_dirty_rows(&mut self) {
        self.dirty_rows.clear();
    }

    pub fn snapshot(&self, timeline: &Timeline) -> TrajectorySnapshot {
        let mut rows = self.rows.clone();
        for (row, event) in rows.iter_mut().zip(timeline.events()) {
            row.details = serde_json::to_value(&event.kind).unwrap_or(serde_json::Value::Null);
        }
        TrajectorySnapshot {
            schema_version: TRAJECTORY_SCHEMA_VERSION,
            event_count: timeline.events().len(),
            current_surface_items: timeline.surface_len(),
            active_turn: timeline.active_turn().map(|id| id.0.to_string()),
            active_step: timeline.active_step().map(|id| id.index),
            open_requests: timeline.open_request_ids().map(str::to_owned).collect(),
            open_tools: timeline.open_tool_call_ids().map(str::to_owned).collect(),
            open_workflows: timeline
                .open_workflow_run_ids()
                .map(str::to_owned)
                .collect(),
            rows,
        }
    }
}

fn row(
    event: &TimelineEvent,
    visibility: SurfaceVisibility,
    request_scopes: &BTreeMap<String, (String, u32)>,
    tool_scopes: &BTreeMap<String, (String, u32, String)>,
) -> TrajectoryRow {
    let (_, _, state, turn_id, step_index, correlation_id, duration_ms, summary) =
        describe(&event.kind, request_scopes, tool_scopes);
    let (layer, class, producer, kind) = dimensions(&event.kind, &state, tool_scopes);
    let outcome = event_outcome(&event.kind);
    let issue_severity = trajectory_issue_severity(&state, outcome.as_deref());
    TrajectoryRow {
        entry_id: format!("t:local/{}", event.seq.get()),
        seq: event.seq.get(),
        parent_entry_id: None,
        nesting_path: vec![event.seq.get()],
        at_ms: event.at_ms,
        layer,
        actor: actor(&event.kind),
        class,
        producer,
        kind,
        state,
        visibility,
        turn_id,
        step_index,
        correlation_id,
        duration_ms,
        outcome,
        issue_severity,
        summary,
        details: serde_json::Value::Null,
    }
}

fn dimensions(
    event: &TimelineEventKind,
    state: &str,
    tool_scopes: &BTreeMap<String, (String, u32, String)>,
) -> (String, String, String, String) {
    match event {
        TimelineEventKind::Messages(message) => message_dimensions(message, tool_scopes),
        TimelineEventKind::Input(_) => coordinates("meta", "governance", "user", "input", state),
        TimelineEventKind::Hook(_) => coordinates("meta", "governance", "hook", "hook", state),
        TimelineEventKind::Turn(_) => coordinates("meta", "lifecycle", "core", "turn", state),
        TimelineEventKind::Step(_) => coordinates("meta", "lifecycle", "core", "step", state),
        TimelineEventKind::Request(_) => {
            coordinates("meta", "lifecycle", "model", "request", state)
        }
        TimelineEventKind::Tool(tool) => {
            let producer = match tool {
                ToolEvent::Started { name, .. } | ToolEvent::Completed { name, .. } => {
                    format!("tool:{name}")
                }
            };
            let kind = match tool {
                ToolEvent::Started { .. } => "tool.call",
                ToolEvent::Completed { .. } => "tool.result",
            };
            (kind.into(), "message".into(), producer, kind.into())
        }
        TimelineEventKind::Workflow(_) => {
            coordinates("meta", "lifecycle", "core", "workflow", state)
        }
        TimelineEventKind::Compaction(_) => {
            coordinates("meta", "governance", "core", "compaction", state)
        }
        TimelineEventKind::ImageProjection(_) => coordinates(
            "meta",
            "governance",
            "core",
            "projection.image_shadow",
            state,
        ),
        TimelineEventKind::Recovery(_) => {
            coordinates("meta", "governance", "core", "context.recovery", state)
        }
        TimelineEventKind::Observation(observation)
            if observation.scope == "model" && observation.name == "changed" =>
        {
            coordinates("meta", "lifecycle", "core", "model", "changed")
        }
        TimelineEventKind::Observation(observation) => {
            let producer = producer_from_scope(&observation.scope);
            (
                "meta".into(),
                "audit".into(),
                producer,
                format!("{}.{}", observation.scope, observation.name),
            )
        }
        TimelineEventKind::Control(control) if control.model_contexts.len() > 1 => {
            coordinates(
                "system.control",
                "governance",
                "core",
                if control.model_contexts.iter().all(|context| {
                    context.activation == crate::ControlContextActivation::Reprojection
                }) {
                    "prompt.control.reprojected"
                } else {
                    "prompt.control.updated"
                },
                state,
            )
        }
        TimelineEventKind::Control(control) => match control.model_contexts.first() {
            Some(context) => {
                let (layer, kind) = match (context.layer, context.activation) {
                    (
                        ControlContextLayer::AgentRole,
                        crate::ControlContextActivation::Transition,
                    ) => ("system.role", "prompt.agent_role.updated"),
                    (
                        ControlContextLayer::GoalDefinition,
                        crate::ControlContextActivation::Transition,
                    ) => ("system.goal", "prompt.goal_definition.updated"),
                    (
                        ControlContextLayer::PlanPhase,
                        crate::ControlContextActivation::Transition,
                    ) => ("system.plan", "prompt.plan_phase.updated"),
                    (
                        ControlContextLayer::Behavior,
                        crate::ControlContextActivation::Transition,
                    ) => ("system.behavior", "prompt.behavior.updated"),
                    (
                        ControlContextLayer::AgentRole,
                        crate::ControlContextActivation::Reprojection,
                    ) => ("system.role", "prompt.agent_role.reprojected"),
                    (
                        ControlContextLayer::GoalDefinition,
                        crate::ControlContextActivation::Reprojection,
                    ) => ("system.goal", "prompt.goal_definition.reprojected"),
                    (
                        ControlContextLayer::PlanPhase,
                        crate::ControlContextActivation::Reprojection,
                    ) => ("system.plan", "prompt.plan_phase.reprojected"),
                    (
                        ControlContextLayer::Behavior,
                        crate::ControlContextActivation::Reprojection,
                    ) => ("system.behavior", "prompt.behavior.reprojected"),
                };
                (
                    layer.into(),
                    "governance".into(),
                    "core".into(),
                    kind.into(),
                )
            }
            None => coordinates("system.behavior", "lifecycle", "core", "control", state),
        },
        TimelineEventKind::SessionTitle(title) => coordinates(
            "meta",
            "lifecycle",
            match &title.source {
                SessionTitleSource::User => "user",
                SessionTitleSource::Generated { .. } | SessionTitleSource::Fallback { .. } => {
                    "sideband"
                }
            },
            "session.title",
            state,
        ),
        TimelineEventKind::Sideband(_) => {
            coordinates("meta", "auxiliary", "core", "sideband", "spawn")
        }
        TimelineEventKind::Subagent(_) => {
            coordinates("meta", "lifecycle", "core", "subagent", state)
        }
        TimelineEventKind::SubagentSeed(_) => {
            coordinates("meta", "lifecycle", "core", "subagent.seed", "linked")
        }
        TimelineEventKind::SubagentResult(_) => {
            coordinates("meta", "lifecycle", "core", "subagent.result", state)
        }
        TimelineEventKind::Notification(_) => {
            coordinates("meta", "governance", "core", "notification", state)
        }
    }
}

fn actor(event: &TimelineEventKind) -> String {
    match event {
        TimelineEventKind::Workflow(WorkflowEvent::Spawned { run_id, .. })
        | TimelineEventKind::Workflow(WorkflowEvent::Resumed { run_id, .. })
        | TimelineEventKind::Workflow(WorkflowEvent::Ended { run_id, .. })
        | TimelineEventKind::Workflow(WorkflowEvent::Closed { run_id, .. }) => {
            format!("workflow:{run_id}")
        }
        _ => "main".into(),
    }
}

fn coordinates(
    layer: &str,
    class: &str,
    producer: &str,
    kind: &str,
    state: &str,
) -> (String, String, String, String) {
    (
        layer.into(),
        class.into(),
        producer.into(),
        format!("{kind}.{state}"),
    )
}

fn message_dimensions(
    message: &MessageEvent,
    tool_scopes: &BTreeMap<String, (String, u32, String)>,
) -> (String, String, String, String) {
    let governance = matches!(
        message.cause,
        crate::MessageCause::IntegrityRepair
            | crate::MessageCause::Compaction
            | crate::MessageCause::ToolResultPrune
            | crate::MessageCause::ContextRebuild
            | crate::MessageCause::Rewind
    );
    if governance {
        let kind = match message.cause {
            crate::MessageCause::Compaction => "replacement.summary",
            crate::MessageCause::Rewind => "context.branch",
            crate::MessageCause::ToolResultPrune => "replacement.range_ref",
            crate::MessageCause::IntegrityRepair => "context.repair",
            crate::MessageCause::ContextRebuild => "context.rebuild",
            _ => unreachable!(),
        };
        return (
            "meta".into(),
            "governance".into(),
            "core".into(),
            kind.into(),
        );
    }

    let first = message.items.first();
    let (layer, producer, kind) = match first {
        Some(sampling_types::ConversationItem::System(_)) => {
            ("system.core", "core", "system.message")
        }
        Some(sampling_types::ConversationItem::User(user)) if user.synthetic_reason.is_some() => {
            ("user.synthetic", "core", "user.message")
        }
        Some(sampling_types::ConversationItem::User(_)) => ("user.direct", "user", "user.message"),
        Some(sampling_types::ConversationItem::Assistant(_))
        | Some(sampling_types::ConversationItem::Reasoning(_)) => {
            ("assistant", "model", "assistant.message")
        }
        Some(sampling_types::ConversationItem::ToolResult(result)) => {
            let producer = tool_scopes
                .get(&result.tool_call_id)
                .map(|(_, _, name)| format!("tool:{name}"))
                .unwrap_or_else(|| "tool:unknown".into());
            return (
                "tool.result".into(),
                "message".into(),
                producer,
                "tool.result".into(),
            );
        }
        Some(sampling_types::ConversationItem::BackendToolCall(_)) => {
            ("tool.call", "model", "tool.call")
        }
        None => ("meta", "core", "message.empty"),
    };
    (layer.into(), "message".into(), producer.into(), kind.into())
}

fn producer_from_scope(scope: &str) -> String {
    for prefix in ["hook", "plugin", "skill", "mcp", "tool"] {
        if scope == prefix || scope.starts_with(&format!("{prefix}:")) {
            return scope.to_owned();
        }
    }
    "core".into()
}

#[allow(clippy::type_complexity)]
fn describe(
    kind: &TimelineEventKind,
    request_scopes: &BTreeMap<String, (String, u32)>,
    tool_scopes: &BTreeMap<String, (String, u32, String)>,
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
        TimelineEventKind::Input(event) => match event {
            InputEvent::Submitted {
                input_id,
                intent,
                payload_ref,
            } => tuple(
                "input",
                "submitted",
                "pending",
                None,
                None,
                Some(input_id.clone()),
                None,
                format!("{intent:?} · {} bytes", payload_ref.bytes),
            ),
            InputEvent::AdmissionResolved {
                input_id,
                decision,
                route,
                supersedes,
            } => tuple(
                "input",
                "admission",
                match decision {
                    InputAdmissionDecision::Allow => "allowed",
                    InputAdmissionDecision::Block { .. } => "blocked",
                },
                None,
                None,
                Some(input_id.clone()),
                None,
                format!("{decision:?} · route={route:?} · supersedes={supersedes:?}"),
            ),
            InputEvent::Rerouted { input_ids, route } => tuple(
                "input",
                "rerouted",
                "projected",
                None,
                None,
                Some(input_ids.join(",")),
                None,
                format!("input projected to {route:?}"),
            ),
            InputEvent::Consumed {
                input_ids, turn, ..
            } => tuple(
                "input",
                "consumed",
                "consumed",
                Some(turn.0.to_string()),
                None,
                Some(input_ids.join(",")),
                None,
                "input consumed exactly once".into(),
            ),
            InputEvent::Handled { input_ids, turn } => tuple(
                "input",
                "handled",
                "consumed",
                Some(turn.0.to_string()),
                None,
                Some(input_ids.join(",")),
                None,
                "input completed without entering model context".into(),
            ),
            InputEvent::Dismissed { input_ids, reason } => tuple(
                "input",
                "dismissed",
                "dismissed",
                None,
                None,
                Some(input_ids.join(",")),
                None,
                format!("{reason:?}"),
            ),
        },
        TimelineEventKind::Hook(event) => match event {
            HookEvent::Triggered {
                occurrence_id,
                event,
                handlers,
                ..
            } => tuple(
                "hook",
                "triggered",
                "planned",
                None,
                None,
                Some(occurrence_id.clone()),
                None,
                format!("{event:?} · {} handler(s)", handlers.len()),
            ),
            HookEvent::RunStarted {
                occurrence_id,
                run_id,
                ..
            } => tuple(
                "hook",
                "run",
                "started",
                None,
                None,
                Some(run_id.clone()),
                None,
                occurrence_id.clone(),
            ),
            HookEvent::RunFinished {
                occurrence_id,
                run_id,
                outcome,
                ..
            } => tuple(
                "hook",
                "run",
                "finished",
                None,
                None,
                Some(run_id.clone()),
                None,
                format!("{occurrence_id} · {outcome:?}"),
            ),
            HookEvent::RunSkipped {
                occurrence_id,
                run_id,
                reason,
                ..
            } => tuple(
                "hook",
                "run",
                "skipped",
                None,
                None,
                Some(run_id.clone()),
                None,
                format!("{occurrence_id} · {reason:?}"),
            ),
            HookEvent::Completed {
                occurrence_id,
                decision,
            } => tuple(
                "hook",
                "completed",
                "completed",
                None,
                None,
                Some(occurrence_id.clone()),
                None,
                format!("{decision:?}"),
            ),
        },
        TimelineEventKind::Turn(event) => match event {
            TurnEvent::Started {
                id,
                identity,
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
                format!("{} · {model_id}", identity.origin),
            ),
            TurnEvent::Ended {
                id,
                outcome,
                duration_ms,
                ..
            } => tuple(
                "turn",
                "turn",
                outcome,
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
        TimelineEventKind::Workflow(event) => match event {
            WorkflowEvent::Spawned {
                run_id,
                execution_epoch,
                name,
                objective,
                ..
            } => tuple(
                "workflow",
                "spawn",
                "running",
                None,
                None,
                Some(run_id.clone()),
                None,
                format!(
                    "{name} · epoch {execution_epoch} · {}",
                    truncate(objective, 180)
                ),
            ),
            WorkflowEvent::Resumed {
                run_id,
                execution_epoch,
            } => tuple(
                "workflow",
                "resume",
                "running",
                None,
                None,
                Some(run_id.clone()),
                None,
                format!("execution epoch {execution_epoch}"),
            ),
            WorkflowEvent::Ended {
                run_id,
                execution_epoch,
                status,
                handoff,
                duration_ms,
                message,
            } => tuple(
                "workflow",
                "end",
                status.as_str(),
                None,
                None,
                Some(run_id.clone()),
                Some(*duration_ms),
                message.clone().unwrap_or_else(|| {
                    format!(
                        "execution epoch {execution_epoch} {} ({handoff:?})",
                        status.as_str()
                    )
                }),
            ),
            WorkflowEvent::Closed {
                run_id,
                execution_epoch,
                status,
                handoff,
                duration_ms,
                message,
            } => tuple(
                "workflow",
                "close",
                status.as_str(),
                None,
                None,
                Some(run_id.clone()),
                Some(*duration_ms),
                message.clone().unwrap_or_else(|| {
                    format!(
                        "run closed after epoch {execution_epoch}: {} ({handoff:?})",
                        status.as_str(),
                    )
                }),
            ),
        },
        TimelineEventKind::Compaction(event) => describe_compaction(event),
        TimelineEventKind::ImageProjection(projection) => tuple(
            "projection",
            "image_shadow",
            "installed",
            None,
            None,
            Some(projection.trigger_runtime.model().to_owned()),
            None,
            format!(
                "{} image group(s) projected for {}",
                projection.shadows.len(),
                projection.trigger_runtime.model()
            ),
        ),
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
            data,
        }) if scope == "model" && name == "changed" => tuple(
            "model",
            "change",
            "changed",
            turn.map(|id| id.0.to_string()),
            step.map(|id| id.index),
            data.as_ref()
                .and_then(|value| value.get("to_model_id"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            None,
            describe_model_change(data.as_ref()),
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
        TimelineEventKind::Control(ControlEvent { revision, .. }) => tuple(
            "governance",
            "control",
            "committed",
            None,
            None,
            Some(revision.to_string()),
            None,
            format!("control revision {revision}"),
        ),
        TimelineEventKind::SessionTitle(SessionTitleEvent { title, source }) => tuple(
            "session",
            "title",
            match source {
                SessionTitleSource::User => "user",
                SessionTitleSource::Generated { .. } => "generated",
                SessionTitleSource::Fallback { .. } => "fallback",
            },
            None,
            None,
            match source {
                SessionTitleSource::User => None,
                SessionTitleSource::Generated { sideband_id, .. }
                | SessionTitleSource::Fallback { sideband_id, .. } => Some(sideband_id.clone()),
            },
            None,
            title.clone(),
        ),
        TimelineEventKind::Sideband(SidebandSpawnEvent {
            sideband_id,
            purpose,
            ..
        }) => tuple(
            "sideband",
            purpose.as_str(),
            "spawn",
            None,
            None,
            Some(sideband_id.clone()),
            None,
            purpose.as_str().into(),
        ),
        TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) => tuple(
            "subagent",
            "spawn",
            "running",
            None,
            None,
            Some(spawn.subagent_id.clone()),
            None,
            format!("{} · {}", spawn.subagent_type, spawn.description),
        ),
        TimelineEventKind::Subagent(SubagentEvent::Ended(end)) => tuple(
            "subagent",
            "end",
            match end.outcome {
                crate::SubagentOutcome::Completed => "completed",
                crate::SubagentOutcome::Failed => "failed",
                crate::SubagentOutcome::Cancelled => "cancelled",
            },
            None,
            None,
            Some(end.subagent_id.clone()),
            Some(end.duration_ms),
            end.error
                .clone()
                .unwrap_or_else(|| "subagent completed".into()),
        ),
        TimelineEventKind::SubagentSeed(SubagentSeedEvent {
            parent_timeline_id,
            parent_spawn_seq,
            subagent_id,
            ..
        }) => tuple(
            "subagent",
            "seed-source",
            "linked",
            None,
            None,
            Some(subagent_id.clone()),
            None,
            format!("t:{parent_timeline_id}/{parent_spawn_seq}"),
        ),
        TimelineEventKind::SubagentResult(SubagentResultEvent {
            subagent_id,
            outcome,
            duration_ms,
            error,
            ..
        }) => tuple(
            "subagent",
            "result",
            match outcome {
                crate::SubagentOutcome::Completed => "completed",
                crate::SubagentOutcome::Failed => "failed",
                crate::SubagentOutcome::Cancelled => "cancelled",
            },
            None,
            None,
            Some(subagent_id.clone()),
            Some(*duration_ms),
            error.clone().unwrap_or_else(|| "subagent completed".into()),
        ),
        TimelineEventKind::Notification(NotificationEvent::Received {
            id,
            source,
            payload_ref,
            ..
        }) => tuple(
            "notification",
            "received",
            "pending",
            None,
            None,
            Some(id.clone()),
            None,
            format!(
                "{} · {} bytes",
                notification_source_label(source),
                payload_ref.bytes
            ),
        ),
        TimelineEventKind::Notification(NotificationEvent::Consumed {
            notification_ids,
            turn,
            ..
        }) => tuple(
            "notification",
            "consumed",
            "admitted",
            Some(turn.0.to_string()),
            None,
            notification_ids.first().cloned(),
            None,
            format!("{} notification(s)", notification_ids.len()),
        ),
        TimelineEventKind::Notification(NotificationEvent::Dismissed {
            notification_ids,
            reason,
        }) => tuple(
            "notification",
            "dismissed",
            "resolved",
            None,
            None,
            notification_ids.first().cloned(),
            None,
            format!("{} notification(s) · {reason:?}", notification_ids.len()),
        ),
    }
}

fn notification_source_label(source: &NotificationSource) -> &'static str {
    match source {
        NotificationSource::MonitorProgress { .. } => "monitor progress",
        NotificationSource::TaskStillRunning { .. } => "task still running",
        NotificationSource::TaskCompleted { .. } => "task completed",
        NotificationSource::SubagentCompleted { .. } => "subagent completed",
        NotificationSource::PlanHandoff { .. } => "plan handoff",
        NotificationSource::WorkflowHandoff { .. } => "workflow handoff",
    }
}

fn describe_model_change(data: Option<&serde_json::Value>) -> String {
    let summary = describe_model_transition(data);
    match data
        .and_then(|value| value.get("reason"))
        .and_then(serde_json::Value::as_str)
    {
        Some("catalog_reload") => format!("{summary} · catalog reload"),
        _ => summary,
    }
}

fn describe_model_transition(data: Option<&serde_json::Value>) -> String {
    let string = |field: &str| {
        data.and_then(|value| value.get(field))
            .and_then(serde_json::Value::as_str)
    };
    let (Some(from_model), Some(to_model)) = (string("from_model_id"), string("to_model_id"))
    else {
        return "model changed".into();
    };
    if from_model != to_model {
        return format!("{from_model} → {to_model}");
    }
    let effort = |field: &str| {
        data.and_then(|value| value.get(field)).and_then(|value| {
            if value.is_null() {
                Some("default")
            } else {
                value.as_str()
            }
        })
    };
    let from_effort = effort("from_reasoning_effort");
    let to_effort = effort("to_reasoning_effort");
    if from_effort != to_effort {
        return format!(
            "{from_model}: {} → {}",
            from_effort.unwrap_or("?"),
            to_effort.unwrap_or("?")
        );
    }
    let from_provider = string("from_provider_model");
    let to_provider = string("to_provider_model");
    if from_provider != to_provider {
        return format!(
            "{from_model} route: {} → {}",
            from_provider.unwrap_or("?"),
            to_provider.unwrap_or("?")
        );
    }
    if data.and_then(|value| value.get("from_model_transport"))
        != data.and_then(|value| value.get("to_model_transport"))
    {
        return format!("{from_model} transport changed");
    }
    format!("model {to_model} selected")
}

fn describe_control_event(previous: Option<&serde_json::Value>, control: &ControlEvent) -> String {
    if control.model_contexts.len() > 1
        && control
            .model_contexts
            .iter()
            .all(|context| context.activation == crate::ControlContextActivation::Reprojection)
    {
        return "Control prompt contexts reassembled".into();
    }
    if let Some(context) = control.model_contexts.first()
        && context.activation == crate::ControlContextActivation::Reprojection
    {
        return match context.layer {
            ControlContextLayer::AgentRole => "Agent role prompt context reassembled".into(),
            ControlContextLayer::GoalDefinition => {
                "Goal definition prompt context reassembled".into()
            }
            ControlContextLayer::PlanPhase => "Plan phase prompt context reassembled".into(),
            ControlContextLayer::Behavior => "Behavior prompt context reassembled".into(),
        };
    }
    describe_control_transition(previous, &control.snapshot)
}

fn describe_control_transition(
    previous: Option<&serde_json::Value>,
    current: &serde_json::Value,
) -> String {
    let mut changes = Vec::new();
    let previous_agent = previous.and_then(|snapshot| json_string(snapshot, "agent_name"));
    let current_agent = json_string(current, "agent_name");
    if previous_agent != current_agent {
        changes.push(match (previous_agent, current_agent) {
            (None, Some(current)) => format!("Agent {current} selected"),
            (Some(previous), Some(current)) => format!("Agent {previous} → {current}"),
            (Some(previous), None) => format!("Agent {previous} cleared"),
            (None, None) => "Agent changed".into(),
        });
    }
    let previous_behavior = previous.and_then(control_behavior);
    let current_behavior = control_behavior(current);
    if previous_behavior != current_behavior {
        changes.push(match (previous_behavior, current_behavior.as_deref()) {
            (None, Some(current)) => format!("behavior {current} selected"),
            (Some(previous), Some(current)) => format!("behavior {previous} → {current}"),
            (Some(previous), None) => format!("behavior {previous} cleared"),
            (None, None) => "behavior changed".into(),
        });
    }

    let previous_goal = previous.and_then(|snapshot| snapshot.get("goal"));
    let current_goal = current.get("goal");
    match (
        previous_goal.filter(|goal| !goal.is_null()),
        current_goal.filter(|goal| !goal.is_null()),
    ) {
        (None, Some(goal)) => {
            let id = json_string(goal, "goal_id").unwrap_or("?");
            let status = json_string(goal, "status").unwrap_or("unknown");
            changes.push(format!("goal {id} created · {status}"));
        }
        (Some(goal), None) => {
            changes.push(format!(
                "goal {} cleared",
                json_string(goal, "goal_id").unwrap_or("?")
            ));
        }
        (Some(previous_goal), Some(current_goal)) => {
            let previous_id = json_string(previous_goal, "goal_id").unwrap_or("?");
            let current_id = json_string(current_goal, "goal_id").unwrap_or("?");
            if previous_id != current_id {
                changes.push(format!("goal {previous_id} → {current_id}"));
            } else {
                let previous_status = json_string(previous_goal, "status");
                let current_status = json_string(current_goal, "status");
                if previous_status != current_status {
                    changes.push(format!(
                        "goal {current_id}: {} → {}",
                        previous_status.unwrap_or("unknown"),
                        current_status.unwrap_or("unknown")
                    ));
                }
                if previous_goal.get("objective") != current_goal.get("objective") {
                    changes.push(format!("goal {current_id} objective revised"));
                }
                if previous_goal.get("token_budget") != current_goal.get("token_budget") {
                    changes.push(format!("goal {current_id} budget updated"));
                }
                if changes.is_empty() && goal_tokens(previous_goal) != goal_tokens(current_goal) {
                    changes.push(format!(
                        "goal {current_id} checkpoint · {} tokens",
                        goal_tokens(current_goal)
                    ));
                }
            }
        }
        (None, None) => {}
    }

    if changes.is_empty() {
        match current_behavior {
            Some(behavior) => format!("{behavior} control checkpoint"),
            None => "control checkpoint".into(),
        }
    } else {
        changes.join("; ")
    }
}

fn control_behavior(snapshot: &serde_json::Value) -> Option<String> {
    let state = snapshot.get("behavior")?.get("state")?;
    match state {
        serde_json::Value::String(state) => Some(match state.as_str() {
            "Normal" => "normal".into(),
            "Clarify" => "clarify".into(),
            "Workflow" => "workflow".into(),
            "Goal" => "goal".into(),
            other => other.to_lowercase(),
        }),
        serde_json::Value::Object(state) if state.len() == 1 => {
            let (kind, value) = state.iter().next()?;
            match kind.as_str() {
                "Plan" => Some(format!(
                    "plan/{}",
                    value.as_str().unwrap_or("unknown").to_lowercase()
                )),
                other => Some(other.to_lowercase()),
            }
        }
        _ => None,
    }
}

fn json_string<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(serde_json::Value::as_str)
}

fn goal_tokens(goal: &serde_json::Value) -> i64 {
    goal.get("tokens_used")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
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
    let correlation_id = event.items.iter().find_map(|item| match item {
        sampling_types::ConversationItem::ToolResult(result) => Some(result.tool_call_id.clone()),
        sampling_types::ConversationItem::BackendToolCall(call) => Some(call.id().to_owned()),
        _ => None,
    });
    tuple(
        "message",
        &format!("{:?}", event.cause).to_lowercase(),
        state,
        None,
        None,
        correlation_id,
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
            input_message_count,
            tool_count,
        } => tuple(
            "request",
            "model_request",
            "started",
            Some(turn.0.to_string()),
            Some(step.index),
            Some(id.clone()),
            None,
            format!("{model_id} · {input_message_count} messages · {tool_count} tools"),
        ),
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
            usage,
            response_message_count,
        } => {
            let (turn, step) = scope(scopes, id);
            let mut metrics = Vec::new();
            if let Some(ttft) = time_to_first_token_ms {
                metrics.push(format!("TTFT {ttft} ms"));
            }
            if let Some(tokens) = usage.input_tokens {
                metrics.push(format!("input {tokens} tok"));
            }
            if let Some(tokens) = usage.output_tokens {
                metrics.push(format!("output {tokens} tok"));
            }
            metrics.push(format!("{response_message_count} responses"));
            tuple(
                "request",
                "model_request",
                "completed",
                turn,
                step,
                Some(id.clone()),
                Some(*duration_ms),
                metrics.join(" · "),
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

fn describe_tool(
    event: &ToolEvent,
    scopes: &BTreeMap<String, (String, u32, String)>,
) -> ReturnTuple {
    match event {
        ToolEvent::Started {
            call_id,
            turn,
            step,
            name,
            input,
        } => tuple(
            "tool",
            name,
            "started",
            Some(turn.0.to_string()),
            Some(step.index),
            Some(call_id.clone()),
            None,
            summarize_tool_payload(input.as_ref()).unwrap_or_else(|| "call admitted".into()),
        ),
        ToolEvent::Completed {
            call_id,
            name,
            outcome,
            duration_ms,
            details,
        } => {
            let (turn, step) = tool_scope(scopes, call_id);
            let summary = summarize_tool_payload(details.as_ref())
                .map(|details| format!("{outcome} · {details}"))
                .unwrap_or_else(|| outcome.clone());
            tuple(
                "tool",
                name,
                "completed",
                turn,
                step,
                Some(call_id.clone()),
                Some(*duration_ms),
                summary,
            )
        }
    }
}

fn summarize_tool_payload(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    let rendered = match value {
        serde_json::Value::Object(fields) => {
            const PREFERRED: [&str; 11] = [
                "path",
                "file_path",
                "command",
                "cmd",
                "query",
                "url",
                "pattern",
                "name",
                "stage",
                "reason",
                "description",
            ];
            let parts = PREFERRED
                .into_iter()
                .filter_map(|key| {
                    fields
                        .get(key)
                        .map(|value| format!("{key}={}", summarize_json_value(value)))
                })
                .take(2)
                .collect::<Vec<_>>();
            if parts.is_empty() {
                format!("{} input fields", fields.len())
            } else {
                parts.join(" · ")
            }
        }
        other => summarize_json_value(other),
    };
    (!rendered.is_empty()).then(|| truncate(&rendered, 180))
}

fn summarize_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => truncate(value, 120),
        serde_json::Value::Null => "null".into(),
        other => truncate(&other.to_string(), 120),
    }
}

fn scope(scopes: &BTreeMap<String, (String, u32)>, id: &str) -> (Option<String>, Option<u32>) {
    scopes
        .get(id)
        .map(|(turn, step)| (Some(turn.clone()), Some(*step)))
        .unwrap_or((None, None))
}

fn tool_scope(
    scopes: &BTreeMap<String, (String, u32, String)>,
    id: &str,
) -> (Option<String>, Option<u32>) {
    scopes
        .get(id)
        .map(|(turn, step, _)| (Some(turn.clone()), Some(*step)))
        .unwrap_or((None, None))
}

fn event_outcome(event: &TimelineEventKind) -> Option<String> {
    match event {
        TimelineEventKind::Turn(TurnEvent::Ended { outcome, .. })
        | TimelineEventKind::Step(StepEvent::Ended { outcome, .. }) => Some(outcome.clone()),
        TimelineEventKind::Request(RequestEvent::Retrying { .. }) => Some("retrying".into()),
        TimelineEventKind::Request(RequestEvent::Completed { .. }) => Some("completed".into()),
        TimelineEventKind::Request(RequestEvent::Failed { .. }) => Some("failed".into()),
        TimelineEventKind::Request(RequestEvent::Cancelled { .. }) => Some("cancelled".into()),
        TimelineEventKind::Tool(ToolEvent::Completed { outcome, .. }) => Some(outcome.clone()),
        TimelineEventKind::Workflow(WorkflowEvent::Ended { status, .. })
        | TimelineEventKind::Workflow(WorkflowEvent::Closed { status, .. }) => {
            Some(status.as_str().into())
        }
        TimelineEventKind::Subagent(SubagentEvent::Ended(end)) => Some(
            match end.outcome {
                crate::SubagentOutcome::Completed => "completed",
                crate::SubagentOutcome::Failed => "failed",
                crate::SubagentOutcome::Cancelled => "cancelled",
            }
            .into(),
        ),
        TimelineEventKind::SubagentResult(result) => Some(
            match result.outcome {
                crate::SubagentOutcome::Completed => "completed",
                crate::SubagentOutcome::Failed => "failed",
                crate::SubagentOutcome::Cancelled => "cancelled",
            }
            .into(),
        ),
        _ => None,
    }
}

pub fn trajectory_issue_severity(
    state: &str,
    outcome: Option<&str>,
) -> Option<TrajectoryIssueSeverity> {
    let state = state.to_ascii_lowercase();
    let outcome = outcome.unwrap_or_default().to_ascii_lowercase();
    if [
        "failed",
        "error",
        "invalid_tool",
        "hook_denied",
        "not_dispatched",
        "outcome_unknown",
        "rejected",
    ]
    .iter()
    .any(|value| state == *value || outcome == *value)
    {
        return Some(TrajectoryIssueSeverity::Error);
    }
    if [
        "cancelled",
        "interrupted",
        "retrying",
        "permission_rejected",
        "permission_cancelled",
        "permission_timed_out",
    ]
    .iter()
    .any(|value| state == *value || outcome == *value)
    {
        return Some(TrajectoryIssueSeverity::Warning);
    }
    None
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
        CompactionEvent::Summary {
            id,
            source_tokens,
            summary_chars,
            result_ref,
            target,
            ..
        } => tuple(
            "compaction",
            "compaction",
            "summary",
            None,
            None,
            Some(id.clone()),
            None,
            format!(
                "{} shadowed items [{}..{}] · {source_tokens} source tokens → {summary_chars} chars · {}:{}",
                target.shadowed.len(),
                surface_id_label(target.start),
                surface_id_label(target.end),
                result_ref.timeline_id,
                result_ref.first_seq
            ),
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

fn surface_id_label(id: SurfaceId) -> String {
    format!("e{}:i{}", id.event.get(), id.item)
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
                MessageCause::ContextRebuild,
            )
            .unwrap();
        let snapshot = timeline.trajectory();
        assert_eq!(snapshot.schema_version, TRAJECTORY_SCHEMA_VERSION);
        assert_eq!(snapshot.rows[0].nesting_path, [0]);
        assert!(snapshot.rows[0].parent_entry_id.is_none());
        assert_eq!(snapshot.rows[0].visibility, SurfaceVisibility::Shadowed);
        assert_eq!(snapshot.rows[1].visibility, SurfaceVisibility::Current);
    }

    #[test]
    fn incremental_projector_retains_only_summary_rows() {
        let timeline = Timeline::from_seed(vec![ConversationItem::user("payload")]).unwrap();
        let mut projector = TrajectoryProjector::default();
        projector.accept(&timeline.events()[0]);

        assert!(projector.rows()[0].details.is_null());
        assert!(!projector.snapshot(&timeline).rows[0].details.is_null());
    }

    #[test]
    fn hook_rows_are_an_exact_projection_of_timeline_hook_facts() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Observation(ObservationEvent {
                scope: "test".into(),
                name: "before_hook".into(),
                turn: None,
                step: None,
                data: None,
            }))
            .unwrap();
        let handler = crate::HookHandlerPlan {
            index: 0,
            run_id: "trajectory-run".into(),
            name: "trajectory-handler".into(),
            provenance: crate::HookHandlerProvenance::ProjectFile,
            kind: crate::HookHandlerKind::Command,
            failure_policy: crate::HookFailurePolicy::Allow,
            action: crate::HookHandlerPlanAction::Execute,
        };
        timeline
            .record(TimelineEventKind::Hook(HookEvent::Triggered {
                occurrence_id: "trajectory-hook".into(),
                event: crate::HookEventType::Notification,
                gate: crate::HookGateKind::Observe,
                cause: crate::HookCause::Notification {
                    notification_id: "trajectory-hook".into(),
                },
                config_generation: 9,
                handlers: vec![handler],
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Hook(HookEvent::RunStarted {
                occurrence_id: "trajectory-hook".into(),
                run_id: "trajectory-run".into(),
                handler_index: 0,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Hook(HookEvent::RunFinished {
                occurrence_id: "trajectory-hook".into(),
                run_id: "trajectory-run".into(),
                handler_index: 0,
                elapsed_ms: 7,
                outcome: crate::HookRunOutcome::Success,
                control: crate::HookRunControl::None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Hook(HookEvent::Completed {
                occurrence_id: "trajectory-hook".into(),
                decision: crate::HookAggregateDecision::Observe,
            }))
            .unwrap();

        let snapshot = timeline.trajectory();
        assert_eq!(snapshot.rows.len(), timeline.events().len());
        for (event, row) in timeline.events().iter().zip(&snapshot.rows) {
            let is_hook_fact = matches!(&event.kind, TimelineEventKind::Hook(_));
            assert_eq!(row.producer == "hook", is_hook_fact);
            if is_hook_fact {
                assert_eq!(row.layer, "meta");
                assert_eq!(row.class, "governance");
                assert_eq!(
                    row.details,
                    serde_json::to_value(&event.kind).expect("Hook fact serializes")
                );
            }
        }
        assert_eq!(
            snapshot
                .rows
                .iter()
                .filter(|row| row.producer == "hook")
                .map(|row| row.state.as_str())
                .collect::<Vec<_>>(),
            ["planned", "started", "finished", "completed"]
        );
    }

    #[test]
    fn projector_stamps_every_in_turn_row_with_stable_scope() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("task")]).unwrap();
        let turn = crate::TurnId(42);
        let step = crate::StepId { turn, index: 3 };
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                input_ids: Vec::new(),
                identity: crate::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 1,
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: crate::TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Messages(MessageEvent {
                cause: crate::MessageCause::Assistant,
                items: vec![ConversationItem::assistant("answer")],
                surface: SurfaceOp::Append,
            }))
            .unwrap();

        let row = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(row.kind, "assistant.message");
        assert_eq!(row.turn_id.as_deref(), Some("42"));
        assert_eq!(row.step_index, Some(3));
    }

    #[test]
    fn projection_exposes_control_commits_as_governance_rows() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 3,
                snapshot: serde_json::json!({ "behavior": "plan" }),
                retired_context_layers: vec![],
                model_contexts: vec![],
            }))
            .unwrap();

        let mut snapshot = timeline.trajectory();
        let row = snapshot.rows.pop().unwrap();
        assert_eq!(row.layer, "system.behavior");
        assert_eq!(row.class, "lifecycle");
        assert_eq!(row.producer, "core");
        assert_eq!(row.kind, "control.committed");
        assert_eq!(row.state, "committed");
        assert_eq!(row.correlation_id.as_deref(), Some("3"));
    }

    #[test]
    fn projection_preserves_the_typed_agent_role_dimension() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "agent_name": "reviewer" }),
                retired_context_layers: vec![],
                model_contexts: vec![crate::ControlContext {
                    layer: crate::ControlContextLayer::AgentRole,
                    activation: crate::ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("role"),
                }],
            }))
            .unwrap();

        let row = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(row.layer, "system.role");
        assert_eq!(row.class, "governance");
        assert_eq!(row.producer, "core");
        assert_eq!(row.kind, "prompt.agent_role.updated");
        assert_eq!(row.visibility, SurfaceVisibility::Current);
        assert_eq!(row.summary, "Agent reviewer selected");
    }

    #[test]
    fn projection_exposes_prompt_context_reprojection_explicitly() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "behavior": "plan" }),
                retired_context_layers: vec![],
                model_contexts: vec![crate::ControlContext {
                    layer: crate::ControlContextLayer::Behavior,
                    activation: crate::ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("plan"),
                }],
            }))
            .unwrap();
        timeline
            .replace_all(
                vec![ConversationItem::system("system")],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({ "behavior": "plan" }),
                retired_context_layers: vec![],
                model_contexts: vec![crate::ControlContext {
                    layer: crate::ControlContextLayer::Behavior,
                    activation: crate::ControlContextActivation::Reprojection,
                    item: ConversationItem::system_reminder("plan"),
                }],
            }))
            .unwrap();

        let row = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(row.layer, "system.behavior");
        assert_eq!(row.class, "governance");
        assert_eq!(row.kind, "prompt.behavior.reprojected");
        assert_eq!(row.summary, "Behavior prompt context reassembled");
    }

    #[test]
    fn projection_activates_only_the_latest_in_turn_control_context() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        let turn = crate::TurnId(7);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                input_ids: Vec::new(),
                identity: crate::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 2,
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: crate::TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        for (revision, behavior) in [(1, "plan"), (2, "normal")] {
            timeline
                .record(TimelineEventKind::Control(crate::ControlEvent {
                    revision,
                    snapshot: serde_json::json!({ "behavior": behavior }),
                    retired_context_layers: vec![],
                    model_contexts: vec![crate::ControlContext {
                        layer: crate::ControlContextLayer::Behavior,
                        activation: crate::ControlContextActivation::Transition,
                        item: ConversationItem::system_reminder(behavior),
                    }],
                }))
                .unwrap();
        }
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: crate::TurnTerminal {
                    stop_reason: "end_turn".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .unwrap();

        let snapshot = timeline.trajectory();
        let first = snapshot
            .rows
            .iter()
            .find(|row| row.correlation_id.as_deref() == Some("1"))
            .unwrap();
        let latest = snapshot
            .rows
            .iter()
            .find(|row| row.correlation_id.as_deref() == Some("2"))
            .unwrap();
        assert_eq!(first.visibility, SurfaceVisibility::LogOnly);
        assert_eq!(latest.visibility, SurfaceVisibility::Current);
    }

    #[test]
    fn projection_activates_atomic_goal_context_at_step_boundary() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        let turn = crate::TurnId(8);
        let step = crate::StepId { turn, index: 0 };
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                input_ids: Vec::new(),
                identity: crate::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 2,
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: crate::TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "control_boundary".into(),
                duration_ms: 1,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({
                    "behavior": { "state": "Goal" },
                    "goal": { "goal_id": "goal-1", "definition_revision": 2 }
                }),
                retired_context_layers: vec![],
                model_contexts: vec![
                    crate::ControlContext {
                        layer: crate::ControlContextLayer::Behavior,
                        activation: crate::ControlContextActivation::Transition,
                        item: ConversationItem::system_reminder("goal behavior"),
                    },
                    crate::ControlContext {
                        layer: crate::ControlContextLayer::GoalDefinition,
                        activation: crate::ControlContextActivation::Transition,
                        item: ConversationItem::goal_directive(
                            "goal revision 2",
                            sampling_types::SyntheticReason::SystemReminder,
                            sampling_types::GoalDirectiveTag {
                                goal_id: "goal-1".into(),
                                definition_revision: 2,
                            },
                        ),
                    },
                ],
            }))
            .unwrap();

        let snapshot = timeline.trajectory();
        let control = snapshot
            .rows
            .iter()
            .find(|row| row.correlation_id.as_deref() == Some("1"))
            .unwrap();
        assert_eq!(control.layer, "system.control");
        assert_eq!(control.kind, "prompt.control.updated.committed");
        assert_eq!(control.visibility, SurfaceVisibility::Current);
        assert_eq!(snapshot.current_surface_items, timeline.surface().len());
        assert!(
            timeline
                .surface()
                .iter()
                .any(|item| item.text_content() == "goal revision 2")
        );
        assert!(
            !timeline
                .surface()
                .iter()
                .any(|item| item.text_content() == "goal behavior")
        );

        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: crate::TurnTerminal {
                    stop_reason: "end_turn".into(),
                    completion_kind: "control_boundary".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        let snapshot = timeline.trajectory();
        assert_eq!(snapshot.current_surface_items, timeline.surface().len());
        assert!(
            timeline
                .surface()
                .iter()
                .any(|item| item.text_content() == "goal behavior")
        );
    }

    #[test]
    fn projection_exposes_model_changes_as_lifecycle_rows() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Observation(ObservationEvent {
                scope: "model".into(),
                name: "changed".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!({
                    "from_model_id": "provider/old",
                    "to_model_id": "provider/new",
                    "from_reasoning_effort": "medium",
                    "to_reasoning_effort": "high",
                    "from_provider_model": "old-wire",
                    "to_provider_model": "new-wire",
                    "from_model_transport": {
                        "model": "old-wire",
                        "api_backend": "responses",
                        "endpoint_fingerprint": "old-endpoint"
                    },
                    "to_model_transport": {
                        "model": "new-wire",
                        "api_backend": "responses",
                        "endpoint_fingerprint": "new-endpoint"
                    },
                    "reason": "user_selection",
                })),
            }))
            .unwrap();

        let row = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(row.layer, "meta");
        assert_eq!(row.class, "lifecycle");
        assert_eq!(row.producer, "core");
        assert_eq!(row.kind, "model.changed");
        assert_eq!(row.state, "changed");
        assert_eq!(row.correlation_id.as_deref(), Some("provider/new"));
        assert_eq!(row.summary, "provider/old → provider/new");
    }

    #[test]
    fn projection_summarizes_behavior_and_goal_transitions() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({
                    "behavior": { "state": "Normal" },
                    "goal": null,
                }),
                retired_context_layers: vec![],
                model_contexts: vec![],
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({
                    "behavior": { "state": "Goal" },
                    "goal": {
                        "goal_id": "goal-1",
                        "status": "active",
                        "objective": "rebuild",
                        "token_budget": null,
                        "tokens_used": 0,
                    },
                }),
                retired_context_layers: vec![],
                model_contexts: vec![],
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(crate::ControlEvent {
                revision: 3,
                snapshot: serde_json::json!({
                    "behavior": { "state": "Goal" },
                    "goal": {
                        "goal_id": "goal-1",
                        "status": "active",
                        "objective": "rebuild",
                        "token_budget": null,
                        "tokens_used": 380,
                    },
                }),
                retired_context_layers: vec![],
                model_contexts: vec![],
            }))
            .unwrap();

        let rows = timeline.trajectory().rows;
        assert_eq!(rows[0].summary, "behavior normal selected");
        assert_eq!(
            rows[1].summary,
            "behavior normal → goal; goal goal-1 created · active"
        );
        assert_eq!(rows[2].summary, "goal goal-1 checkpoint · 380 tokens");
    }

    #[test]
    fn projection_carries_request_scope_to_terminal_rows() {
        let mut timeline = Timeline::default();
        let turn = crate::TurnId(9);
        let step = crate::StepId { turn, index: 2 };
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                input_ids: Vec::new(),
                identity: crate::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: crate::TurnInputKind::Prompt,
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
                usage: crate::RequestUsage {
                    input_tokens: Some(120),
                    output_tokens: Some(24),
                    ..Default::default()
                },
                response_message_count: 1,
            }))
            .unwrap();

        let terminal = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(terminal.turn_id.as_deref(), Some("9"));
        assert_eq!(terminal.step_index, Some(2));
        assert_eq!(
            terminal.summary,
            "TTFT 2 ms · input 120 tok · output 24 tok · 1 responses"
        );
    }

    #[test]
    fn tool_calls_and_results_expose_the_same_pairing_identity() {
        let turn = crate::TurnId(4);
        let step = crate::StepId { turn, index: 1 };
        let call = ToolEvent::Started {
            call_id: "call-pair".into(),
            turn,
            step,
            name: "read_file".into(),
            input: None,
        };
        let result = ToolEvent::Completed {
            call_id: "call-pair".into(),
            name: "read_file".into(),
            outcome: "completed".into(),
            duration_ms: 3,
            details: None,
        };

        assert_eq!(
            describe_tool(&call, &BTreeMap::new()).5.as_deref(),
            Some("call-pair")
        );
        assert_eq!(
            describe_tool(&result, &BTreeMap::new()).5.as_deref(),
            Some("call-pair")
        );

        let message = MessageEvent {
            cause: MessageCause::ToolResult,
            items: vec![ConversationItem::tool_result("call-pair", "done")],
            surface: SurfaceOp::Append,
        };
        assert_eq!(describe_message(&message).5.as_deref(), Some("call-pair"));
    }

    #[test]
    fn tool_rows_preserve_name_input_pairing_and_diagnostic_outcome() {
        let turn = crate::TurnId(4);
        let step = crate::StepId { turn, index: 1 };
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                input_ids: Vec::new(),
                identity: crate::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "read".into(),
                input_kind: crate::TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Messages(MessageEvent {
                cause: MessageCause::Assistant,
                items: vec![ConversationItem::assistant_tool_calls(vec![
                    sampling_types::ToolCall {
                        id: std::sync::Arc::from("call-pair"),
                        name: "read_file".into(),
                        arguments: std::sync::Arc::from(r#"{"path":"/tmp/input.txt"}"#),
                    },
                ])],
                surface: SurfaceOp::Append,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Tool(ToolEvent::Started {
                call_id: "call-pair".into(),
                turn,
                step,
                name: "read_file".into(),
                input: Some(serde_json::json!({ "path": "/tmp/input.txt" })),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Tool(ToolEvent::Completed {
                call_id: "call-pair".into(),
                name: "read_file".into(),
                outcome: "not_dispatched".into(),
                duration_ms: 3,
                details: Some(serde_json::json!({ "reason": "policy" })),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Messages(MessageEvent {
                cause: MessageCause::ToolResult,
                items: vec![ConversationItem::tool_result("call-pair", "blocked")],
                surface: SurfaceOp::Append,
            }))
            .unwrap();

        let rows = timeline.trajectory().rows;
        let call = rows
            .iter()
            .find(|row| row.kind == "tool.call" && row.state == "started")
            .unwrap();
        assert_eq!(call.producer, "tool:read_file");
        assert_eq!(call.summary, "path=/tmp/input.txt");
        let completion = rows
            .iter()
            .find(|row| row.kind == "tool.result" && row.state == "completed")
            .unwrap();
        assert_eq!(completion.producer, "tool:read_file");
        assert_eq!(completion.outcome.as_deref(), Some("not_dispatched"));
        assert_eq!(
            completion.issue_severity,
            Some(TrajectoryIssueSeverity::Error)
        );
        assert_eq!(completion.summary, "not_dispatched · reason=policy");
        let result = rows
            .iter()
            .find(|row| row.kind == "tool.result" && row.state != "completed")
            .unwrap();
        assert_eq!(result.producer, "tool:read_file");
        assert_eq!(result.correlation_id.as_deref(), Some("call-pair"));
    }

    #[test]
    fn projection_exposes_session_title_as_lifecycle_fact() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "Canonical title".into(),
                source: SessionTitleSource::User,
            }))
            .unwrap();

        let row = timeline.trajectory().rows.pop().unwrap();
        assert_eq!(row.layer, "meta");
        assert_eq!(row.class, "lifecycle");
        assert_eq!(row.producer, "user");
        assert_eq!(row.state, "user");
        assert_eq!(row.summary, "Canonical title");
    }
}
