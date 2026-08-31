//! Append-only agent timeline and its deterministic folds.
//!
//! The timeline is the durable causal ledger for a session. Streaming deltas
//! are transport-only; complete messages and lifecycle boundaries are facts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use sampling_types::{
    ContentPart, ConversationItem, DanglingToolCallReason, PermissionEvidence, SyntheticReason,
};
use serde::{Deserialize, Serialize};

use crate::SidebandSpawnEvent;

pub const TIMELINE_SCHEMA_VERSION: u8 = 23;
pub const MAX_WORKFLOW_RUN_ID_BYTES: usize = 128;
pub const MAX_WORKFLOW_INITIAL_MANIFEST_BYTES: usize = 512 * 1024;
pub const MAX_NOTIFICATION_ID_BYTES: usize = 128;
pub const MAX_NOTIFICATION_PAYLOAD_BYTES: u64 = 1024 * 1024;
/// A busy turn can accumulate monitor ticks faster than the model can consume
/// them. Preserve a useful recent window per monitor while the task output
/// artifact remains the complete stream.
pub const MAX_PENDING_MONITOR_PROGRESS_PER_TASK: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventSeq(u64);

impl EventSeq {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(#[serde(with = "turn_id_serde")] pub u64);

mod turn_id_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepId {
    pub turn: TurnId,
    pub index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceId {
    pub event: EventSeq,
    pub item: u32,
}

/// Exact, replay-stable range on the current model-visible Surface.
///
/// `start` and `end` are convenient boundaries. `shadowed` is authoritative:
/// it proves that a replacement covered every current item in between and
/// prevents an old range plan from being applied to a rewritten Surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRange {
    pub start: SurfaceId,
    pub end: SurfaceId,
    pub shadowed: Vec<SurfaceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImageShadowSource {
    Description { result_ref: crate::TimelineRangeRef },
}

/// One irreversible replacement for an image-bearing Surface item. Acceptance
/// consumes `source` and creates a new text-only Surface identity owned by the
/// enclosing `ImageProjection` event. The immutable source event remains raw
/// causal evidence, but no later Surface view can recover its image bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageShadow {
    pub source: SurfaceId,
    pub fingerprint: String,
    pub image_count: usize,
    pub replacement: String,
    pub provenance: ImageShadowSource,
}

/// Assistant response consumed together with image-bearing ToolResults. Tool
/// call identity remains, while the Assistant content, original arguments,
/// and exact ordered Reasoning/BackendToolCall carriers are irreversibly
/// replaced in the model-facing Surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageToolCallShadow {
    pub source: SurfaceId,
    pub tool_call_ids: Vec<String>,
    pub carrier_sources: Vec<SurfaceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageProjectionEvent {
    /// Runtime whose rejection triggered the conversion. Provenance only: it
    /// never scopes or reverses the installed shadows.
    pub trigger_runtime: sampling_types::ModelImageInputKey,
    pub source_revision: u64,
    pub shadows: Vec<ImageShadow>,
    pub tool_calls: Vec<ImageToolCallShadow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageCause {
    Seed,
    DirectUser,
    Interjection,
    User,
    Assistant,
    ToolResult,
    IntegrityRepair,
    Compaction,
    ToolResultPrune,
    MemoryContext,
    ContextRebuild,
    Rewind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SurfaceOp {
    Append,
    Replace {
        start: SurfaceId,
        end: SurfaceId,
        shadowed: Vec<SurfaceId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageEvent {
    pub cause: MessageCause,
    pub items: Vec<ConversationItem>,
    pub surface: SurfaceOp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnIdentity {
    pub origin: String,
    pub turn_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_definition_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnTerminal {
    pub stop_reason: String,
    pub completion_kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnInputKind {
    Prompt,
    Bash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptRecord {
    pub prompt_index: usize,
    pub text: String,
    pub input_kind: TurnInputKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TurnEvent {
    Started {
        id: TurnId,
        identity: TurnIdentity,
        model_id: String,
        input_message_count: usize,
        prompt_index: usize,
        prompt_text: String,
        input_kind: TurnInputKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        redirect_kind: Option<String>,
    },
    Ended {
        id: TurnId,
        outcome: String,
        duration_ms: u64,
        tool_count: u32,
        terminal: TurnTerminal,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cancellation_category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepEvent {
    Started {
        id: StepId,
    },
    Ended {
        id: StepId,
        outcome: String,
        duration_ms: u64,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestEvent {
    Started {
        id: String,
        turn: TurnId,
        step: StepId,
        model_id: String,
        input_message_count: usize,
        tool_count: usize,
    },
    Retrying {
        id: String,
        attempt: u32,
        max_retries: u32,
        reason: String,
    },
    Completed {
        id: String,
        duration_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time_to_first_token_ms: Option<u64>,
        usage: RequestUsage,
        response_message_count: usize,
    },
    Failed {
        id: String,
        duration_ms: u64,
        error_kind: String,
        message: String,
        retryable: bool,
    },
    Cancelled {
        id: String,
        duration_ms: u64,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolEvent {
    Started {
        call_id: String,
        turn: TurnId,
        step: StepId,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<serde_json::Value>,
    },
    Completed {
        call_id: String,
        name: String,
        outcome: String,
        duration_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

/// Durable lifecycle of one deterministic Workflow actor.
///
/// The Workflow journal records calls made by the actor; it cannot declare its
/// own existence or execution boundary. Those causal facts live in the parent
/// session Timeline so a merged Trajectory can attach `workflow:<run_id>` to
/// one exact spawn event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowEvent {
    Spawned {
        run_id: String,
        execution_epoch: u64,
        name: String,
        objective: String,
        script_hash: String,
        args_hash: String,
        /// Canonical, credential-free initial Run projection. Timeline owns
        /// Run existence; the mutable manifest sidecar may be rebuilt from
        /// this snapshot plus later lifecycle facts after a crash.
        initial_manifest: serde_json::Value,
    },
    Resumed {
        run_id: String,
        execution_epoch: u64,
    },
    Ended {
        run_id: String,
        execution_epoch: u64,
        status: WorkflowExecutionStatus,
        handoff: WorkflowTurnHandoff,
        duration_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Permanently close a resumable run while no execution is active (for
    /// example, cancelling a paused run).
    Closed {
        run_id: String,
        execution_epoch: u64,
        status: WorkflowExecutionStatus,
        handoff: WorkflowTurnHandoff,
        duration_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionStatus {
    UserPaused,
    BackOffPaused,
    NoProgressPaused,
    InfraPaused,
    Blocked,
    BudgetLimited,
    Interrupted,
    Complete,
    Failed,
    Cancelled,
}

/// Whether a durable Workflow boundary owns a model-facing successor turn.
///
/// This is recorded explicitly instead of being inferred from status: a
/// natural completion and an explicit stop may both be terminal while only
/// the former owns a follow-up turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTurnHandoff {
    None,
    Completion,
    AttentionRequired,
}

impl WorkflowExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserPaused => "user_paused",
            Self::BackOffPaused => "back_off_paused",
            Self::NoProgressPaused => "no_progress_paused",
            Self::InfraPaused => "infra_paused",
            Self::Blocked => "blocked",
            Self::BudgetLimited => "budget_limited",
            Self::Interrupted => "interrupted",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompactionEvent {
    Started {
        id: String,
        source_items: usize,
        prompt_index: usize,
    },
    /// The summary model call completed. Content remains single-sourced in the
    /// referenced Sideband result; this governance fact links it to the frozen
    /// parent input before the Surface replacement commits.
    Summary {
        id: String,
        input_ref: crate::TimelineRangeRef,
        result_ref: crate::TimelineRangeRef,
        /// Exact Surface range summarized by `result_ref` and subsequently
        /// shadowed by the compaction replacement.
        target: SurfaceRange,
        source_tokens: u64,
        summary_chars: usize,
    },
    Completed {
        id: String,
        source_items: usize,
        result_items: usize,
        duration_ms: u64,
    },
    Failed {
        id: String,
        duration_ms: u64,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryEvent {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationEvent {
    pub scope: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<TurnId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<StepId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Durable session control-plane state.
///
/// The payload stays shell-owned, while Timeline owns its identity, ordering,
/// durability and revision monotonicity. A transition may also carry its one
/// model-visible context item. Applying both from the same event prevents a
/// crash from committing state without its context. Transition activation is
/// layer-specific: AgentRole and GoalDefinition changes enter Surface after
/// the active step ends, while Behavior changes remain turn-bound. A
/// re-projection restores an already-effective item immediately after
/// compaction shadows its former Surface anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlContextLayer {
    AgentRole,
    GoalDefinition,
    PlanPhase,
    Behavior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlContextActivation {
    Transition,
    Reprojection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlContext {
    pub layer: ControlContextLayer,
    pub activation: ControlContextActivation,
    pub item: ConversationItem,
}

/// The context item currently governing one Control layer.
///
/// The anchor remains meaningful after compaction shadows it from Surface so
/// the exact effective context can be projected again. A transition waiting
/// for its authority boundary to end is deliberately not represented here.
#[derive(Debug, Clone)]
pub struct ActiveControlContext {
    pub surface_id: SurfaceId,
    pub item: ConversationItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEvent {
    pub revision: u64,
    pub snapshot: serde_json::Value,
    /// Layers whose prior model context stops being authoritative in this
    /// atomic transition. Historical Surface items remain evidence, but a
    /// later compaction must not reproject them as current instructions.
    pub retired_context_layers: Vec<ControlContextLayer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_contexts: Vec<ControlContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionTitleSource {
    User,
    Generated {
        sideband_id: String,
        result_seq: u64,
    },
    Fallback {
        sideband_id: String,
        terminal_seq: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTitleEvent {
    pub title: String,
    pub source: SessionTitleSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentContextSource {
    New,
    Forked,
    Resumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubagentSpawnEvent {
    pub subagent_id: String,
    pub child_session_id: String,
    /// Immediate security parent. Lifecycle ownership may independently be
    /// rooted at the primary Session for presentation and cancellation.
    pub security_parent_session_id: String,
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
    pub context_source: SubagentContextSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<crate::TimelineRangeRef>,
    pub context_normalized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_prompt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_definition_revision: Option<u64>,
    /// Whether this child owns a model-facing completion receipt. Internal
    /// harness children set this to false, and crash recovery must preserve
    /// that decision instead of manufacturing a reminder.
    pub surface_completion: bool,
    pub child_cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    pub effective_model_id: String,
    /// Secret-free identity of the exact provider/model/endpoint route used
    /// by this child. A resumed child must resolve to the same route; matching
    /// only the user-facing model ID is insufficient.
    pub model_transport_key: sampling_types::ModelImageInputKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<sampling_types::ReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentOutcome {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubagentTerminalEvent {
    pub subagent_id: String,
    pub child_session_id: String,
    pub outcome: SubagentOutcome,
    pub duration_ms: u64,
    pub tool_calls: u32,
    pub turns: u32,
    pub tokens_used: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<crate::TimelineRangeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "event",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SubagentEvent {
    Spawned(SubagentSpawnEvent),
    Ended(SubagentTerminalEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubagentSeedEvent {
    pub parent_timeline_id: String,
    pub parent_spawn_seq: u64,
    pub subagent_id: String,
    /// Direct security parent of the child. This is intentionally separate
    /// from the lifecycle-root Timeline id so sibling children cannot reuse
    /// one another's resume authority.
    pub security_parent_session_id: String,
    pub context_source: SubagentContextSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<crate::TimelineRangeRef>,
    pub normalized: bool,
}

/// Terminal fact owned by the child entity. The parent closes its spawn only
/// by referencing this exact event; result content lives in an immutable
/// content-addressed artifact referenced by `output_ref`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubagentResultEvent {
    pub subagent_id: String,
    pub outcome: SubagentOutcome,
    pub duration_ms: u64,
    pub tool_calls: u32,
    pub turns: u32,
    pub tokens_used: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_ref: Option<String>,
}

/// Domain identity of a signal delivered to this session.
///
/// Payload and presentation deliberately live outside this enum. The source
/// identity is the at-least-once deduplication key; the immutable payload is
/// referenced separately by [`NotificationPayloadRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanHandoffKind {
    Execute,
    Revise,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationSource {
    MonitorProgress {
        task_id: String,
        owner: NotificationOwner,
    },
    /// A background task that was still running at a graceful session
    /// checkpoint. Unlike terminal notifications, this receipt is context for
    /// the next real turn and must never start a turn by itself.
    TaskStillRunning {
        task_id: String,
        task_kind: NotificationTaskKind,
        owner: NotificationOwner,
    },
    TaskCompleted {
        task_id: String,
        task_kind: NotificationTaskKind,
        owner: NotificationOwner,
    },
    SubagentCompleted {
        subagent_id: String,
        owner: NotificationOwner,
    },
    PlanHandoff {
        artifact_hash: String,
        artifact_revision: u64,
        handoff: PlanHandoffKind,
    },
    WorkflowHandoff {
        run_id: String,
        handoff: WorkflowTurnHandoff,
    },
}

/// Stable producer identity for at-least-once delivery. Ownership is durable
/// receipt metadata captured on the first admission, not part of the producer
/// key: a retry after actor restart must resolve to that original evidence even
/// when the bridge can no longer reconstruct the owner in memory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NotificationSourceIdentity {
    MonitorProgress {
        task_id: String,
    },
    TaskStillRunning {
        task_id: String,
        task_kind: NotificationTaskKind,
    },
    TaskCompleted {
        task_id: String,
        task_kind: NotificationTaskKind,
    },
    SubagentCompleted {
        subagent_id: String,
    },
    PlanHandoff {
        artifact_hash: String,
        artifact_revision: u64,
        handoff: PlanHandoffKind,
    },
    WorkflowHandoff {
        run_id: String,
        handoff: WorkflowTurnHandoff,
    },
}

impl NotificationSource {
    fn subject_id(&self) -> &str {
        match self {
            Self::MonitorProgress { task_id, .. }
            | Self::TaskStillRunning { task_id, .. }
            | Self::TaskCompleted { task_id, .. } => task_id,
            Self::SubagentCompleted { subagent_id, .. } => subagent_id,
            Self::PlanHandoff { artifact_hash, .. } => artifact_hash,
            Self::WorkflowHandoff { run_id, .. } => run_id,
        }
    }

    pub fn owner(&self) -> NotificationOwner {
        match self {
            Self::MonitorProgress { owner, .. }
            | Self::TaskStillRunning { owner, .. }
            | Self::TaskCompleted { owner, .. } => owner.clone(),
            Self::SubagentCompleted { owner, .. } => owner.clone(),
            Self::PlanHandoff {
                artifact_hash,
                artifact_revision,
                handoff,
            } => NotificationOwner::Plan {
                artifact_hash: artifact_hash.clone(),
                artifact_revision: *artifact_revision,
                handoff: *handoff,
            },
            Self::WorkflowHandoff { .. } => NotificationOwner::Session,
        }
    }

    pub fn with_owner(mut self, notification_owner: NotificationOwner) -> Self {
        match &mut self {
            Self::MonitorProgress { owner, .. }
            | Self::TaskStillRunning { owner, .. }
            | Self::TaskCompleted { owner, .. } => *owner = notification_owner,
            Self::SubagentCompleted { owner, .. } => *owner = notification_owner,
            Self::PlanHandoff { .. } | Self::WorkflowHandoff { .. } => {}
        }
        self
    }

    fn identity(&self) -> NotificationSourceIdentity {
        match self {
            Self::MonitorProgress { task_id, .. } => NotificationSourceIdentity::MonitorProgress {
                task_id: task_id.clone(),
            },
            Self::TaskStillRunning {
                task_id, task_kind, ..
            } => NotificationSourceIdentity::TaskStillRunning {
                task_id: task_id.clone(),
                task_kind: *task_kind,
            },
            Self::TaskCompleted {
                task_id, task_kind, ..
            } => NotificationSourceIdentity::TaskCompleted {
                task_id: task_id.clone(),
                task_kind: *task_kind,
            },
            Self::SubagentCompleted { subagent_id, .. } => {
                NotificationSourceIdentity::SubagentCompleted {
                    subagent_id: subagent_id.clone(),
                }
            }
            Self::PlanHandoff {
                artifact_hash,
                artifact_revision,
                handoff,
            } => NotificationSourceIdentity::PlanHandoff {
                artifact_hash: artifact_hash.clone(),
                artifact_revision: *artifact_revision,
                handoff: *handoff,
            },
            Self::WorkflowHandoff { run_id, handoff } => {
                NotificationSourceIdentity::WorkflowHandoff {
                    run_id: run_id.clone(),
                    handoff: *handoff,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationOwner {
    #[default]
    Session,
    Goal {
        goal_id: String,
        definition_revision: u64,
    },
    Plan {
        artifact_hash: String,
        artifact_revision: u64,
        handoff: PlanHandoffKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationTaskKind {
    Task,
    Monitor,
}

/// Source-owned revision used together with [`NotificationSource`] for
/// idempotent at-least-once delivery.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationSourceVersion {
    Ordinal { value: u64 },
    Opaque { value: String },
}

/// Immutable, content-addressed model-facing notification payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPayloadRef {
    pub blake3: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationEvent {
    Received {
        id: String,
        owner_session_id: String,
        source: NotificationSource,
        source_version: NotificationSourceVersion,
        payload_ref: NotificationPayloadRef,
    },
    /// Consume one or more pending signals and materialize their exact
    /// synthetic model input in the same immutable fact. This removes the
    /// crash window between inbox consumption and Surface visibility.
    Consumed {
        notification_ids: Vec<String>,
        turn: TurnId,
        /// Present when inbox admission itself materializes a synthetic model
        /// input. `None` acknowledges a receipt whose payload was already
        /// surfaced by a tool result in this turn.
        input: Option<ConversationItem>,
    },
    /// Resolve receipts that must remain observable Timeline evidence but are
    /// forbidden from opening a model turn.
    Dismissed {
        notification_ids: Vec<String>,
        reason: NotificationDismissReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDismissReason {
    GoalOwnedAutostart {
        goal_id: String,
        definition_revision: u64,
    },
    PlanSuperseded {
        artifact_hash: String,
        artifact_revision: u64,
        handoff: PlanHandoffKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNotification {
    pub received_seq: EventSeq,
    pub id: String,
    pub owner_session_id: String,
    pub source: NotificationSource,
    pub source_version: NotificationSourceVersion,
    pub payload_ref: NotificationPayloadRef,
}

pub fn notification_id(
    owner_session_id: &str,
    source: &NotificationSource,
    source_version: &NotificationSourceVersion,
) -> Result<String, TimelineError> {
    if !valid_notification_identifier(owner_session_id)
        || !valid_notification_identifier(source.subject_id())
        || !valid_notification_source_version(source, source_version)
    {
        return Err(TimelineError::InvalidNotification);
    }
    let identity = serde_json::to_vec(&(owner_session_id, source.identity(), source_version))
        .map_err(|_| TimelineError::InvalidNotification)?;
    Ok(format!("notification-{}", blake3::hash(&identity).to_hex()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "event",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TimelineEventKind {
    Messages(MessageEvent),
    Turn(TurnEvent),
    Step(StepEvent),
    Request(RequestEvent),
    Tool(ToolEvent),
    Workflow(WorkflowEvent),
    Compaction(CompactionEvent),
    ImageProjection(ImageProjectionEvent),
    Recovery(RecoveryEvent),
    Observation(ObservationEvent),
    Control(ControlEvent),
    SessionTitle(SessionTitleEvent),
    Sideband(SidebandSpawnEvent),
    Subagent(SubagentEvent),
    SubagentSeed(SubagentSeedEvent),
    SubagentResult(SubagentResultEvent),
    Notification(NotificationEvent),
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    pub version: u8,
    pub seq: EventSeq,
    pub at_ms: i64,
    #[serde(flatten)]
    pub kind: TimelineEventKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimelineEventWire {
    version: u8,
    seq: EventSeq,
    at_ms: i64,
    #[serde(rename = "type")]
    event_type: String,
    event: serde_json::Value,
}

impl<'de> Deserialize<'de> for TimelineEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TimelineEventWire::deserialize(deserializer)?;
        let kind = serde_json::from_value(serde_json::json!({
            "type": wire.event_type,
            "event": wire.event,
        }))
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            version: wire.version,
            seq: wire.seq,
            at_ms: wire.at_ms,
            kind,
        })
    }
}

impl TimelineEvent {
    pub fn messages(&self) -> Option<&MessageEvent> {
        match &self.kind {
            TimelineEventKind::Messages(event) => Some(event),
            _ => None,
        }
    }

    fn appended_message_items(&self) -> Option<&[ConversationItem]> {
        match &self.kind {
            TimelineEventKind::Messages(MessageEvent {
                items,
                surface: SurfaceOp::Append,
                ..
            }) => Some(items),
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                input: Some(input),
                ..
            }) => Some(std::slice::from_ref(input)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LifecycleFold {
    active_turn: Option<TurnId>,
    active_step: Option<StepId>,
    seen_turns: BTreeSet<TurnId>,
    seen_steps: BTreeSet<StepId>,
    seen_requests: BTreeSet<String>,
    seen_tools: BTreeSet<String>,
    seen_compactions: BTreeSet<String>,
    workflows: BTreeMap<String, WorkflowFold>,
    open_subagents: BTreeMap<String, OpenSubagent>,
    open_requests: BTreeMap<String, (TurnId, StepId)>,
    open_tools: BTreeMap<String, (TurnId, StepId, String)>,
    open_compaction: Option<OpenCompaction>,
    control_revision: Option<u64>,
}

#[derive(Debug, Clone)]
struct WorkflowFold {
    execution_epoch: u64,
    open: bool,
    closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowLifecycle {
    pub name: String,
    pub objective: String,
    pub script_hash: String,
    pub args_hash: String,
    pub initial_manifest: serde_json::Value,
    pub execution_epoch: u64,
    pub status: Option<WorkflowExecutionStatus>,
    pub handoff: Option<WorkflowTurnHandoff>,
    pub message: Option<String>,
    pub open: bool,
    pub closed: bool,
}

#[derive(Debug, Clone)]
struct OpenSubagent {
    workflow_run_id: Option<String>,
}

#[derive(Debug, Clone)]
struct OpenCompaction {
    id: String,
    source_items: usize,
    summaries: u8,
    replacements: u8,
    target: Option<SurfaceRange>,
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    events: Vec<TimelineEvent>,
    surface: Vec<ConversationItem>,
    surface_ids: Vec<SurfaceId>,
    surface_revision: u64,
    pending_control_contexts: BTreeMap<ControlContextLayer, (EventSeq, ConversationItem)>,
    pending_notifications: BTreeMap<String, PendingNotification>,
    received_notification_ids: BTreeSet<String>,
    received_notifications:
        BTreeMap<(NotificationSourceIdentity, NotificationSourceVersion), EventSeq>,
    pending_monitor_notifications: BTreeMap<String, VecDeque<String>>,
    terminal_monitors: BTreeSet<String>,
    terminal_tasks: BTreeSet<String>,
    subagent_result_recorded: bool,
    lifecycle: LifecycleFold,
}

#[derive(Debug, thiserror::Error)]
pub enum TimelineError {
    #[error("unsupported timeline schema version {actual}; expected {expected}")]
    UnsupportedVersion { expected: u8, actual: u8 },
    #[error("timeline event seq {actual} is not the expected contiguous seq {expected}")]
    NonContiguousSeq { expected: u64, actual: u64 },
    #[error("timeline event timestamp must be non-negative")]
    InvalidTimestamp,
    #[error("append message event must contain at least one item")]
    EmptyAppend,
    #[error("replacement boundary is not present on the current surface")]
    StaleReplacementBoundary,
    #[error("replacement start occurs after its end")]
    ReversedReplacement,
    #[error("replacement shadow set does not exactly cover the selected surface range")]
    IncompleteShadowSet,
    #[error("surface item count exceeds u32 identity capacity")]
    TooManyItems,
    #[error("message cause does not match its surface operation or item shape")]
    InvalidMessageShape,
    #[error("rewind target {0} has no branch-local user prompt marker")]
    MissingPromptMarker(usize),
    #[error("tool-result prune must replace exactly one tool result")]
    InvalidToolResultPrune,
    #[error("tool-result prune changed fields other than content")]
    ToolResultIdentityChanged,
    #[error("image projection does not match the current Surface or its Sideband provenance")]
    InvalidImageProjection,
    #[error("control revision {actual} must be greater than the previous revision {previous}")]
    NonMonotonicControlRevision { previous: u64, actual: u64 },
    #[error("control model context must be one non-empty synthetic text reminder after system")]
    InvalidControlContext,
    #[error("control context re-projection requires a shadowed latest context in the same layer")]
    InvalidControlReprojection,
    #[error("turn {actual:?} cannot start while {active:?} is active")]
    TurnAlreadyActive { active: TurnId, actual: TurnId },
    #[error("turn {0:?} already has a start event")]
    TurnAlreadySeen(TurnId),
    #[error("turn identity has an invalid Goal owner")]
    InvalidTurnIdentity,
    #[error("turn boundary {actual:?} does not match active turn {active:?}")]
    TurnMismatch {
        active: Option<TurnId>,
        actual: TurnId,
    },
    #[error("step {actual:?} cannot start while {active:?} is active")]
    StepAlreadyActive { active: StepId, actual: StepId },
    #[error("step {0:?} already has a start event")]
    StepAlreadySeen(StepId),
    #[error("step boundary {actual:?} does not match active step {active:?}")]
    StepMismatch {
        active: Option<StepId>,
        actual: StepId,
    },
    #[error("request {0} already has a start event")]
    RequestAlreadyOpen(String),
    #[error("request {0} has no matching start event")]
    RequestNotOpen(String),
    #[error("tool call {0} already has a start event")]
    ToolAlreadyOpen(String),
    #[error("tool call {0} has no matching start event")]
    ToolNotOpen(String),
    #[error("tool call {call_id} completion name {actual} differs from start name {expected}")]
    ToolNameMismatch {
        call_id: String,
        expected: String,
        actual: String,
    },
    #[error("invalid workflow lifecycle event")]
    InvalidWorkflow,
    #[error("workflow {0} already has a spawn fact")]
    DuplicateWorkflowSpawn(String),
    #[error("workflow {0} has no spawn fact")]
    WorkflowNotFound(String),
    #[error("workflow {0} already has an active execution")]
    WorkflowAlreadyOpen(String),
    #[error("workflow {0} has no active execution")]
    WorkflowNotOpen(String),
    #[error("workflow {0} is permanently closed")]
    WorkflowAlreadyClosed(String),
    #[error("workflow {run_id} execution epoch {actual} does not follow {previous}")]
    WorkflowEpochMismatch {
        run_id: String,
        previous: u64,
        actual: u64,
    },
    #[error("{boundary} boundary cannot close with open child events")]
    OpenChildren { boundary: &'static str },
    #[error("compaction {0} already active")]
    CompactionAlreadyOpen(String),
    #[error("compaction {0} already has a start event")]
    CompactionAlreadySeen(String),
    #[error("compaction {0} has no matching start event")]
    CompactionNotOpen(String),
    #[error("compaction {0} recorded more than one summary")]
    DuplicateCompactionSummary(String),
    #[error("compaction replacement occurred before its summary was recorded")]
    CompactionReplacementBeforeSummary,
    #[error("compaction {0} has an invalid summary reference")]
    InvalidCompactionSummary(String),
    #[error("compaction replacement does not match its summarized Surface range")]
    CompactionTargetMismatch,
    #[error("compaction replacement requires an explicit stable Surface range")]
    CompactionRangeRequired,
    #[error("compaction replacement has no active compaction transaction")]
    CompactionReplacementNotOpen,
    #[error("compaction {0} recorded more than one replacement")]
    DuplicateCompactionReplacement(String),
    #[error("compaction {0} completed without exactly one replacement")]
    MissingCompactionReplacement(String),
    #[error("compaction {0} failed after its replacement was already committed")]
    FailedCompactionHasReplacement(String),
    #[error("non-compaction replacement occurred while compaction {0} was active")]
    ReplacementDuringCompaction(String),
    #[error("context rebuild requires a branch with no prompt turns")]
    ContextRebuildAfterTurn,
    #[error("invalid sideband spawn: {0}")]
    InvalidSideband(#[from] crate::SidebandError),
    #[error("sideband {0} already has a spawn fact")]
    DuplicateSidebandSpawn(String),
    #[error("session title must be non-empty and at most 160 characters")]
    InvalidSessionTitle,
    #[error("generated session title cannot replace a user title")]
    GeneratedTitleAfterUserTitle,
    #[error("generated session title source must identify a valid sideband result")]
    InvalidSessionTitleSource,
    #[error("invalid subagent lifecycle event")]
    InvalidSubagent,
    #[error("subagent {0} already has a spawn fact")]
    DuplicateSubagentSpawn(String),
    #[error("child session {0} is already owned by another subagent spawn")]
    DuplicateSubagentChild(String),
    #[error("subagent {0} has no open spawn fact")]
    SubagentNotOpen(String),
    #[error("subagent {0} already has a terminal fact")]
    SubagentAlreadyEnded(String),
    #[error("subagent terminal child session differs from its spawn fact")]
    SubagentChildMismatch,
    #[error("child Timeline already has a subagent seed-source fact")]
    DuplicateSubagentSeed,
    #[error("child Timeline has no subagent seed-source fact")]
    MissingSubagentSeed,
    #[error("child Timeline is closed by its subagent result fact")]
    SubagentTimelineEnded,
    #[error("child Timeline has no subagent result fact")]
    MissingSubagentResult,
    #[error("child result differs from its seed-source fact")]
    SubagentSeedMismatch,
    #[error("child seed-source does not match the parent spawn fact")]
    InvalidSubagentSeedLink,
    #[error("parent terminal does not match the referenced child result fact")]
    InvalidSubagentResultLink,
    #[error("invalid notification fact")]
    InvalidNotification,
    #[error("notification source was already received")]
    DuplicateNotificationSource,
    #[error("notification {0} was already consumed")]
    NotificationAlreadyConsumed(String),
    #[error("notification {0} has no received fact")]
    NotificationNotFound(String),
}

impl Timeline {
    pub fn from_events(events: Vec<TimelineEvent>) -> Result<Self, TimelineError> {
        let mut timeline = Self::default();
        for event in events {
            timeline.accept(event)?;
        }
        Ok(timeline)
    }

    pub fn from_seed(items: Vec<ConversationItem>) -> Result<Self, TimelineError> {
        let mut timeline = Self::default();
        for mut item in items {
            // Prompt coordinates are local to a live lineage. Inherited users
            // are an immutable seed prefix, not prompts in the child branch;
            // retaining the parent's indices makes child prompt 0 collide with
            // an inherited marker during rewind.
            if let ConversationItem::User(user) = &mut item {
                user.prompt_index = None;
            }
            timeline.append(item, MessageCause::Seed)?;
        }
        Ok(timeline)
    }

    pub fn events(&self) -> &[TimelineEvent] {
        &self.events
    }

    /// Durable notification inbox projection in receive order.
    ///
    /// This is derived exclusively from Timeline facts. A terminal task fact
    /// supersedes still-pending progress for the same monitor, so progress can
    /// be coalesced without ever discarding a terminal signal.
    pub fn pending_notifications(&self) -> Vec<PendingNotification> {
        let mut pending = self
            .pending_notifications
            .values()
            .cloned()
            .collect::<Vec<_>>();
        pending.sort_by_key(|notification| notification.received_seq);
        pending
    }

    /// Find the immutable receipt for an at-least-once source delivery.
    pub fn received_notification_id(
        &self,
        source: &NotificationSource,
        source_version: &NotificationSourceVersion,
    ) -> Option<&str> {
        self.received_notification_event(source, source_version)
            .and_then(|event| match &event.kind {
                TimelineEventKind::Notification(NotificationEvent::Received { id, .. }) => {
                    Some(id.as_str())
                }
                _ => None,
            })
    }

    pub(crate) fn received_notification_event(
        &self,
        source: &NotificationSource,
        source_version: &NotificationSourceVersion,
    ) -> Option<&TimelineEvent> {
        let seq = self
            .received_notifications
            .get(&(source.identity(), source_version.clone()))?;
        usize::try_from(seq.get())
            .ok()
            .and_then(|index| self.events.get(index))
    }

    pub fn next_seq(&self) -> EventSeq {
        EventSeq(self.events.len() as u64)
    }

    /// Monotonic revision of the model-visible Surface. Unlike the Timeline
    /// sequence, lifecycle and observability events do not advance it.
    pub fn surface_revision(&self) -> u64 {
        self.surface_revision
    }

    pub fn surface(&self) -> &[ConversationItem] {
        &self.surface
    }

    pub fn surface_ids(&self) -> &[SurfaceId] {
        &self.surface_ids
    }

    /// Effective model context for each Control layer, whether or not its
    /// anchor is still present on the current Surface.
    ///
    /// This replays the same layer-specific activation rule as Surface.
    pub fn active_control_contexts(
        &self,
    ) -> std::collections::BTreeMap<ControlContextLayer, ActiveControlContext> {
        let mut active = std::collections::BTreeMap::new();
        let mut active_turn = false;
        let mut active_step = false;
        let mut pending = BTreeMap::new();
        for event in &self.events {
            match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started { .. }) => active_turn = true,
                TimelineEventKind::Step(StepEvent::Started { .. }) => active_step = true,
                TimelineEventKind::Control(control) => {
                    for layer in &control.retired_context_layers {
                        pending.remove(layer);
                        active.remove(layer);
                    }
                    for (item, context) in control.model_contexts.iter().enumerate() {
                        let projection = ActiveControlContext {
                            surface_id: SurfaceId {
                                event: event.seq,
                                item: item as u32,
                            },
                            item: context.item.clone(),
                        };
                        if control_transition_waits_for_boundary(active_turn, active_step, context)
                        {
                            pending.insert(context.layer, projection);
                        } else {
                            active.insert(context.layer, projection);
                        }
                    }
                }
                TimelineEventKind::Step(StepEvent::Ended { .. }) => {
                    active_step = false;
                    for layer in [
                        ControlContextLayer::AgentRole,
                        ControlContextLayer::GoalDefinition,
                        ControlContextLayer::PlanPhase,
                    ] {
                        if let Some(projection) = pending.remove(&layer) {
                            active.insert(layer, projection);
                        }
                    }
                }
                TimelineEventKind::Turn(TurnEvent::Ended { .. }) => {
                    active_turn = false;
                    active_step = false;
                    for (layer, projection) in std::mem::take(&mut pending) {
                        active.insert(layer, projection);
                    }
                }
                _ => {}
            }
        }
        active
    }

    pub fn session_title(&self) -> Option<(EventSeq, &SessionTitleEvent)> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::SessionTitle(title) => Some((event.seq, title)),
                _ => None,
            })
    }

    /// Validate the causal half of a parent-spawn/child-seed pair. This is
    /// deliberately a cross-ledger operation: each Timeline remains an
    /// independently archivable entity, and callers resolve the other entity
    /// only when they need to dereference the link.
    pub fn validate_subagent_seed_link(
        &self,
        parent_timeline_id: &str,
        parent_spawn_seq: EventSeq,
        spawn: &SubagentSpawnEvent,
    ) -> Result<(), TimelineError> {
        let seed = self.events.iter().find_map(|event| match &event.kind {
            TimelineEventKind::SubagentSeed(seed) => Some(seed),
            _ => None,
        });
        let Some(seed) = seed else {
            return Err(TimelineError::MissingSubagentSeed);
        };
        if seed.parent_timeline_id != parent_timeline_id
            || seed.parent_spawn_seq != parent_spawn_seq.get()
            || seed.subagent_id != spawn.subagent_id
            || seed.security_parent_session_id != spawn.security_parent_session_id
            || seed.context_source != spawn.context_source
            || seed.source_ref != spawn.source_ref
            || seed.normalized != spawn.context_normalized
        {
            return Err(TimelineError::InvalidSubagentSeedLink);
        }
        Ok(())
    }

    /// Resolve and validate the child result referenced by a parent terminal.
    /// A syntactically valid range is insufficient: its exact event and every
    /// parent-owned terminal projection must agree with the child fact.
    pub fn validate_subagent_result_link(
        &self,
        parent_timeline_id: &str,
        parent_spawn_seq: EventSeq,
        spawn: &SubagentSpawnEvent,
        terminal: &SubagentTerminalEvent,
    ) -> Result<&SubagentResultEvent, TimelineError> {
        self.validate_subagent_seed_link(parent_timeline_id, parent_spawn_seq, spawn)?;
        let result_ref = terminal
            .result_ref
            .as_ref()
            .ok_or(TimelineError::MissingSubagentResult)?;
        let result_index = usize::try_from(result_ref.first_seq)
            .map_err(|_| TimelineError::InvalidSubagentResultLink)?;
        let Some(event) = self.events.get(result_index) else {
            return Err(TimelineError::MissingSubagentResult);
        };
        let TimelineEventKind::SubagentResult(result) = &event.kind else {
            return Err(TimelineError::InvalidSubagentResultLink);
        };
        if result_ref.timeline_id != spawn.child_session_id
            || result_ref.first_seq != result_ref.last_seq
            || event.seq.get() != result_ref.first_seq
            || terminal.subagent_id != spawn.subagent_id
            || terminal.child_session_id != spawn.child_session_id
            || result.subagent_id != spawn.subagent_id
            || result.outcome != terminal.outcome
            || result.duration_ms != terminal.duration_ms
            || result.tool_calls != terminal.tool_calls
            || result.turns != terminal.turns
            || result.tokens_used != terminal.tokens_used
            || result.error != terminal.error
        {
            return Err(TimelineError::InvalidSubagentResultLink);
        }
        Ok(result)
    }

    /// Build the uncompressed transcript for the currently selected branch.
    ///
    /// Compaction and content rewrites only change the model-facing Surface;
    /// they never erase canonical conversation facts. Rewind is the sole
    /// branch-selection operation, so it replaces the branch accumulated so
    /// far. This projection is the source for history/search features that
    /// need original text without resurrecting a rewound-away branch.
    pub fn branch_transcript(&self) -> Vec<ConversationItem> {
        self.branch_transcript_with_ids().1
    }

    /// Build the same selected-branch transcript while preserving a stable
    /// coordinate for every item. Auxiliary retrieval uses these coordinates
    /// to record the exact subset sent to a provider instead of pretending
    /// that a whole frozen branch was materialized.
    pub fn branch_transcript_with_ids(&self) -> (Vec<SurfaceId>, Vec<ConversationItem>) {
        let fold = fold_branch_provenance(self);
        fold.leaf_order
            .into_iter()
            .filter_map(|id| fold.leaf_values.get(&id).cloned().map(|value| (id, value)))
            .unzip()
    }

    /// Original branch leaves unloaded by completed compaction transactions.
    ///
    /// A compaction target names the Surface nodes visible at summary time,
    /// but content-only tool-result pruning creates newer Surface identities
    /// before compaction. Recall
    /// consumes the unmodified branch transcript, whose items retain their
    /// earlier identities. Fold replacement provenance here so both views use
    /// the same leaf coordinates instead of silently losing recallability
    /// after an intermediate rewrite.
    ///
    /// Failed or half-written transactions never become recall evidence.
    pub fn completed_compaction_unloaded_branch_ids(&self) -> Vec<SurfaceId> {
        fold_branch_provenance(self).unloaded.into_iter().collect()
    }

    /// Build the uncompressed current branch and cut it before prompt `target`.
    ///
    /// Compaction and content-only rewrites shadow Surface nodes but do not
    /// erase rewind history. A Rewind replacement is different: it selects a
    /// new branch root, so earlier discarded appends must not reappear.
    pub fn rewind_surface(&self, target: usize) -> Result<Vec<ConversationItem>, TimelineError> {
        let mut branch = self.branch_transcript();
        if !branch.iter().any(|item| {
            matches!(item, ConversationItem::User(user) if user.prompt_index == Some(target))
        }) {
            return Err(TimelineError::MissingPromptMarker(target));
        }
        let keep = sampling_types::conversation_truncate_for_prompt(&branch, target);
        branch.truncate(keep);
        Ok(branch)
    }

    /// User-authored inputs for the selected branch, indexed by prompt number.
    pub fn prompt_records(&self) -> Vec<PromptRecord> {
        let mut prompts = BTreeMap::<usize, PromptRecord>::new();
        for event in &self.events {
            match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started {
                    identity,
                    prompt_index: index,
                    prompt_text: text,
                    input_kind,
                    ..
                }) if identity.origin == "user" => {
                    prompts.insert(
                        *index,
                        PromptRecord {
                            prompt_index: *index,
                            text: text.clone(),
                            input_kind: *input_kind,
                        },
                    );
                }
                TimelineEventKind::Messages(MessageEvent {
                    cause: MessageCause::Rewind,
                    items,
                    ..
                }) => {
                    let next = items
                        .iter()
                        .filter_map(|item| match item {
                            ConversationItem::User(user) => user.prompt_index,
                            _ => None,
                        })
                        .max()
                        .map_or(0, |index| index.saturating_add(1));
                    prompts.retain(|index, _| *index < next);
                }
                _ => {}
            }
        }
        prompts.into_values().collect()
    }

    pub fn direct_user_permission_evidence(&self) -> Vec<ConversationItem> {
        self.branch_transcript()
            .into_iter()
            .filter_map(|item| {
                let ConversationItem::User(user) = item else {
                    return None;
                };
                let evidence = user.permission_evidence?;
                if evidence.text().trim().is_empty() {
                    return None;
                }
                let mut authority = ConversationItem::user(evidence.text());
                authority.set_permission_evidence(evidence);
                Some(authority)
            })
            .collect()
    }

    /// Ordered permission-classifier context for the selected branch.
    ///
    /// User entries are rendered exclusively from typed, sanitized authority
    /// evidence. Assistant entries retain only tool calls downstream; neither
    /// projected user text nor assistant prose/tool results can become
    /// authorization evidence.
    pub fn permission_classifier_context(&self) -> Vec<ConversationItem> {
        self.branch_transcript()
            .into_iter()
            .filter_map(|item| match item {
                ConversationItem::User(user) => {
                    let evidence = user.permission_evidence?;
                    if evidence.text().trim().is_empty() {
                        return None;
                    }
                    let mut authority = ConversationItem::user(evidence.text());
                    authority.set_permission_evidence(evidence);
                    Some(authority)
                }
                ConversationItem::Assistant(assistant) if !assistant.tool_calls.is_empty() => {
                    Some(ConversationItem::Assistant(assistant))
                }
                _ => None,
            })
            .collect()
    }

    /// Next branch-local prompt coordinate, derived only from accepted turn
    /// starts and rewind branch selections. Every v2 turn start carries both
    /// its coordinate and prompt text; this fold consumes only the coordinate.
    pub fn next_prompt_index(&self) -> usize {
        let mut prompts = BTreeMap::<usize, ()>::new();
        for event in &self.events {
            match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started {
                    prompt_index: index,
                    ..
                }) => {
                    prompts.insert(*index, ());
                }
                TimelineEventKind::Messages(MessageEvent {
                    cause: MessageCause::Rewind,
                    items,
                    ..
                }) => {
                    let next = items
                        .iter()
                        .filter_map(|item| match item {
                            ConversationItem::User(user) => user.prompt_index,
                            _ => None,
                        })
                        .max()
                        .map_or(0, |index| index.saturating_add(1));
                    prompts.retain(|index, _| *index < next);
                }
                _ => {}
            }
        }
        let mut next = 0;
        while prompts.remove(&next).is_some() {
            next += 1;
        }
        next
    }

    pub fn last_completed_compaction_prompt_index(&self) -> Option<usize> {
        let mut starts = BTreeMap::<&str, usize>::new();
        let mut latest = None;
        for event in &self.events {
            match &event.kind {
                TimelineEventKind::Compaction(CompactionEvent::Started {
                    id,
                    prompt_index,
                    ..
                }) => {
                    starts.insert(id, *prompt_index);
                }
                TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. }) => {
                    if let Some(index) = starts.get(id.as_str()) {
                        latest = Some(*index);
                    }
                }
                TimelineEventKind::Messages(MessageEvent {
                    cause: MessageCause::Rewind,
                    ..
                }) => latest = None,
                _ => {}
            }
        }
        latest
    }

    /// Whether crash recovery may legally close a compaction that linked its
    /// durable sideband summary but had not yet committed the Surface shadow.
    /// Consumers validating cross-ledger provenance must permit only this
    /// exact open state; completed or otherwise malformed transactions remain
    /// fail-closed.
    pub fn compaction_summary_awaits_recovery(&self, id: &str, target: &SurfaceRange) -> bool {
        self.lifecycle.open_compaction.as_ref().is_some_and(|open| {
            open.id == id
                && open.summaries == 1
                && open.replacements == 0
                && open.target.as_ref() == Some(target)
        })
    }

    pub fn surface_len(&self) -> usize {
        self.surface.len()
    }

    pub fn surface_item(&self, index: usize) -> Option<&ConversationItem> {
        self.surface.get(index)
    }

    pub fn active_turn(&self) -> Option<TurnId> {
        self.lifecycle.active_turn
    }

    pub fn active_step(&self) -> Option<StepId> {
        self.lifecycle.active_step
    }

    pub fn open_request_ids(&self) -> impl Iterator<Item = &str> {
        self.lifecycle.open_requests.keys().map(String::as_str)
    }

    pub fn open_tool_call_ids(&self) -> impl Iterator<Item = &str> {
        self.lifecycle.open_tools.keys().map(String::as_str)
    }

    pub fn open_workflow_run_ids(&self) -> impl Iterator<Item = &str> {
        self.lifecycle
            .workflows
            .iter()
            .filter_map(|(run_id, lifecycle)| lifecycle.open.then_some(run_id.as_str()))
    }

    /// Canonical lifecycle projection for a Workflow owned by this Timeline.
    /// Consumers must not reconstruct Workflow state by scanning raw events.
    pub fn workflow_lifecycle(&self, run_id: &str) -> Option<WorkflowLifecycle> {
        let fold = self.lifecycle.workflows.get(run_id)?;
        let (name, objective, script_hash, args_hash, initial_manifest) =
            self.events.iter().find_map(|event| match &event.kind {
                TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                    run_id: candidate,
                    name,
                    objective,
                    script_hash,
                    args_hash,
                    initial_manifest,
                    ..
                }) if candidate == run_id => Some((
                    name.clone(),
                    objective.clone(),
                    script_hash.clone(),
                    args_hash.clone(),
                    initial_manifest.clone(),
                )),
                _ => None,
            })?;
        let terminal = (!fold.open).then(|| {
            self.events
                .iter()
                .rev()
                .find_map(|event| match &event.kind {
                    TimelineEventKind::Workflow(WorkflowEvent::Ended {
                        run_id: candidate,
                        status,
                        handoff,
                        message,
                        ..
                    })
                    | TimelineEventKind::Workflow(WorkflowEvent::Closed {
                        run_id: candidate,
                        status,
                        handoff,
                        message,
                        ..
                    }) if candidate == run_id => Some((*status, *handoff, message.clone())),
                    _ => None,
                })
        });
        let (status, handoff, message) = terminal
            .flatten()
            .map_or((None, None, None), |(status, handoff, message)| {
                (Some(status), Some(handoff), message)
            });
        Some(WorkflowLifecycle {
            name,
            objective,
            script_hash,
            args_hash,
            initial_manifest,
            execution_epoch: fold.execution_epoch,
            status,
            handoff,
            message,
            open: fold.open,
            closed: fold.closed,
        })
    }

    /// Append deterministic terminal facts for work left open by an interrupted
    /// process. Physical history is never truncated or rewritten.
    pub fn settle_open_compaction(
        &mut self,
        reason: &str,
    ) -> Result<Option<TimelineEvent>, TimelineError> {
        let Some(open) = self.lifecycle.open_compaction.clone() else {
            return Ok(None);
        };
        let duration_ms = duration_since(self.compaction_started_at(&open.id), wall_time_ms());
        let terminal = if open.summaries == 1 && open.replacements == 1 {
            CompactionEvent::Completed {
                id: open.id,
                source_items: open.source_items,
                result_items: self.surface.len(),
                duration_ms,
            }
        } else {
            CompactionEvent::Failed {
                id: open.id,
                duration_ms,
                error: reason.into(),
            }
        };
        self.record(TimelineEventKind::Compaction(terminal))
            .map(Some)
    }

    pub fn recover_interrupted(&mut self) -> Result<Vec<TimelineEvent>, TimelineError> {
        // Subagents are independently durable entities whose backend may
        // still be running after this process restarts. Only the backend-aware
        // reconciler may close them. Workflows that own an open child must
        // remain open for the same reason.
        let workflows = self
            .lifecycle
            .workflows
            .iter()
            .filter_map(|(run_id, lifecycle)| {
                (lifecycle.open
                    && !self.lifecycle.open_subagents.values().any(|subagent| {
                        subagent.workflow_run_id.as_deref() == Some(run_id.as_str())
                    }))
                .then_some((run_id.clone(), lifecycle.execution_epoch))
            })
            .collect::<Vec<_>>();
        if self.lifecycle.active_turn.is_none()
            && self.lifecycle.active_step.is_none()
            && self.lifecycle.open_requests.is_empty()
            && self.lifecycle.open_tools.is_empty()
            && self.lifecycle.open_compaction.is_none()
            && workflows.is_empty()
        {
            return Ok(Vec::new());
        }
        let start = self.events.len();
        let now = wall_time_ms();
        let requests = self
            .lifecycle
            .open_requests
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let tools = self
            .lifecycle
            .open_tools
            .iter()
            .map(|(call_id, (_, _, name))| (call_id.clone(), name.clone()))
            .collect::<Vec<_>>();
        let compaction = self.lifecycle.open_compaction.clone();
        self.record(TimelineEventKind::Recovery(RecoveryEvent {
            action: "close_interrupted_work".into(),
            correlation_id: self.lifecycle.active_turn.map(|turn| turn.0.to_string()),
            reason: "process ended before causal children reached a terminal state".into(),
            details: Some(serde_json::json!({
                "requests": &requests,
                "tools": tools.iter().map(|(id, _)| id).collect::<Vec<_>>(),
                "compaction": compaction.as_ref().map(|open| &open.id),
                "workflows": workflows.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            })),
        }))?;
        for id in requests {
            let duration_ms = duration_since(self.request_started_at(&id), now);
            self.record(TimelineEventKind::Request(RequestEvent::Cancelled {
                id,
                duration_ms,
                reason: "process_interrupted".into(),
            }))?;
        }
        for (call_id, name) in tools {
            let duration_ms = duration_since(self.tool_started_at(&call_id), now);
            self.record(TimelineEventKind::Tool(ToolEvent::Completed {
                call_id,
                name,
                outcome: "outcome_unknown".into(),
                duration_ms,
                details: Some(serde_json::json!({ "recovered": true })),
            }))?;
        }
        if let Some(open) = compaction {
            let duration_ms = duration_since(self.compaction_started_at(&open.id), now);
            let terminal = if open.summaries == 1 && open.replacements == 1 {
                CompactionEvent::Completed {
                    id: open.id,
                    source_items: open.source_items,
                    result_items: self.surface.len(),
                    duration_ms,
                }
            } else {
                CompactionEvent::Failed {
                    id: open.id,
                    duration_ms,
                    error: "process_interrupted".into(),
                }
            };
            self.record(TimelineEventKind::Compaction(terminal))?;
        }
        for (run_id, execution_epoch) in workflows {
            let duration_ms =
                duration_since(self.workflow_started_at(&run_id, execution_epoch), now);
            self.record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id,
                execution_epoch,
                status: WorkflowExecutionStatus::Interrupted,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms,
                message: Some("process_interrupted".into()),
            }))?;
        }
        if let Some((step, started)) = self
            .lifecycle
            .active_step
            .map(|step| (step, self.step_started_at(step)))
        {
            self.record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "interrupted".into(),
                duration_ms: duration_since(started, now),
            }))?;
        }
        if let Some((turn, started)) = self
            .lifecycle
            .active_turn
            .map(|turn| (turn, self.turn_started_at(turn)))
        {
            self.record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "interrupted".into(),
                duration_ms: duration_since(started, now),
                tool_count: 0,
                terminal: TurnTerminal {
                    stop_reason: "interrupted".into(),
                    completion_kind: "recovered_interruption".into(),
                },
                cancellation_category: Some("process_interrupted".into()),
                details: Some(serde_json::json!({ "recovered": true })),
            }))?;
        }
        Ok(self.events[start..].to_vec())
    }

    /// Repair message-level tool pairing after process interruption. Lifecycle
    /// terminals alone cannot satisfy provider protocols: every assistant tool
    /// declaration also needs one adjacent `ToolResult` in the Surface.
    pub fn recover_surface_integrity(&mut self) -> Result<Vec<TimelineEvent>, TimelineError> {
        let mut repaired_surface = self.surface.clone();
        let report = crate::compaction_utils::repair_history_with_reason(
            &mut repaired_surface,
            DanglingToolCallReason::ProcessInterrupted,
        );
        if !report.changed() {
            return Ok(Vec::new());
        }

        let start = self.events.len();
        self.record(TimelineEventKind::Recovery(RecoveryEvent {
            action: "repair_surface_tool_pairing".into(),
            correlation_id: None,
            reason: "surface contained duplicate or dangling tool results after interruption"
                .into(),
            details: Some(serde_json::json!({
                "deduplicated": report.duplicates_removed,
                "stripped": report.stripped_tool_result_ids,
                "synthesized": report.synthetic_results_inserted,
            })),
        }))?;
        self.replace_all(repaired_surface, MessageCause::IntegrityRepair)?;
        Ok(self.events[start..].to_vec())
    }

    /// Apply an explicit provider-pairing repair as append-only recovery facts.
    pub fn repair_surface_history(
        &mut self,
    ) -> Result<
        (
            crate::compaction_utils::HistoryRepairReport,
            Vec<TimelineEvent>,
        ),
        TimelineError,
    > {
        let mut repaired_surface = self.surface.clone();
        let report = crate::compaction_utils::repair_history(&mut repaired_surface);
        if !report.changed() {
            return Ok((report, Vec::new()));
        }
        let start = self.events.len();
        // Explicit repair is one atomic Surface transition. Keeping intent and
        // replacement in separate events would allow a partially committed
        // repair to strand the actor between two authoritative facts.
        self.replace_all(repaired_surface, MessageCause::IntegrityRepair)?;
        Ok((report, self.events[start..].to_vec()))
    }

    fn request_started_at(&self, id: &str) -> Option<i64> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Request(RequestEvent::Started { id: candidate, .. })
                    if candidate == id =>
                {
                    Some(event.at_ms)
                }
                _ => None,
            })
    }

    fn tool_started_at(&self, id: &str) -> Option<i64> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Tool(ToolEvent::Started { call_id, .. }) if call_id == id => {
                    Some(event.at_ms)
                }
                _ => None,
            })
    }

    fn compaction_started_at(&self, id: &str) -> Option<i64> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Compaction(CompactionEvent::Started {
                    id: candidate, ..
                }) if candidate == id => Some(event.at_ms),
                _ => None,
            })
    }

    fn workflow_started_at(&self, run_id: &str, execution_epoch: u64) -> Option<i64> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                    run_id: candidate,
                    execution_epoch: epoch,
                    ..
                })
                | TimelineEventKind::Workflow(WorkflowEvent::Resumed {
                    run_id: candidate,
                    execution_epoch: epoch,
                }) if candidate == run_id && *epoch == execution_epoch => Some(event.at_ms),
                _ => None,
            })
    }

    fn step_started_at(&self, id: StepId) -> Option<i64> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Step(StepEvent::Started { id: candidate })
                    if *candidate == id =>
                {
                    Some(event.at_ms)
                }
                _ => None,
            })
    }

    fn turn_started_at(&self, id: TurnId) -> Option<i64> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started { id: candidate, .. })
                    if *candidate == id =>
                {
                    Some(event.at_ms)
                }
                _ => None,
            })
    }

    pub fn turn_items_since(&self, start: EventSeq) -> Vec<ConversationItem> {
        let fold = fold_branch_provenance(self);
        fold.leaf_order
            .into_iter()
            .filter(|id| {
                fold.leaf_birth.get(id).is_some_and(|birth| *birth >= start)
                    && fold.leaf_is_message.get(id).copied().unwrap_or(false)
            })
            .filter_map(|id| fold.leaf_values.get(&id).cloned())
            .collect()
    }

    pub fn append(
        &mut self,
        item: ConversationItem,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        self.append_many(vec![item], cause)
    }

    pub fn append_many(
        &mut self,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        self.record(TimelineEventKind::Messages(MessageEvent {
            cause,
            items,
            surface: SurfaceOp::Append,
        }))
    }

    pub fn record(&mut self, kind: TimelineEventKind) -> Result<TimelineEvent, TimelineError> {
        let event = self.prepare(kind)?;
        self.accept(event)?;
        Ok(self
            .events
            .last()
            .expect("accepted event must be stored")
            .clone())
    }

    /// Build and validate the next event without mutating the fold. This is the
    /// prepare phase used by fail-closed durable boundaries: storage commits
    /// the exact event first, then the actor accepts it while serialization
    /// prevents intervening writes.
    pub fn prepare(&self, kind: TimelineEventKind) -> Result<TimelineEvent, TimelineError> {
        let event = TimelineEvent {
            version: TIMELINE_SCHEMA_VERSION,
            seq: self.next_seq(),
            at_ms: wall_time_ms(),
            kind,
        };
        self.validate(&event)?;
        Ok(event)
    }

    pub fn replace_all(
        &mut self,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        if cause == MessageCause::Compaction {
            return Err(TimelineError::CompactionRangeRequired);
        }
        if self.surface.is_empty() {
            return self.append_many(items, cause);
        }
        self.replace_range(0, self.surface.len() - 1, items, cause)
    }

    pub fn replace_range(
        &mut self,
        start_index: usize,
        end_index: usize,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        if cause == MessageCause::Compaction {
            return Err(TimelineError::CompactionRangeRequired);
        }
        let Some(start) = self.surface_ids.get(start_index).copied() else {
            return Err(TimelineError::StaleReplacementBoundary);
        };
        let Some(end) = self.surface_ids.get(end_index).copied() else {
            return Err(TimelineError::StaleReplacementBoundary);
        };
        if start_index > end_index {
            return Err(TimelineError::ReversedReplacement);
        }
        let shadowed = self.surface_ids[start_index..=end_index].to_vec();
        self.record(TimelineEventKind::Messages(MessageEvent {
            cause,
            items,
            surface: SurfaceOp::Replace {
                start,
                end,
                shadowed,
            },
        }))
    }

    /// Replace an externally planned range using its stable Surface identity.
    /// The normal message validator proves that the range is still current.
    pub fn replace_compaction_range(
        &mut self,
        target: SurfaceRange,
        items: Vec<ConversationItem>,
    ) -> Result<TimelineEvent, TimelineError> {
        self.record(TimelineEventKind::Messages(MessageEvent {
            cause: MessageCause::Compaction,
            items,
            surface: SurfaceOp::Replace {
                start: target.start,
                end: target.end,
                shadowed: target.shadowed,
            },
        }))
    }

    pub fn accept(&mut self, event: TimelineEvent) -> Result<(), TimelineError> {
        let lifecycle = self.validate(&event)?;
        match &event.kind {
            TimelineEventKind::Messages(messages) => self.apply_messages(event.seq, messages),
            TimelineEventKind::ImageProjection(projection) => {
                let branch = fold_branch_provenance(self);
                // One completed compaction summary may own several raw image
                // leaves. Projection mutates the summary's SurfaceId after the
                // first leaf, so resolving every later leaf through the stale
                // pre-projection id would silently skip it. Freeze ownership,
                // not identity: all leaves owned by one current Surface entry
                // keep targeting the same index while their replacements are
                // composed in event order.
                let surface_index_by_leaf = branch
                    .surface
                    .iter()
                    .filter_map(|entry| {
                        self.surface_ids
                            .iter()
                            .position(|source| source == &entry.id)
                            .map(|index| (entry, index))
                    })
                    .flat_map(|(entry, index)| {
                        entry.leaves.iter().copied().map(move |leaf| (leaf, index))
                    })
                    .collect::<std::collections::HashMap<_, _>>();
                for (item, shadow) in projection.shadows.iter().enumerate() {
                    let Some(source_leaf) = branch.leaf_values.get(&shadow.source) else {
                        continue;
                    };
                    let Some(index) = surface_index_by_leaf.get(&shadow.source).copied() else {
                        continue;
                    };
                    let projected = sampling_types::conversation::replace_item_images_with_text(
                        &mut self.surface[index],
                        &shadow.replacement,
                    );
                    let derived_redacted =
                        sampling_types::conversation::redact_projected_image_compaction_references(
                            &mut self.surface[index],
                            source_leaf,
                            &shadow.replacement,
                        );
                    if projected > 0 || derived_redacted {
                        if projected > 0 {
                            debug_assert_eq!(projected, shadow.image_count);
                        }
                        self.surface_ids[index] = SurfaceId {
                            event: event.seq,
                            item: item as u32,
                        };
                    }
                }
                let mut replacement_item = projection.shadows.len() as u32;
                for tool_call in &projection.tool_calls {
                    let Some(source_leaf) = branch.leaf_values.get(&tool_call.source) else {
                        continue;
                    };
                    let Some(index) = surface_index_by_leaf.get(&tool_call.source).copied() else {
                        continue;
                    };
                    for item in &mut self.surface {
                        sampling_types::conversation::redact_projected_image_tool_result_references(
                            item,
                            source_leaf,
                            &tool_call.tool_call_ids,
                        );
                    }
                    let redacted = tool_call
                        .tool_call_ids
                        .iter()
                        .filter(|id| {
                            sampling_types::conversation::redact_projected_image_tool_call(
                                &mut self.surface[index],
                                id,
                            )
                        })
                        .count();
                    let derived_redacted = sampling_types::conversation::redact_projected_image_tool_call_compaction_references(
                        &mut self.surface[index],
                        source_leaf,
                        &tool_call.tool_call_ids,
                    );
                    if redacted > 0 || derived_redacted {
                        if redacted > 0 {
                            debug_assert_eq!(redacted, tool_call.tool_call_ids.len());
                        }
                        self.surface_ids[index] = SurfaceId {
                            event: event.seq,
                            item: replacement_item,
                        };
                    }
                    replacement_item = replacement_item.saturating_add(1);
                    for carrier_source in &tool_call.carrier_sources {
                        let carrier_item = replacement_item;
                        replacement_item = replacement_item.saturating_add(1);
                        let Some(source_leaf) = branch.leaf_values.get(carrier_source) else {
                            continue;
                        };
                        let Some(index) = surface_index_by_leaf.get(carrier_source).copied() else {
                            continue;
                        };
                        let carrier_redacted =
                            sampling_types::conversation::redact_projected_image_response_carrier(
                                &mut self.surface[index],
                            );
                        let derived_redacted = sampling_types::conversation::redact_projected_image_response_carrier_compaction_references(
                            &mut self.surface[index],
                            source_leaf,
                        );
                        if carrier_redacted || derived_redacted {
                            self.surface_ids[index] = SurfaceId {
                                event: event.seq,
                                item: carrier_item,
                            };
                        }
                    }
                }
                self.surface_revision = self.surface_revision.saturating_add(1);
            }
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                input: Some(input),
                ..
            }) => self.append_surface_items(event.seq, std::slice::from_ref(input)),
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                input: None, ..
            }) => {}
            TimelineEventKind::Notification(NotificationEvent::Dismissed { .. }) => {}
            TimelineEventKind::Control(control) => {
                for layer in &control.retired_context_layers {
                    self.pending_control_contexts.remove(layer);
                }
                for context in &control.model_contexts {
                    if control_transition_waits_for_boundary(
                        self.lifecycle.active_turn.is_some(),
                        self.lifecycle.active_step.is_some(),
                        context,
                    ) {
                        self.pending_control_contexts
                            .insert(context.layer, (event.seq, context.item.clone()));
                    } else {
                        self.append_surface_items(event.seq, std::slice::from_ref(&context.item));
                    }
                }
            }
            TimelineEventKind::Step(StepEvent::Ended { .. }) => {
                for (source, item) in
                    take_pending_step_control_contexts(&mut self.pending_control_contexts)
                {
                    self.append_surface_items(source, std::slice::from_ref(&item));
                }
            }
            TimelineEventKind::Turn(TurnEvent::Ended { .. }) => {
                for (source, item) in
                    take_pending_control_contexts(&mut self.pending_control_contexts)
                {
                    self.append_surface_items(source, std::slice::from_ref(&item));
                }
            }
            _ => {}
        }
        if let TimelineEventKind::Notification(notification) = &event.kind {
            self.apply_notification(event.seq, notification);
        }
        if matches!(&event.kind, TimelineEventKind::SubagentResult(_)) {
            self.subagent_result_recorded = true;
        }
        self.lifecycle = lifecycle;
        self.events.push(event);
        Ok(())
    }

    fn apply_notification(&mut self, seq: EventSeq, event: &NotificationEvent) {
        match event {
            NotificationEvent::Received {
                id,
                owner_session_id,
                source,
                source_version,
                payload_ref,
            } => {
                self.received_notification_ids.insert(id.clone());
                self.received_notifications
                    .insert((source.identity(), source_version.clone()), seq);
                let notification = PendingNotification {
                    received_seq: seq,
                    id: id.clone(),
                    owner_session_id: owner_session_id.clone(),
                    source: source.clone(),
                    source_version: source_version.clone(),
                    payload_ref: payload_ref.clone(),
                };
                match source {
                    NotificationSource::TaskCompleted {
                        task_id, task_kind, ..
                    } => {
                        self.terminal_tasks.insert(task_id.clone());
                        let checkpoints = self
                            .pending_notifications
                            .iter()
                            .filter_map(|(pending_id, pending)| {
                                matches!(
                                    &pending.source,
                                    NotificationSource::TaskStillRunning {
                                        task_id: pending_task_id,
                                        ..
                                    } if pending_task_id == task_id
                                )
                                .then(|| pending_id.clone())
                            })
                            .collect::<Vec<_>>();
                        for checkpoint_id in checkpoints {
                            self.pending_notifications.remove(&checkpoint_id);
                        }
                        if *task_kind == NotificationTaskKind::Monitor {
                            self.terminal_monitors.insert(task_id.clone());
                            if let Some(progress) =
                                self.pending_monitor_notifications.remove(task_id)
                            {
                                for progress_id in progress {
                                    self.pending_notifications.remove(&progress_id);
                                }
                            }
                        }
                        self.pending_notifications.insert(id.clone(), notification);
                    }
                    NotificationSource::MonitorProgress { task_id, .. } => {
                        if self.terminal_monitors.contains(task_id) {
                            return;
                        }
                        self.pending_notifications.insert(id.clone(), notification);
                        let progress = self
                            .pending_monitor_notifications
                            .entry(task_id.clone())
                            .or_default();
                        progress.push_back(id.clone());
                        while progress.len() > MAX_PENDING_MONITOR_PROGRESS_PER_TASK {
                            if let Some(superseded) = progress.pop_front() {
                                self.pending_notifications.remove(&superseded);
                            }
                        }
                    }
                    NotificationSource::TaskStillRunning { task_id, .. } => {
                        if self.terminal_tasks.contains(task_id) {
                            return;
                        }
                        self.pending_notifications.insert(id.clone(), notification);
                    }
                    NotificationSource::SubagentCompleted { .. }
                    | NotificationSource::PlanHandoff { .. }
                    | NotificationSource::WorkflowHandoff { .. } => {
                        self.pending_notifications.insert(id.clone(), notification);
                    }
                }
            }
            NotificationEvent::Consumed {
                notification_ids, ..
            }
            | NotificationEvent::Dismissed {
                notification_ids, ..
            } => {
                for id in notification_ids {
                    let removed = self.pending_notifications.remove(id);
                    if let Some(PendingNotification {
                        source: NotificationSource::MonitorProgress { task_id, .. },
                        ..
                    }) = removed
                        && let Some(progress) = self.pending_monitor_notifications.get_mut(&task_id)
                    {
                        progress.retain(|progress_id| progress_id != id);
                        if progress.is_empty() {
                            self.pending_monitor_notifications.remove(&task_id);
                        }
                    }
                }
            }
        }
    }

    fn validate(&self, event: &TimelineEvent) -> Result<LifecycleFold, TimelineError> {
        if self.subagent_result_recorded {
            return Err(TimelineError::SubagentTimelineEnded);
        }
        if event.version != TIMELINE_SCHEMA_VERSION {
            return Err(TimelineError::UnsupportedVersion {
                expected: TIMELINE_SCHEMA_VERSION,
                actual: event.version,
            });
        }
        let expected = self.events.len() as u64;
        if event.seq.get() != expected {
            return Err(TimelineError::NonContiguousSeq {
                expected,
                actual: event.seq.get(),
            });
        }
        if event.at_ms < 0 {
            return Err(TimelineError::InvalidTimestamp);
        }
        if let TimelineEventKind::Turn(TurnEvent::Started { identity, .. }) = &event.kind {
            let goal_owner_is_valid = match (
                identity.goal_id.as_deref(),
                identity.goal_definition_revision,
            ) {
                (None, None) => identity.origin != "goal_continuation",
                (Some(goal_id), Some(revision)) => {
                    valid_notification_identifier(goal_id) && revision > 0
                }
                _ => false,
            };
            if !goal_owner_is_valid {
                return Err(TimelineError::InvalidTurnIdentity);
            }
        }

        let mut lifecycle = self.lifecycle.clone();
        lifecycle.accept(&event.kind)?;
        if let TimelineEventKind::Compaction(CompactionEvent::Summary {
            id,
            input_ref,
            result_ref,
            target,
            summary_chars,
            ..
        }) = &event.kind
        {
            input_ref.validate()?;
            result_ref.validate()?;
            crate::validate_sideband_id(&result_ref.timeline_id)?;
            if result_ref.first_seq != result_ref.last_seq || *summary_chars == 0 {
                return Err(TimelineError::InvalidCompactionSummary(id.clone()));
            }
            let spawn = self.events.iter().find_map(|event| match &event.kind {
                TimelineEventKind::Sideband(spawn)
                    if spawn.sideband_id == result_ref.timeline_id =>
                {
                    Some(spawn)
                }
                _ => None,
            });
            if !spawn.is_some_and(|spawn| {
                spawn.purpose == crate::SidebandPurpose::CompactionSummary
                    && spawn.source_refs.iter().any(|source| source == input_ref)
            }) {
                return Err(TimelineError::InvalidCompactionSummary(id.clone()));
            }
            self.validate_surface_range(target)?;
        }
        if let TimelineEventKind::ImageProjection(projection) = &event.kind {
            let replacement_count = projection.tool_calls.iter().try_fold(
                projection.shadows.len(),
                |count, tool_call| {
                    count
                        .checked_add(1)
                        .and_then(|count| count.checked_add(tool_call.carrier_sources.len()))
                },
            );
            if !projection.trigger_runtime.is_valid()
                || projection.source_revision != self.surface_revision
                || projection.shadows.is_empty()
                || replacement_count.is_none_or(|count| count > u32::MAX as usize)
            {
                return Err(TimelineError::InvalidImageProjection);
            }
            let (branch_ids, branch_items) = self.branch_transcript_with_ids();
            let groups = sampling_types::conversation::conversation_image_groups(&branch_items)
                .into_iter()
                .filter_map(|group| {
                    branch_ids
                        .get(group.item_index)
                        .copied()
                        .map(|source| (source, group))
                })
                .collect::<BTreeMap<_, _>>();
            let mut sources = BTreeSet::new();
            let mut expected_tool_calls =
                BTreeMap::<SurfaceId, (BTreeSet<String>, Vec<SurfaceId>)>::new();
            for shadow in &projection.shadows {
                let group = groups.get(&shadow.source);
                if !sources.insert(shadow.source)
                    || shadow.replacement.trim().is_empty()
                    || !group.is_some_and(|group| {
                        group.fingerprint == shadow.fingerprint
                            && group.image_count() == shadow.image_count
                    })
                {
                    return Err(TimelineError::InvalidImageProjection);
                }
                let ImageShadowSource::Description { result_ref } = &shadow.provenance;
                let valid_ref = result_ref.validate().is_ok()
                    && result_ref.first_seq == result_ref.last_seq
                    && self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::Sideband(spawn)
                                if spawn.sideband_id == result_ref.timeline_id
                                    && spawn.purpose == crate::SidebandPurpose::ImageDescription
                                    && spawn.source_refs.iter().any(|source| {
                                        source.first_seq <= shadow.source.event.get()
                                            && shadow.source.event.get() <= source.last_seq
                                    })
                        )
                    });
                if !valid_ref {
                    return Err(TimelineError::InvalidImageProjection);
                }
                if let Some(tool_call) = group.and_then(|group| group.tool_call.as_ref()) {
                    let Some(source) = branch_ids.get(tool_call.item_index).copied() else {
                        return Err(TimelineError::InvalidImageProjection);
                    };
                    expected_tool_calls
                        .entry(source)
                        .or_default()
                        .0
                        .insert(tool_call.tool_call_id.clone());
                }
            }
            for (source, (_, carrier_sources)) in &mut expected_tool_calls {
                let Some(assistant_index) = branch_ids.iter().position(|id| id == source) else {
                    return Err(TimelineError::InvalidImageProjection);
                };
                if !branch_items
                    .get(assistant_index)
                    .is_some_and(|item| matches!(item, ConversationItem::Assistant(_)))
                {
                    return Err(TimelineError::InvalidImageProjection);
                }
                let Some(sources) =
                    sampling_types::conversation::assistant_response_carrier_indices(
                        &branch_items,
                        assistant_index,
                    )
                    .into_iter()
                    .map(|index| branch_ids.get(index).copied())
                    .collect::<Option<Vec<_>>>()
                else {
                    return Err(TimelineError::InvalidImageProjection);
                };
                *carrier_sources = sources;
            }
            let mut projected_tool_calls =
                BTreeMap::<SurfaceId, (BTreeSet<String>, Vec<SurfaceId>)>::new();
            for tool_call in &projection.tool_calls {
                if tool_call.tool_call_ids.is_empty()
                    || projected_tool_calls.contains_key(&tool_call.source)
                    || tool_call
                        .carrier_sources
                        .iter()
                        .copied()
                        .collect::<BTreeSet<_>>()
                        .len()
                        != tool_call.carrier_sources.len()
                {
                    return Err(TimelineError::InvalidImageProjection);
                }
                let ids = tool_call
                    .tool_call_ids
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                if ids.len() != tool_call.tool_call_ids.len() {
                    return Err(TimelineError::InvalidImageProjection);
                }
                projected_tool_calls
                    .insert(tool_call.source, (ids, tool_call.carrier_sources.clone()));
            }
            if projected_tool_calls != expected_tool_calls {
                return Err(TimelineError::InvalidImageProjection);
            }
        }
        if let TimelineEventKind::Messages(messages) = &event.kind {
            if messages.cause == MessageCause::ContextRebuild
                && (self.next_prompt_index() != 0
                    || self
                        .events
                        .iter()
                        .any(|event| matches!(event.kind, TimelineEventKind::ImageProjection(_))))
            {
                return Err(TimelineError::ContextRebuildAfterTurn);
            }
            self.validate_messages(messages)?;
        }
        if let TimelineEventKind::Notification(notification) = &event.kind {
            self.validate_notification(notification)?;
        }
        if let TimelineEventKind::Control(control) = &event.kind {
            let retired = control
                .retired_context_layers
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if (!control.model_contexts.is_empty()
                && !matches!(self.surface.first(), Some(ConversationItem::System(_))))
                || retired.len() != control.retired_context_layers.len()
                || control
                    .model_contexts
                    .iter()
                    .any(|context| retired.contains(&context.layer))
                || control
                    .model_contexts
                    .iter()
                    .any(|context| !is_valid_control_context(context))
            {
                return Err(TimelineError::InvalidControlContext);
            }
            let active = self.active_control_contexts();
            for context in &control.model_contexts {
                if context.activation == ControlContextActivation::Reprojection {
                    let current = active.get(&context.layer);
                    if current.is_none()
                        || current
                            .is_some_and(|current| self.surface_ids.contains(&current.surface_id))
                    {
                        return Err(TimelineError::InvalidControlReprojection);
                    }
                }
            }
        }
        if let TimelineEventKind::Sideband(sideband) = &event.kind {
            sideband.validate()?;
            if self.events.iter().any(|event| {
                matches!(
                    &event.kind,
                    TimelineEventKind::Sideband(existing)
                        if existing.sideband_id == sideband.sideband_id
                )
            }) {
                return Err(TimelineError::DuplicateSidebandSpawn(
                    sideband.sideband_id.clone(),
                ));
            }
            for source_ref in &sideband.source_refs {
                if source_ref.last_seq >= event.seq.get() {
                    return Err(TimelineError::InvalidSideband(
                        crate::SidebandError::FutureInputRef {
                            last: source_ref.last_seq,
                            spawn: event.seq.get(),
                        },
                    ));
                }
            }
        }
        if let TimelineEventKind::SessionTitle(title) = &event.kind {
            let normalized = title.title.trim();
            if normalized.is_empty() || normalized.chars().count() > 160 {
                return Err(TimelineError::InvalidSessionTitle);
            }
            match &title.source {
                SessionTitleSource::User => {}
                SessionTitleSource::Generated {
                    sideband_id,
                    result_seq,
                } => {
                    if crate::validate_sideband_id(sideband_id).is_err() || *result_seq == 0 {
                        return Err(TimelineError::InvalidSessionTitleSource);
                    }
                    if self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::SessionTitle(SessionTitleEvent {
                                source: SessionTitleSource::User,
                                ..
                            })
                        )
                    }) {
                        return Err(TimelineError::GeneratedTitleAfterUserTitle);
                    }
                    if !self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::Sideband(spawn)
                                if spawn.sideband_id == *sideband_id
                                    && spawn.purpose == crate::SidebandPurpose::SessionTitle
                        )
                    }) {
                        return Err(TimelineError::InvalidSessionTitleSource);
                    }
                }
                SessionTitleSource::Fallback {
                    sideband_id,
                    terminal_seq,
                } => {
                    if crate::validate_sideband_id(sideband_id).is_err() || *terminal_seq == 0 {
                        return Err(TimelineError::InvalidSessionTitleSource);
                    }
                    if self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::SessionTitle(SessionTitleEvent {
                                source: SessionTitleSource::User,
                                ..
                            })
                        )
                    }) {
                        return Err(TimelineError::GeneratedTitleAfterUserTitle);
                    }
                    if !self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::Sideband(spawn)
                                if spawn.sideband_id == *sideband_id
                                    && spawn.purpose == crate::SidebandPurpose::SessionTitle
                        )
                    }) {
                        return Err(TimelineError::InvalidSessionTitleSource);
                    }
                }
            }
        }
        if let TimelineEventKind::Subagent(subagent) = &event.kind {
            match subagent {
                SubagentEvent::Spawned(spawn) => {
                    if spawn.subagent_id.trim().is_empty()
                        || spawn.child_session_id.trim().is_empty()
                        || spawn.security_parent_session_id.trim().is_empty()
                        || spawn.subagent_type.trim().is_empty()
                        || spawn.description.trim().is_empty()
                        || spawn.prompt.trim().is_empty()
                        || spawn.child_cwd.trim().is_empty()
                        || spawn.effective_model_id.trim().is_empty()
                        || !spawn.model_transport_key.is_valid()
                    {
                        return Err(TimelineError::InvalidSubagent);
                    }
                    if let Some(source_ref) = &spawn.source_ref {
                        source_ref.validate()?;
                    }
                    if [
                        spawn.parent_prompt_id.as_deref(),
                        spawn.capability_mode.as_deref(),
                        spawn.permission_mode.as_deref(),
                        spawn.effective_permission_mode.as_deref(),
                        spawn.workflow_run_id.as_deref(),
                        spawn.goal_id.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|value| value.trim().is_empty())
                    {
                        return Err(TimelineError::InvalidSubagent);
                    }
                    if !matches!(
                        (spawn.goal_id.as_deref(), spawn.goal_definition_revision),
                        (None, None) | (Some(_), Some(1..))
                    ) {
                        return Err(TimelineError::InvalidSubagent);
                    }
                    if let Some(run_id) = &spawn.workflow_run_id {
                        let workflow = self
                            .lifecycle
                            .workflows
                            .get(run_id)
                            .ok_or_else(|| TimelineError::WorkflowNotFound(run_id.clone()))?;
                        if workflow.closed {
                            return Err(TimelineError::WorkflowAlreadyClosed(run_id.clone()));
                        }
                        if !workflow.open {
                            return Err(TimelineError::WorkflowNotOpen(run_id.clone()));
                        }
                    }
                    if self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::Subagent(SubagentEvent::Spawned(existing))
                                if existing.subagent_id == spawn.subagent_id
                        )
                    }) {
                        return Err(TimelineError::DuplicateSubagentSpawn(
                            spawn.subagent_id.clone(),
                        ));
                    }
                    if self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::Subagent(SubagentEvent::Spawned(existing))
                                if existing.child_session_id == spawn.child_session_id
                        )
                    }) {
                        return Err(TimelineError::DuplicateSubagentChild(
                            spawn.child_session_id.clone(),
                        ));
                    }
                }
                SubagentEvent::Ended(end) => {
                    let spawn = self.events.iter().find_map(|event| match &event.kind {
                        TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                            if spawn.subagent_id == end.subagent_id =>
                        {
                            Some(spawn)
                        }
                        _ => None,
                    });
                    let Some(spawn) = spawn else {
                        return Err(TimelineError::SubagentNotOpen(end.subagent_id.clone()));
                    };
                    if spawn.child_session_id != end.child_session_id {
                        return Err(TimelineError::SubagentChildMismatch);
                    }
                    if let Some(result_ref) = &end.result_ref {
                        result_ref.validate()?;
                        if result_ref.timeline_id != end.child_session_id
                            || result_ref.first_seq != result_ref.last_seq
                        {
                            return Err(TimelineError::SubagentChildMismatch);
                        }
                    } else if end.outcome == SubagentOutcome::Completed {
                        return Err(TimelineError::InvalidSubagent);
                    }
                    if self.events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            TimelineEventKind::Subagent(SubagentEvent::Ended(existing))
                                if existing.subagent_id == end.subagent_id
                        )
                    }) {
                        return Err(TimelineError::SubagentAlreadyEnded(end.subagent_id.clone()));
                    }
                    let error_valid = match end.outcome {
                        SubagentOutcome::Completed => end.error.is_none(),
                        SubagentOutcome::Failed | SubagentOutcome::Cancelled => end
                            .error
                            .as_deref()
                            .is_some_and(|error| !error.trim().is_empty()),
                    };
                    if !error_valid {
                        return Err(TimelineError::InvalidSubagent);
                    }
                }
            }
        }
        if let TimelineEventKind::SubagentSeed(seed) = &event.kind {
            let already_seeded = self
                .events
                .iter()
                .any(|event| matches!(event.kind, TimelineEventKind::SubagentSeed(_)));
            if already_seeded {
                return Err(TimelineError::DuplicateSubagentSeed);
            }
            if seed.parent_timeline_id.trim().is_empty()
                || seed.subagent_id.trim().is_empty()
                || seed.security_parent_session_id.trim().is_empty()
            {
                return Err(TimelineError::InvalidSubagent);
            }
            if let Some(source_ref) = &seed.source_ref {
                source_ref.validate()?;
            }
        }
        if let TimelineEventKind::SubagentResult(result) = &event.kind {
            let seed = self.events.iter().find_map(|event| match &event.kind {
                TimelineEventKind::SubagentSeed(seed) => Some(seed),
                _ => None,
            });
            let Some(seed) = seed else {
                return Err(TimelineError::MissingSubagentSeed);
            };
            if seed.subagent_id != result.subagent_id {
                return Err(TimelineError::SubagentSeedMismatch);
            }
            let error_valid = match result.outcome {
                SubagentOutcome::Completed => result.error.is_none(),
                SubagentOutcome::Failed | SubagentOutcome::Cancelled => result
                    .error
                    .as_deref()
                    .is_some_and(|error| !error.trim().is_empty()),
            };
            let output_valid = result.output_ref.as_deref().is_none_or(|output_ref| {
                output_ref
                    .strip_prefix("artifact:subagent-output:blake3:")
                    .is_some_and(|hash| {
                        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
            });
            if !error_valid || !output_valid {
                return Err(TimelineError::InvalidSubagent);
            }
        }
        Ok(lifecycle)
    }

    fn validate_notification(&self, notification: &NotificationEvent) -> Result<(), TimelineError> {
        match notification {
            NotificationEvent::Received {
                id,
                owner_session_id,
                source,
                source_version,
                payload_ref,
            } => {
                if !valid_notification_identifier(id)
                    || !valid_notification_identifier(owner_session_id)
                    || !valid_notification_identifier(source.subject_id())
                    || payload_ref.bytes == 0
                    || payload_ref.bytes > MAX_NOTIFICATION_PAYLOAD_BYTES
                    || !valid_blake3(&payload_ref.blake3)
                    || !valid_notification_source_version(source, source_version)
                    || !valid_notification_owner(&source.owner())
                    || notification_id(owner_session_id, source, source_version)
                        .ok()
                        .as_deref()
                        != Some(id.as_str())
                {
                    return Err(TimelineError::InvalidNotification);
                }
                if self.received_notification_ids.contains(id) {
                    return Err(TimelineError::InvalidNotification);
                }
                if self
                    .received_notification_id(source, source_version)
                    .is_some()
                {
                    return Err(TimelineError::DuplicateNotificationSource);
                }
            }
            NotificationEvent::Consumed {
                notification_ids,
                turn,
                input,
            } => {
                if self.lifecycle.active_turn != Some(*turn)
                    || notification_ids.is_empty()
                    || notification_ids.len() > u32::MAX as usize
                {
                    return Err(TimelineError::InvalidNotification);
                }
                let mut unique = BTreeSet::new();
                for id in notification_ids {
                    if !unique.insert(id) {
                        return Err(TimelineError::InvalidNotification);
                    }
                    if !self.received_notification_ids.contains(id) {
                        return Err(TimelineError::NotificationNotFound(id.clone()));
                    }
                    if !self.pending_notifications.contains_key(id) {
                        return Err(TimelineError::NotificationAlreadyConsumed(id.clone()));
                    }
                }
                if let Some(input) = input
                    && valid_notification_input(input)
                {
                    let plan_receipts = notification_ids
                        .iter()
                        .filter_map(|id| {
                            let notification = self.pending_notifications.get(id)?;
                            matches!(notification.source.owner(), NotificationOwner::Plan { .. })
                                .then_some(notification.source.identity())
                        })
                        .collect::<Vec<_>>();
                    if !plan_receipts.is_empty() {
                        let plan_turn = self.events.iter().rev().any(|event| {
                            matches!(
                                &event.kind,
                                TimelineEventKind::Turn(TurnEvent::Started { id, identity, .. })
                                    if id == turn && identity.origin == "plan_handoff"
                            )
                        });
                        let exact_plan_batch = plan_receipts.len() == notification_ids.len()
                            && plan_receipts.windows(2).all(|pair| pair[0] == pair[1]);
                        if !plan_turn || !exact_plan_batch {
                            return Err(TimelineError::InvalidNotification);
                        }
                    }
                } else if let Some(input) = input {
                    let Some(goal_id) = valid_goal_notification_input(input) else {
                        return Err(TimelineError::InvalidNotification);
                    };
                    let goal_turn_revision = self.events.iter().rev().find_map(|event| {
                        let TimelineEventKind::Turn(TurnEvent::Started { id, identity, .. }) =
                            &event.kind
                        else {
                            return None;
                        };
                        (id == turn
                            && identity.origin == "goal_continuation"
                            && identity.goal_id.as_deref() == Some(goal_id))
                        .then_some(identity.goal_definition_revision)
                        .flatten()
                    });
                    let receipts_match = notification_ids.iter().all(|id| {
                        self.pending_notifications
                            .get(id)
                            .is_some_and(|notification| {
                                matches!(
                                    notification.source.owner(),
                                    NotificationOwner::Goal {
                                        goal_id: owner_goal_id,
                                        definition_revision: owner_revision,
                                    } if owner_goal_id == goal_id
                                        && goal_turn_revision == Some(owner_revision)
                                )
                            })
                    });
                    if goal_turn_revision.is_none() || !receipts_match {
                        return Err(TimelineError::InvalidNotification);
                    }
                }
            }
            NotificationEvent::Dismissed {
                notification_ids,
                reason,
            } => {
                if notification_ids.is_empty() || notification_ids.len() > u32::MAX as usize {
                    return Err(TimelineError::InvalidNotification);
                }
                let mut unique = BTreeSet::new();
                for id in notification_ids {
                    let reason_matches_owner = self.pending_notifications.get(id).is_some_and(
                        |notification| match reason {
                            NotificationDismissReason::GoalOwnedAutostart {
                                goal_id,
                                definition_revision,
                            } => {
                                valid_notification_identifier(goal_id)
                                    && matches!(
                                        notification.source.owner(),
                                        NotificationOwner::Goal {
                                            goal_id: owner_goal_id,
                                            definition_revision: owner_revision,
                                        } if owner_goal_id == *goal_id
                                            && owner_revision == *definition_revision
                                    )
                            }
                            NotificationDismissReason::PlanSuperseded {
                                artifact_hash,
                                artifact_revision,
                                handoff,
                            } => {
                                valid_blake3(artifact_hash)
                                    && matches!(
                                        notification.source.owner(),
                                        NotificationOwner::Plan {
                                            artifact_hash: owner_hash,
                                            artifact_revision: owner_revision,
                                            handoff: owner_handoff,
                                        } if owner_hash == *artifact_hash
                                            && owner_revision == *artifact_revision
                                            && owner_handoff == *handoff
                                    )
                            }
                        },
                    );
                    if !unique.insert(id)
                        || !self.received_notification_ids.contains(id)
                        || !reason_matches_owner
                    {
                        return Err(TimelineError::InvalidNotification);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_messages(&self, messages: &MessageEvent) -> Result<(), TimelineError> {
        let _ = u32::try_from(messages.items.len()).map_err(|_| TimelineError::TooManyItems)?;
        match &messages.surface {
            SurfaceOp::Append => {
                if messages.items.is_empty() {
                    return Err(TimelineError::EmptyAppend);
                }
                let valid = match messages.cause {
                    MessageCause::Seed => {
                        self.events.iter().all(|event| {
                            matches!(
                                &event.kind,
                                TimelineEventKind::Messages(MessageEvent {
                                    cause: MessageCause::Seed,
                                    surface: SurfaceOp::Append,
                                    ..
                                })
                            )
                        }) && valid_system_layout(&messages.items)
                    }
                    MessageCause::MemoryContext => {
                        matches!(
                            messages.items.as_slice(),
                            [ConversationItem::User(user)]
                                if user.synthetic_reason
                                    == Some(SyntheticReason::MemoryContext)
                        ) && !messages.items[0].text_content().trim().is_empty()
                    }
                    MessageCause::DirectUser => self.valid_direct_user_append(&messages.items),
                    MessageCause::Interjection => self.valid_interjection_append(&messages.items),
                    MessageCause::User => messages.items.iter().all(|item| {
                        matches!(
                            item,
                            ConversationItem::User(user)
                                if user.permission_evidence.is_none()
                                    && !matches!(
                                        user.synthetic_reason,
                                        Some(SyntheticReason::Interjection)
                                    )
                                    && !matches!(
                                        user.synthetic_reason.as_ref(),
                                        Some(
                                            SyntheticReason::ProjectInstructions
                                                | SyntheticReason::SessionRules
                                                | SyntheticReason::MemoryContext
                                        )
                                    )
                        )
                    }),
                    MessageCause::Assistant => messages.items.iter().all(|item| {
                        matches!(
                            item,
                            ConversationItem::Assistant(_)
                                | ConversationItem::BackendToolCall(_)
                                | ConversationItem::Reasoning(_)
                        )
                    }),
                    MessageCause::ToolResult => messages
                        .items
                        .iter()
                        .all(|item| matches!(item, ConversationItem::ToolResult(_))),
                    MessageCause::ContextRebuild => {
                        self.surface.is_empty() && valid_system_layout(&messages.items)
                    }
                    MessageCause::IntegrityRepair
                    | MessageCause::Compaction
                    | MessageCause::ToolResultPrune
                    | MessageCause::Rewind => false,
                };
                if !valid {
                    return Err(TimelineError::InvalidMessageShape);
                }
            }
            SurfaceOp::Replace {
                start,
                end,
                shadowed,
            } => {
                let Some(start_index) = self.surface_ids.iter().position(|id| id == start) else {
                    return Err(TimelineError::StaleReplacementBoundary);
                };
                let Some(end_index) = self.surface_ids.iter().position(|id| id == end) else {
                    return Err(TimelineError::StaleReplacementBoundary);
                };
                if start_index > end_index {
                    return Err(TimelineError::ReversedReplacement);
                }
                if self.surface_ids[start_index..=end_index] != *shadowed {
                    return Err(TimelineError::IncompleteShadowSet);
                }
                let replaced = &self.surface[start_index..=end_index];
                let replaces_all = start_index == 0 && end_index + 1 == self.surface.len();
                if !replacement_preserves_system_head(&self.surface, start_index, &messages.items) {
                    return Err(TimelineError::InvalidMessageShape);
                }
                match messages.cause {
                    MessageCause::Compaction
                        if !messages.items.is_empty()
                            && valid_compaction_replacement(&messages.items) => {}
                    MessageCause::ToolResultPrune if replaces_all => {
                        validate_tool_result_prune(replaced, messages)?;
                    }
                    MessageCause::ContextRebuild if replaces_all => {}
                    MessageCause::IntegrityRepair
                        if replaces_all
                            && valid_integrity_repair(&self.surface, &messages.items) => {}
                    MessageCause::Rewind
                        if replaces_all && self.valid_rewind_replacement(&messages.items) => {}
                    MessageCause::Seed
                    | MessageCause::DirectUser
                    | MessageCause::Interjection
                    | MessageCause::User
                    | MessageCause::Assistant
                    | MessageCause::ToolResult
                    | MessageCause::IntegrityRepair
                    | MessageCause::Compaction
                    | MessageCause::ToolResultPrune
                    | MessageCause::MemoryContext
                    | MessageCause::ContextRebuild
                    | MessageCause::Rewind => {
                        return Err(TimelineError::InvalidMessageShape);
                    }
                }
            }
        }
        Ok(())
    }

    fn valid_direct_user_append(&self, items: &[ConversationItem]) -> bool {
        let Some(active) = self.lifecycle.active_turn else {
            return false;
        };
        let Some((identity, prompt_index)) =
            self.events
                .iter()
                .rev()
                .find_map(|event| match &event.kind {
                    TimelineEventKind::Turn(TurnEvent::Started {
                        id,
                        identity,
                        prompt_index,
                        ..
                    }) if *id == active => Some((identity, *prompt_index)),
                    _ => None,
                })
        else {
            return false;
        };
        matches!(
            items,
            [ConversationItem::User(user)]
                if identity.origin == "user"
                    && user.synthetic_reason.is_none()
                    && user.prompt_index == Some(prompt_index)
                    && matches!(
                        user.permission_evidence,
                        Some(PermissionEvidence::DirectUser { ref text })
                            if if text.trim().is_empty() {
                                user.content.iter().any(|part| {
                                    matches!(part, sampling_types::ContentPart::Image { .. })
                                })
                            } else {
                                user.content.iter().any(|part| {
                                    matches!(part, sampling_types::ContentPart::Text { text }
                                        if !text.trim().is_empty())
                                })
                            }
                    )
        )
    }

    fn valid_interjection_append(&self, items: &[ConversationItem]) -> bool {
        self.lifecycle.active_turn.is_some()
            && matches!(
                items,
                [ConversationItem::User(user)]
                    if user.synthetic_reason == Some(SyntheticReason::Interjection)
                        && user.prompt_index.is_none()
                        && matches!(
                            user.permission_evidence,
                            Some(PermissionEvidence::Interjection { ref text })
                                if if text.trim().is_empty() {
                                    user.content.iter().any(|part| {
                                        matches!(part, sampling_types::ContentPart::Image { .. })
                                    })
                                } else {
                                    user.content.iter().any(|part| {
                                        matches!(part, sampling_types::ContentPart::Text { text }
                                            if !text.trim().is_empty())
                                    })
                                }
                        )
            )
    }

    fn valid_rewind_replacement(&self, replacement: &[ConversationItem]) -> bool {
        (0..self.next_prompt_index()).any(|target| {
            self.rewind_surface(target)
                .is_ok_and(|expected| conversation_slices_match(&expected, replacement))
        })
    }

    fn validate_surface_range(&self, target: &SurfaceRange) -> Result<(), TimelineError> {
        let Some(start_index) = self.surface_ids.iter().position(|id| id == &target.start) else {
            return Err(TimelineError::StaleReplacementBoundary);
        };
        let Some(end_index) = self.surface_ids.iter().position(|id| id == &target.end) else {
            return Err(TimelineError::StaleReplacementBoundary);
        };
        if start_index > end_index {
            return Err(TimelineError::ReversedReplacement);
        }
        if self.surface_ids[start_index..=end_index] != target.shadowed {
            return Err(TimelineError::IncompleteShadowSet);
        }
        Ok(())
    }

    fn apply_messages(&mut self, event_seq: EventSeq, messages: &MessageEvent) {
        let item_count = u32::try_from(messages.items.len())
            .expect("message item capacity was checked during validation");
        match &messages.surface {
            SurfaceOp::Append => self.append_surface_items(event_seq, &messages.items),
            SurfaceOp::Replace { start, end, .. } => {
                let start_index = self
                    .surface_ids
                    .iter()
                    .position(|id| id == start)
                    .expect("replacement start was checked during validation");
                let end_index = self
                    .surface_ids
                    .iter()
                    .position(|id| id == end)
                    .expect("replacement end was checked during validation");
                self.surface
                    .splice(start_index..=end_index, messages.items.iter().cloned());
                self.surface_ids.splice(
                    start_index..=end_index,
                    (0..item_count).map(|item| SurfaceId {
                        event: event_seq,
                        item,
                    }),
                );
            }
        }
        if matches!(&messages.surface, SurfaceOp::Replace { .. }) {
            self.surface_revision = self.surface_revision.saturating_add(1);
        }
    }

    fn append_surface_items(&mut self, event_seq: EventSeq, items: &[ConversationItem]) {
        let item_count = u32::try_from(items.len())
            .expect("surface item capacity was checked during validation");
        self.surface.extend(items.iter().cloned());
        self.surface_ids
            .extend((0..item_count).map(|item| SurfaceId {
                event: event_seq,
                item,
            }));
        self.surface_revision = self.surface_revision.saturating_add(1);
    }
}

impl LifecycleFold {
    fn accept(&mut self, kind: &TimelineEventKind) -> Result<(), TimelineError> {
        match kind {
            TimelineEventKind::Turn(TurnEvent::Started { id, .. }) => {
                if let Some(active) = self.active_turn {
                    return Err(TimelineError::TurnAlreadyActive {
                        active,
                        actual: *id,
                    });
                }
                if !self.seen_turns.insert(*id) {
                    return Err(TimelineError::TurnAlreadySeen(*id));
                }
                self.active_turn = Some(*id);
            }
            TimelineEventKind::Turn(TurnEvent::Ended { id, .. }) => {
                if self.active_turn != Some(*id) {
                    return Err(TimelineError::TurnMismatch {
                        active: self.active_turn,
                        actual: *id,
                    });
                }
                if self.active_step.is_some()
                    || !self.open_requests.is_empty()
                    || !self.open_tools.is_empty()
                    || self.open_compaction.is_some()
                {
                    return Err(TimelineError::OpenChildren { boundary: "turn" });
                }
                self.active_turn = None;
            }
            TimelineEventKind::Step(StepEvent::Started { id }) => {
                if self.active_turn != Some(id.turn) {
                    return Err(TimelineError::TurnMismatch {
                        active: self.active_turn,
                        actual: id.turn,
                    });
                }
                if let Some(active) = self.active_step {
                    return Err(TimelineError::StepAlreadyActive {
                        active,
                        actual: *id,
                    });
                }
                if !self.seen_steps.insert(*id) {
                    return Err(TimelineError::StepAlreadySeen(*id));
                }
                self.active_step = Some(*id);
            }
            TimelineEventKind::Step(StepEvent::Ended { id, .. }) => {
                if self.active_step != Some(*id) {
                    return Err(TimelineError::StepMismatch {
                        active: self.active_step,
                        actual: *id,
                    });
                }
                if self.open_requests.values().any(|(_, step)| step == id)
                    || self.open_tools.values().any(|(_, step, _)| step == id)
                    || self.open_compaction.is_some()
                {
                    return Err(TimelineError::OpenChildren { boundary: "step" });
                }
                self.active_step = None;
            }
            TimelineEventKind::Request(RequestEvent::Started { id, turn, step, .. }) => {
                if self.active_turn != Some(*turn) {
                    return Err(TimelineError::TurnMismatch {
                        active: self.active_turn,
                        actual: *turn,
                    });
                }
                if self.active_step != Some(*step) {
                    return Err(TimelineError::StepMismatch {
                        active: self.active_step,
                        actual: *step,
                    });
                }
                if !self.seen_requests.insert(id.clone()) {
                    return Err(TimelineError::RequestAlreadyOpen(id.clone()));
                }
                if self
                    .open_requests
                    .insert(id.clone(), (*turn, *step))
                    .is_some()
                {
                    return Err(TimelineError::RequestAlreadyOpen(id.clone()));
                }
            }
            TimelineEventKind::Request(RequestEvent::Retrying { id, .. }) => {
                if !self.open_requests.contains_key(id) {
                    return Err(TimelineError::RequestNotOpen(id.clone()));
                }
            }
            TimelineEventKind::Request(RequestEvent::Completed { id, .. })
            | TimelineEventKind::Request(RequestEvent::Failed { id, .. })
            | TimelineEventKind::Request(RequestEvent::Cancelled { id, .. }) => {
                if self.open_requests.remove(id).is_none() {
                    return Err(TimelineError::RequestNotOpen(id.clone()));
                }
            }
            TimelineEventKind::Tool(ToolEvent::Started {
                call_id,
                turn,
                step,
                name,
                ..
            }) => {
                if self.active_turn != Some(*turn) {
                    return Err(TimelineError::TurnMismatch {
                        active: self.active_turn,
                        actual: *turn,
                    });
                }
                if self.active_step != Some(*step) {
                    return Err(TimelineError::StepMismatch {
                        active: self.active_step,
                        actual: *step,
                    });
                }
                if !self.seen_tools.insert(call_id.clone()) {
                    return Err(TimelineError::ToolAlreadyOpen(call_id.clone()));
                }
                if self
                    .open_tools
                    .insert(call_id.clone(), (*turn, *step, name.clone()))
                    .is_some()
                {
                    return Err(TimelineError::ToolAlreadyOpen(call_id.clone()));
                }
            }
            TimelineEventKind::Tool(ToolEvent::Completed { call_id, name, .. }) => {
                let Some((_, _, expected)) = self.open_tools.remove(call_id) else {
                    return Err(TimelineError::ToolNotOpen(call_id.clone()));
                };
                if expected != *name {
                    return Err(TimelineError::ToolNameMismatch {
                        call_id: call_id.clone(),
                        expected,
                        actual: name.clone(),
                    });
                }
            }
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) => {
                self.open_subagents.insert(
                    spawn.subagent_id.clone(),
                    OpenSubagent {
                        workflow_run_id: spawn.workflow_run_id.clone(),
                    },
                );
            }
            TimelineEventKind::Subagent(SubagentEvent::Ended(end)) => {
                self.open_subagents.remove(&end.subagent_id);
            }
            TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id,
                execution_epoch,
                name,
                objective,
                script_hash,
                args_hash,
                initial_manifest,
            }) => {
                if !valid_workflow_run_id(run_id)
                    || *execution_epoch != 0
                    || name.trim().is_empty()
                    || objective.trim().is_empty()
                    || !valid_workflow_content_hash(script_hash)
                    || !valid_workflow_content_hash(args_hash)
                    || !valid_workflow_initial_manifest(initial_manifest)
                {
                    return Err(TimelineError::InvalidWorkflow);
                }
                if self.workflows.contains_key(run_id) {
                    return Err(TimelineError::DuplicateWorkflowSpawn(run_id.clone()));
                }
                self.workflows.insert(
                    run_id.clone(),
                    WorkflowFold {
                        execution_epoch: *execution_epoch,
                        open: true,
                        closed: false,
                    },
                );
            }
            TimelineEventKind::Workflow(WorkflowEvent::Resumed {
                run_id,
                execution_epoch,
            }) => {
                if !valid_workflow_run_id(run_id) {
                    return Err(TimelineError::InvalidWorkflow);
                }
                let lifecycle = self
                    .workflows
                    .get_mut(run_id)
                    .ok_or_else(|| TimelineError::WorkflowNotFound(run_id.clone()))?;
                if lifecycle.open {
                    return Err(TimelineError::WorkflowAlreadyOpen(run_id.clone()));
                }
                if lifecycle.closed {
                    return Err(TimelineError::WorkflowAlreadyClosed(run_id.clone()));
                }
                if lifecycle.execution_epoch.checked_add(1) != Some(*execution_epoch) {
                    return Err(TimelineError::WorkflowEpochMismatch {
                        run_id: run_id.clone(),
                        previous: lifecycle.execution_epoch,
                        actual: *execution_epoch,
                    });
                }
                lifecycle.execution_epoch = *execution_epoch;
                lifecycle.open = true;
            }
            TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id,
                execution_epoch,
                status,
                handoff,
                message,
                ..
            }) => {
                if !valid_workflow_run_id(run_id)
                    || !valid_workflow_turn_handoff(*status, *handoff, false)
                    || message
                        .as_deref()
                        .is_some_and(|message| message.trim().is_empty())
                {
                    return Err(TimelineError::InvalidWorkflow);
                }
                if self
                    .open_subagents
                    .values()
                    .any(|subagent| subagent.workflow_run_id.as_deref() == Some(run_id.as_str()))
                {
                    return Err(TimelineError::OpenChildren {
                        boundary: "workflow",
                    });
                }
                let lifecycle = self
                    .workflows
                    .get_mut(run_id)
                    .ok_or_else(|| TimelineError::WorkflowNotFound(run_id.clone()))?;
                if lifecycle.closed {
                    return Err(TimelineError::WorkflowAlreadyClosed(run_id.clone()));
                }
                if !lifecycle.open {
                    return Err(TimelineError::WorkflowNotOpen(run_id.clone()));
                }
                if *execution_epoch != lifecycle.execution_epoch {
                    return Err(TimelineError::WorkflowEpochMismatch {
                        run_id: run_id.clone(),
                        previous: lifecycle.execution_epoch,
                        actual: *execution_epoch,
                    });
                }
                lifecycle.open = false;
                lifecycle.closed = matches!(
                    status,
                    WorkflowExecutionStatus::Interrupted
                        | WorkflowExecutionStatus::Complete
                        | WorkflowExecutionStatus::Cancelled
                );
            }
            TimelineEventKind::Workflow(WorkflowEvent::Closed {
                run_id,
                execution_epoch,
                status,
                handoff,
                message,
                ..
            }) => {
                if !valid_workflow_run_id(run_id)
                    || !matches!(
                        status,
                        WorkflowExecutionStatus::Interrupted | WorkflowExecutionStatus::Cancelled
                    )
                    || !valid_workflow_turn_handoff(*status, *handoff, true)
                    || message
                        .as_deref()
                        .is_some_and(|message| message.trim().is_empty())
                {
                    return Err(TimelineError::InvalidWorkflow);
                }
                if self
                    .open_subagents
                    .values()
                    .any(|subagent| subagent.workflow_run_id.as_deref() == Some(run_id.as_str()))
                {
                    return Err(TimelineError::OpenChildren {
                        boundary: "workflow",
                    });
                }
                let lifecycle = self
                    .workflows
                    .get_mut(run_id)
                    .ok_or_else(|| TimelineError::WorkflowNotFound(run_id.clone()))?;
                if lifecycle.open || lifecycle.closed {
                    return Err(if lifecycle.closed {
                        TimelineError::WorkflowAlreadyClosed(run_id.clone())
                    } else {
                        TimelineError::WorkflowAlreadyOpen(run_id.clone())
                    });
                }
                if *execution_epoch != lifecycle.execution_epoch {
                    return Err(TimelineError::WorkflowEpochMismatch {
                        run_id: run_id.clone(),
                        previous: lifecycle.execution_epoch,
                        actual: *execution_epoch,
                    });
                }
                lifecycle.closed = true;
            }
            TimelineEventKind::Compaction(CompactionEvent::Started {
                id, source_items, ..
            }) => {
                if !self.seen_compactions.insert(id.clone()) {
                    return Err(TimelineError::CompactionAlreadySeen(id.clone()));
                }
                if let Some(active) = self.open_compaction.replace(OpenCompaction {
                    id: id.clone(),
                    source_items: *source_items,
                    summaries: 0,
                    replacements: 0,
                    target: None,
                }) {
                    return Err(TimelineError::CompactionAlreadyOpen(active.id));
                }
            }
            TimelineEventKind::Compaction(CompactionEvent::Summary { id, target, .. }) => {
                let Some(open) = self.open_compaction.as_mut() else {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                };
                if open.id != *id {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                }
                if open.summaries != 0 {
                    return Err(TimelineError::DuplicateCompactionSummary(id.clone()));
                }
                open.summaries = 1;
                open.target = Some(target.clone());
            }
            TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. }) => {
                let Some(open) = self.open_compaction.as_ref() else {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                };
                if open.id != *id {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                }
                if open.replacements != 1 {
                    return Err(TimelineError::MissingCompactionReplacement(id.clone()));
                }
                self.open_compaction = None;
            }
            TimelineEventKind::Compaction(CompactionEvent::Failed { id, .. }) => {
                let Some(open) = self.open_compaction.as_ref() else {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                };
                if open.id != *id {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                }
                if open.replacements != 0 {
                    return Err(TimelineError::FailedCompactionHasReplacement(id.clone()));
                }
                self.open_compaction = None;
            }
            TimelineEventKind::Messages(MessageEvent {
                cause: MessageCause::Compaction,
                surface:
                    SurfaceOp::Replace {
                        start,
                        end,
                        shadowed,
                    },
                ..
            }) => {
                let Some(open) = self.open_compaction.as_mut() else {
                    return Err(TimelineError::CompactionReplacementNotOpen);
                };
                if open.summaries != 1 {
                    return Err(TimelineError::CompactionReplacementBeforeSummary);
                }
                if open.replacements != 0 {
                    return Err(TimelineError::DuplicateCompactionReplacement(
                        open.id.clone(),
                    ));
                }
                if open.target.as_ref()
                    != Some(&SurfaceRange {
                        start: *start,
                        end: *end,
                        shadowed: shadowed.clone(),
                    })
                {
                    return Err(TimelineError::CompactionTargetMismatch);
                }
                open.replacements = 1;
            }
            TimelineEventKind::Messages(MessageEvent {
                cause: MessageCause::Compaction,
                ..
            }) => return Err(TimelineError::CompactionReplacementNotOpen),
            TimelineEventKind::Messages(MessageEvent {
                surface: SurfaceOp::Replace { .. },
                ..
            }) if self.open_compaction.is_some() => {
                return Err(TimelineError::ReplacementDuringCompaction(
                    self.open_compaction
                        .as_ref()
                        .expect("guarded by is_some")
                        .id
                        .clone(),
                ));
            }
            TimelineEventKind::Messages(_)
            | TimelineEventKind::ImageProjection(_)
            | TimelineEventKind::Recovery(_)
            | TimelineEventKind::Observation(_)
            | TimelineEventKind::SessionTitle(_)
            | TimelineEventKind::Sideband(_)
            | TimelineEventKind::SubagentSeed(_)
            | TimelineEventKind::SubagentResult(_)
            | TimelineEventKind::Notification(_) => {}
            TimelineEventKind::Control(control) => {
                if let Some(previous) = self.control_revision
                    && control.revision <= previous
                {
                    return Err(TimelineError::NonMonotonicControlRevision {
                        previous,
                        actual: control.revision,
                    });
                }
                self.control_revision = Some(control.revision);
            }
        }
        Ok(())
    }
}

fn valid_notification_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_NOTIFICATION_ID_BYTES
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn valid_compaction_replacement(items: &[ConversationItem]) -> bool {
    matches!(
        items,
        [ConversationItem::User(user)]
            if user.synthetic_reason == Some(SyntheticReason::CompactionMeta)
                && user.permission_evidence.is_none()
                && user.goal_directive.is_none()
                && user.cwd_generation.is_none()
                && user.prior_turn_interrupt.is_none()
                && user.prompt_index.is_none()
                && !user.content.is_empty()
                && user.content.iter().all(|part| matches!(part, ContentPart::Text { .. }))
                && !items[0].text_content().trim().is_empty()
    )
}

fn valid_integrity_repair(before: &[ConversationItem], replacement: &[ConversationItem]) -> bool {
    let mut user_cancelled = before.to_vec();
    sampling_types::dedup_duplicate_tool_results(&mut user_cancelled);
    sampling_types::repair_dangling_tool_calls(
        &mut user_cancelled,
        DanglingToolCallReason::UserCancelled,
    );
    if conversation_slices_match(&user_cancelled, replacement) {
        return true;
    }

    let mut interrupted = before.to_vec();
    crate::compaction_utils::repair_history_with_reason(
        &mut interrupted,
        DanglingToolCallReason::ProcessInterrupted,
    );
    if conversation_slices_match(&interrupted, replacement) {
        return true;
    }

    let mut explicit = before.to_vec();
    crate::compaction_utils::repair_history(&mut explicit);
    conversation_slices_match(&explicit, replacement)
}

fn conversation_slices_match(left: &[ConversationItem], right: &[ConversationItem]) -> bool {
    match (serde_json::to_value(left), serde_json::to_value(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn valid_blake3(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_notification_source_version(
    source: &NotificationSource,
    version: &NotificationSourceVersion,
) -> bool {
    match (source, version) {
        (
            NotificationSource::MonitorProgress { .. }
            | NotificationSource::TaskStillRunning { .. },
            NotificationSourceVersion::Opaque { value },
        ) => valid_notification_identifier(value),
        (
            NotificationSource::WorkflowHandoff { handoff, .. },
            NotificationSourceVersion::Opaque { value },
        ) => *handoff != WorkflowTurnHandoff::None && valid_notification_identifier(value),
        (
            NotificationSource::TaskCompleted { .. } | NotificationSource::SubagentCompleted { .. },
            NotificationSourceVersion::Ordinal { value },
        ) => *value > 0,
        (
            NotificationSource::PlanHandoff {
                artifact_revision, ..
            },
            NotificationSourceVersion::Ordinal { value },
        ) => *value == *artifact_revision && *value > 0,
        _ => false,
    }
}

fn valid_notification_owner(owner: &NotificationOwner) -> bool {
    match owner {
        NotificationOwner::Session => true,
        NotificationOwner::Goal {
            goal_id,
            definition_revision,
        } => valid_notification_identifier(goal_id) && *definition_revision > 0,
        NotificationOwner::Plan {
            artifact_hash,
            artifact_revision,
            ..
        } => valid_blake3(artifact_hash) && *artifact_revision > 0,
    }
}

fn valid_notification_input(item: &ConversationItem) -> bool {
    let ConversationItem::User(user) = item else {
        return false;
    };
    matches!(
        user.synthetic_reason,
        Some(
            SyntheticReason::TaskCompleted
                | SyntheticReason::SubagentCompleted
                | SyntheticReason::NotificationDrain
        )
    ) && user.permission_evidence.is_none()
        && user.goal_directive.is_none()
        && user.cwd_generation.is_none()
        && user.prior_turn_interrupt.is_none()
        && user.prompt_index.is_some()
        && !user.content.is_empty()
        && user
            .content
            .iter()
            .all(|part| matches!(part, ContentPart::Text { .. }))
        && !item.text_content().trim().is_empty()
}

fn valid_goal_notification_input(item: &ConversationItem) -> Option<&str> {
    let ConversationItem::User(user) = item else {
        return None;
    };
    let tag = user.goal_directive.as_ref()?;
    (user.synthetic_reason == Some(SyntheticReason::SystemReminder)
        && user.permission_evidence.is_none()
        && user.cwd_generation.is_none()
        && user.prior_turn_interrupt.is_none()
        && user.prompt_index.is_some()
        && tag.definition_revision > 0
        && valid_notification_identifier(&tag.goal_id)
        && !user.content.is_empty()
        && user
            .content
            .iter()
            .all(|part| matches!(part, ContentPart::Text { .. }))
        && !item.text_content().trim().is_empty())
    .then_some(tag.goal_id.as_str())
}

fn is_valid_control_context(context: &ControlContext) -> bool {
    let ConversationItem::User(user) = &context.item else {
        return false;
    };
    let goal_tag_is_valid = match context.layer {
        ControlContextLayer::GoalDefinition => user.goal_directive.as_ref().is_some_and(|tag| {
            tag.definition_revision > 0 && valid_notification_identifier(&tag.goal_id)
        }),
        ControlContextLayer::AgentRole
        | ControlContextLayer::PlanPhase
        | ControlContextLayer::Behavior => user.goal_directive.is_none(),
    };
    user.synthetic_reason == Some(SyntheticReason::SystemReminder)
        && user.permission_evidence.is_none()
        && goal_tag_is_valid
        && user.cwd_generation.is_none()
        && user.prior_turn_interrupt.is_none()
        && user.prompt_index.is_none()
        && !user.content.is_empty()
        && user
            .content
            .iter()
            .all(|part| matches!(part, ContentPart::Text { .. }))
        && !context.item.text_content().trim().is_empty()
}

/// Fold the effective boundary of Control-owned model context.
///
/// A transition is durable immediately but cannot enter Surface until its
/// authority boundary closes. AgentRole and GoalDefinition wait for the active
/// step so neither can split an assistant tool call from its result; Behavior
/// waits for the whole turn because it owns turn admission. Intermediate
/// transitions remain ledger facts, while only the latest pending context per
/// layer becomes model-visible.
fn fold_control_context_activation(
    active_turn: &mut bool,
    active_step: &mut bool,
    pending: &mut BTreeMap<ControlContextLayer, (EventSeq, ConversationItem)>,
    event: &TimelineEvent,
) -> Vec<(EventSeq, ConversationItem)> {
    match &event.kind {
        TimelineEventKind::Turn(TurnEvent::Started { .. }) => {
            *active_turn = true;
            Vec::new()
        }
        TimelineEventKind::Step(StepEvent::Started { .. }) => {
            *active_step = true;
            Vec::new()
        }
        TimelineEventKind::Control(control) => {
            for layer in &control.retired_context_layers {
                pending.remove(layer);
            }
            let mut projected = Vec::new();
            for context in &control.model_contexts {
                if control_transition_waits_for_boundary(*active_turn, *active_step, context) {
                    pending.insert(context.layer, (event.seq, context.item.clone()));
                } else {
                    projected.push((event.seq, context.item.clone()));
                }
            }
            projected
        }
        TimelineEventKind::Step(StepEvent::Ended { .. }) => {
            *active_step = false;
            take_pending_step_control_contexts(pending)
        }
        TimelineEventKind::Turn(TurnEvent::Ended { .. }) => {
            *active_turn = false;
            *active_step = false;
            take_pending_control_contexts(pending)
        }
        _ => Vec::new(),
    }
}

pub(crate) fn control_transition_waits_for_boundary(
    active_turn: bool,
    active_step: bool,
    context: &ControlContext,
) -> bool {
    if context.activation != ControlContextActivation::Transition {
        return false;
    }
    match context.layer {
        ControlContextLayer::AgentRole
        | ControlContextLayer::GoalDefinition
        | ControlContextLayer::PlanPhase => active_step,
        ControlContextLayer::Behavior => active_turn,
    }
}

fn take_pending_step_control_contexts(
    pending: &mut BTreeMap<ControlContextLayer, (EventSeq, ConversationItem)>,
) -> Vec<(EventSeq, ConversationItem)> {
    let mut contexts = [
        ControlContextLayer::AgentRole,
        ControlContextLayer::GoalDefinition,
        ControlContextLayer::PlanPhase,
    ]
    .into_iter()
    .filter_map(|layer| pending.remove(&layer))
    .collect::<Vec<_>>();
    contexts.sort_by_key(|(source, _)| *source);
    contexts
}

fn take_pending_control_contexts(
    pending: &mut BTreeMap<ControlContextLayer, (EventSeq, ConversationItem)>,
) -> Vec<(EventSeq, ConversationItem)> {
    let mut contexts = std::mem::take(pending).into_values().collect::<Vec<_>>();
    contexts.sort_by_key(|(source, _)| *source);
    contexts
}

#[derive(Debug, Clone)]
struct BranchProvenance {
    id: SurfaceId,
    value: ConversationItem,
    leaves: Vec<SurfaceId>,
}

#[derive(Default)]
struct BranchFold {
    surface: Vec<BranchProvenance>,
    leaf_order: Vec<SurfaceId>,
    leaf_values: BTreeMap<SurfaceId, ConversationItem>,
    leaf_birth: BTreeMap<SurfaceId, EventSeq>,
    leaf_is_message: BTreeMap<SurfaceId, bool>,
    unloaded: BTreeSet<SurfaceId>,
}

fn fold_branch_provenance(timeline: &Timeline) -> BranchFold {
    let completed = timeline
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. }) => {
                Some(id.as_str())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut fold = BranchFold::default();
    let mut active_turn = false;
    let mut active_step = false;
    let mut pending_control_contexts = BTreeMap::new();

    for event in &timeline.events {
        if let Some(items) = event.appended_message_items() {
            append_branch_leaves(&mut fold, event.seq, items, true);
            continue;
        }
        let activated = fold_control_context_activation(
            &mut active_turn,
            &mut active_step,
            &mut pending_control_contexts,
            event,
        );
        for (source, value) in activated {
            append_branch_leaves(&mut fold, source, std::slice::from_ref(&value), false);
        }
        match &event.kind {
            TimelineEventKind::Messages(messages) => match &messages.surface {
                SurfaceOp::Append => {}
                SurfaceOp::Replace { start, end, .. } => {
                    if matches!(
                        messages.cause,
                        MessageCause::Rewind | MessageCause::ContextRebuild
                    ) {
                        reset_branch(&mut fold, event.seq, &messages.items);
                        continue;
                    }
                    let Some(start_index) =
                        fold.surface.iter().position(|entry| entry.id == *start)
                    else {
                        continue;
                    };
                    let Some(end_index) = fold.surface.iter().position(|entry| entry.id == *end)
                    else {
                        continue;
                    };
                    if start_index > end_index {
                        continue;
                    }
                    let replaced = fold.surface[start_index..=end_index].to_vec();
                    let leaves = replacement_branch_leaves(
                        event.seq,
                        messages.cause,
                        &replaced,
                        &messages.items,
                    );
                    let replacement = messages
                        .items
                        .iter()
                        .cloned()
                        .zip(leaves)
                        .enumerate()
                        .map(|(item, (value, leaves))| BranchProvenance {
                            id: SurfaceId {
                                event: event.seq,
                                item: item as u32,
                            },
                            value,
                            leaves,
                        })
                        .collect::<Vec<_>>();
                    fold.surface.splice(start_index..=end_index, replacement);
                    if messages.cause == MessageCause::IntegrityRepair {
                        rebuild_integrity_branch(&mut fold, event.seq, &messages.items);
                    }
                }
            },
            TimelineEventKind::ImageProjection(projection) => {
                for (item, shadow) in projection.shadows.iter().enumerate() {
                    let Some(source_leaf) = fold.leaf_values.get(&shadow.source).cloned() else {
                        continue;
                    };
                    let mut projected_leaf = source_leaf.clone();
                    let removed = sampling_types::conversation::replace_item_images_with_text(
                        &mut projected_leaf,
                        &shadow.replacement,
                    );
                    debug_assert_eq!(removed, shadow.image_count);
                    let replacement_id = SurfaceId {
                        event: event.seq,
                        item: item as u32,
                    };
                    replace_branch_leaves(
                        &mut fold,
                        std::slice::from_ref(&shadow.source),
                        replacement_id,
                        projected_leaf,
                    );
                    if fold.unloaded.remove(&shadow.source) {
                        fold.unloaded.insert(replacement_id);
                    }
                    for entry in &mut fold.surface {
                        let owns_source = entry.leaves.contains(&shadow.source);
                        if !owns_source {
                            continue;
                        }
                        for leaf in &mut entry.leaves {
                            if *leaf == shadow.source {
                                *leaf = replacement_id;
                            }
                        }
                        let projected = sampling_types::conversation::replace_item_images_with_text(
                            &mut entry.value,
                            &shadow.replacement,
                        );
                        let derived_redacted = sampling_types::conversation::redact_projected_image_compaction_references(
                            &mut entry.value,
                            &source_leaf,
                            &shadow.replacement,
                        );
                        if projected > 0 || derived_redacted {
                            if projected > 0 {
                                debug_assert_eq!(projected, shadow.image_count);
                            }
                            entry.id = replacement_id;
                        }
                    }
                }
                let mut replacement_item = projection.shadows.len() as u32;
                for tool_call in &projection.tool_calls {
                    let Some(source_leaf) = fold.leaf_values.get(&tool_call.source).cloned() else {
                        continue;
                    };
                    for value in fold.leaf_values.values_mut() {
                        sampling_types::conversation::redact_projected_image_tool_result_references(
                            value,
                            &source_leaf,
                            &tool_call.tool_call_ids,
                        );
                    }
                    for entry in &mut fold.surface {
                        sampling_types::conversation::redact_projected_image_tool_result_references(
                            &mut entry.value,
                            &source_leaf,
                            &tool_call.tool_call_ids,
                        );
                    }
                    let mut projected_leaf = source_leaf.clone();
                    let redacted = tool_call
                        .tool_call_ids
                        .iter()
                        .filter(|id| {
                            sampling_types::conversation::redact_projected_image_tool_call(
                                &mut projected_leaf,
                                id,
                            )
                        })
                        .count();
                    debug_assert_eq!(redacted, tool_call.tool_call_ids.len());
                    let replacement_id = SurfaceId {
                        event: event.seq,
                        item: replacement_item,
                    };
                    replacement_item = replacement_item.saturating_add(1);
                    replace_branch_leaves(
                        &mut fold,
                        std::slice::from_ref(&tool_call.source),
                        replacement_id,
                        projected_leaf,
                    );
                    if fold.unloaded.remove(&tool_call.source) {
                        fold.unloaded.insert(replacement_id);
                    }
                    for entry in &mut fold.surface {
                        if !entry.leaves.contains(&tool_call.source) {
                            continue;
                        }
                        for leaf in &mut entry.leaves {
                            if *leaf == tool_call.source {
                                *leaf = replacement_id;
                            }
                        }
                        let redacted = tool_call
                            .tool_call_ids
                            .iter()
                            .filter(|id| {
                                sampling_types::conversation::redact_projected_image_tool_call(
                                    &mut entry.value,
                                    id,
                                )
                            })
                            .count();
                        let derived_redacted = sampling_types::conversation::redact_projected_image_tool_call_compaction_references(
                            &mut entry.value,
                            &source_leaf,
                            &tool_call.tool_call_ids,
                        );
                        if redacted > 0 || derived_redacted {
                            if redacted > 0 {
                                debug_assert_eq!(redacted, tool_call.tool_call_ids.len());
                            }
                            entry.id = replacement_id;
                        }
                    }
                    for carrier_source in &tool_call.carrier_sources {
                        let replacement_id = SurfaceId {
                            event: event.seq,
                            item: replacement_item,
                        };
                        replacement_item = replacement_item.saturating_add(1);
                        let Some(source_leaf) = fold.leaf_values.get(carrier_source).cloned()
                        else {
                            continue;
                        };
                        let mut projected_leaf = source_leaf.clone();
                        let redacted =
                            sampling_types::conversation::redact_projected_image_response_carrier(
                                &mut projected_leaf,
                            );
                        debug_assert!(redacted);
                        replace_branch_leaves(
                            &mut fold,
                            std::slice::from_ref(carrier_source),
                            replacement_id,
                            projected_leaf,
                        );
                        if fold.unloaded.remove(carrier_source) {
                            fold.unloaded.insert(replacement_id);
                        }
                        for entry in &mut fold.surface {
                            if !entry.leaves.contains(carrier_source) {
                                continue;
                            }
                            for leaf in &mut entry.leaves {
                                if *leaf == *carrier_source {
                                    *leaf = replacement_id;
                                }
                            }
                            let carrier_redacted = sampling_types::conversation::redact_projected_image_response_carrier(
                                &mut entry.value,
                            );
                            let derived_redacted = sampling_types::conversation::redact_projected_image_response_carrier_compaction_references(
                                &mut entry.value,
                                &source_leaf,
                            );
                            if carrier_redacted || derived_redacted {
                                entry.id = replacement_id;
                            }
                        }
                    }
                }
            }
            TimelineEventKind::Compaction(CompactionEvent::Summary { id, target, .. })
                if completed.contains(id.as_str()) =>
            {
                let Some(start_index) = fold
                    .surface
                    .iter()
                    .position(|entry| entry.id == target.start)
                else {
                    continue;
                };
                let Some(end_index) = fold.surface.iter().position(|entry| entry.id == target.end)
                else {
                    continue;
                };
                if start_index <= end_index
                    && fold.surface[start_index..=end_index]
                        .iter()
                        .map(|entry| entry.id)
                        .eq(target.shadowed.iter().copied())
                {
                    fold.unloaded.extend(
                        fold.surface[start_index..=end_index]
                            .iter()
                            .flat_map(|entry| entry.leaves.iter().copied()),
                    );
                }
            }
            _ => {}
        }
    }
    fold
}

fn append_branch_leaves(
    fold: &mut BranchFold,
    event: EventSeq,
    items: &[ConversationItem],
    is_message: bool,
) {
    for (item, value) in items.iter().cloned().enumerate() {
        let id = SurfaceId {
            event,
            item: item as u32,
        };
        fold.surface.push(BranchProvenance {
            id,
            value: value.clone(),
            leaves: vec![id],
        });
        fold.leaf_order.push(id);
        fold.leaf_values.insert(id, value);
        fold.leaf_birth.insert(id, event);
        fold.leaf_is_message.insert(id, is_message);
    }
}

fn reset_branch(fold: &mut BranchFold, event: EventSeq, items: &[ConversationItem]) {
    *fold = BranchFold::default();
    append_branch_leaves(fold, event, items, true);
}

fn rebuild_integrity_branch(fold: &mut BranchFold, event: EventSeq, items: &[ConversationItem]) {
    let referenced = fold
        .surface
        .iter()
        .flat_map(|entry| entry.leaves.iter().copied())
        .collect::<BTreeSet<_>>();
    fold.leaf_order.retain(|id| referenced.contains(id));
    fold.leaf_values.retain(|id, _| referenced.contains(id));
    fold.leaf_birth.retain(|id, _| referenced.contains(id));
    fold.leaf_is_message.retain(|id, _| referenced.contains(id));
    for (item, entry) in fold.surface.iter().enumerate() {
        if entry.leaves.len() == 1 {
            fold.leaf_values
                .insert(entry.leaves[0], items[item].clone());
        } else if entry.leaves.is_empty() {
            let id = SurfaceId {
                event,
                item: item as u32,
            };
            fold.leaf_order.push(id);
            fold.leaf_values.insert(id, items[item].clone());
            fold.leaf_birth.insert(id, event);
            fold.leaf_is_message.insert(id, true);
        }
    }
}

fn replace_branch_leaves(
    fold: &mut BranchFold,
    replaced: &[SurfaceId],
    replacement: SurfaceId,
    value: ConversationItem,
) {
    let replaced = replaced.iter().copied().collect::<BTreeSet<_>>();
    let birth = replaced
        .iter()
        .filter_map(|id| fold.leaf_birth.get(id).copied())
        .min()
        .unwrap_or(replacement.event);
    let is_message = replaced
        .iter()
        .any(|id| fold.leaf_is_message.get(id).copied().unwrap_or(false));
    let insert_at = fold
        .leaf_order
        .iter()
        .position(|id| replaced.contains(id))
        .unwrap_or(fold.leaf_order.len());
    fold.leaf_order.retain(|id| !replaced.contains(id));
    fold.leaf_order.insert(insert_at, replacement);
    fold.leaf_values.retain(|id, _| !replaced.contains(id));
    fold.leaf_values.insert(replacement, value);
    fold.leaf_birth.retain(|id, _| !replaced.contains(id));
    fold.leaf_birth.insert(replacement, birth);
    fold.leaf_is_message.retain(|id, _| !replaced.contains(id));
    fold.leaf_is_message.insert(replacement, is_message);
}

fn replacement_branch_leaves(
    event: EventSeq,
    cause: MessageCause,
    previous: &[BranchProvenance],
    replacement: &[ConversationItem],
) -> Vec<Vec<SurfaceId>> {
    let own_leaf = |item: usize| {
        vec![SurfaceId {
            event,
            item: item as u32,
        }]
    };
    match cause {
        MessageCause::Compaction => {
            let leaves = previous
                .iter()
                .flat_map(|entry| entry.leaves.iter().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            replacement.iter().map(|_| leaves.clone()).collect()
        }
        MessageCause::ToolResultPrune if previous.len() == replacement.len() => {
            previous.iter().map(|entry| entry.leaves.clone()).collect()
        }
        MessageCause::IntegrityRepair => {
            let mut next_previous = 0;
            replacement
                .iter()
                .enumerate()
                .map(|(item, value)| {
                    let matched = previous[next_previous..]
                        .iter()
                        .position(|entry| conversation_items_match(&entry.value, value))
                        .map(|offset| next_previous + offset);
                    matched.map_or_else(
                        || own_leaf(item),
                        |index| {
                            next_previous = index + 1;
                            previous[index].leaves.clone()
                        },
                    )
                })
                .collect()
        }
        MessageCause::Seed
        | MessageCause::DirectUser
        | MessageCause::Interjection
        | MessageCause::User
        | MessageCause::Assistant
        | MessageCause::ToolResult
        | MessageCause::MemoryContext
        | MessageCause::ToolResultPrune
        | MessageCause::ContextRebuild
        | MessageCause::Rewind => (0..replacement.len()).map(own_leaf).collect(),
    }
}

fn conversation_items_match(left: &ConversationItem, right: &ConversationItem) -> bool {
    match (serde_json::to_value(left), serde_json::to_value(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn wall_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn duration_since(started_at_ms: Option<i64>, now_ms: i64) -> u64 {
    started_at_ms
        .and_then(|started| now_ms.checked_sub(started))
        .and_then(|duration| u64::try_from(duration).ok())
        .unwrap_or(0)
}

fn valid_workflow_initial_manifest(manifest: &serde_json::Value) -> bool {
    manifest.is_object()
        && serde_json::to_vec(manifest)
            .is_ok_and(|encoded| encoded.len() <= MAX_WORKFLOW_INITIAL_MANIFEST_BYTES)
}

fn valid_workflow_content_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_workflow_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= MAX_WORKFLOW_RUN_ID_BYTES
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_workflow_turn_handoff(
    status: WorkflowExecutionStatus,
    handoff: WorkflowTurnHandoff,
    closed: bool,
) -> bool {
    if closed {
        return handoff == WorkflowTurnHandoff::None;
    }
    match handoff {
        WorkflowTurnHandoff::None => matches!(
            status,
            WorkflowExecutionStatus::UserPaused
                | WorkflowExecutionStatus::BackOffPaused
                | WorkflowExecutionStatus::NoProgressPaused
                | WorkflowExecutionStatus::InfraPaused
                | WorkflowExecutionStatus::Blocked
                | WorkflowExecutionStatus::Cancelled
        ),
        WorkflowTurnHandoff::Completion => matches!(
            status,
            WorkflowExecutionStatus::BudgetLimited
                | WorkflowExecutionStatus::Interrupted
                | WorkflowExecutionStatus::Complete
                | WorkflowExecutionStatus::Failed
        ),
        WorkflowTurnHandoff::AttentionRequired => matches!(
            status,
            WorkflowExecutionStatus::UserPaused
                | WorkflowExecutionStatus::BackOffPaused
                | WorkflowExecutionStatus::NoProgressPaused
                | WorkflowExecutionStatus::InfraPaused
                | WorkflowExecutionStatus::Blocked
        ),
    }
}

fn valid_system_layout(items: &[ConversationItem]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(index, item)| !matches!(item, ConversationItem::System(_)) || index == 0)
}

fn replacement_preserves_system_head(
    current: &[ConversationItem],
    start_index: usize,
    replacement: &[ConversationItem],
) -> bool {
    let replacement_systems = replacement
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, ConversationItem::System(_)).then_some((index, item))
        })
        .collect::<Vec<_>>();
    match current.first() {
        Some(ConversationItem::System(before)) if start_index == 0 => {
            matches!(
                replacement_systems.as_slice(),
                [(0, ConversationItem::System(after))] if before.content == after.content
            )
        }
        _ => replacement_systems.is_empty(),
    }
}

fn validate_tool_result_prune(
    replaced: &[ConversationItem],
    replacement: &MessageEvent,
) -> Result<(), TimelineError> {
    if replaced.len() != replacement.items.len() {
        return Err(TimelineError::InvalidToolResultPrune);
    }
    let mut content_changed = false;
    for (before, after) in replaced.iter().zip(&replacement.items) {
        match (before, after) {
            (ConversationItem::ToolResult(before), ConversationItem::ToolResult(after)) => {
                let before_images =
                    serde_json::to_value(&before.images).expect("conversation images serialize");
                let after_images =
                    serde_json::to_value(&after.images).expect("conversation images serialize");
                if before.tool_call_id != after.tool_call_id || before_images != after_images {
                    return Err(TimelineError::ToolResultIdentityChanged);
                }
                content_changed |= before.content != after.content;
            }
            _ => {
                let before = serde_json::to_value(before).expect("conversation item serializes");
                let after = serde_json::to_value(after).expect("conversation item serializes");
                if before != after {
                    return Err(TimelineError::InvalidToolResultPrune);
                }
            }
        }
    }
    if !content_changed {
        return Err(TimelineError::InvalidToolResultPrune);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_compaction_summary_for(
        timeline: &mut Timeline,
        id: &str,
        target: SurfaceRange,
    ) -> SurfaceRange {
        let input_ref = crate::TimelineRangeRef {
            timeline_id: "test-timeline".into(),
            first_seq: 0,
            last_seq: timeline.next_seq().get() - 1,
        };
        let sideband_id = "00000000-0000-0000-0000-000000000001";
        timeline
            .record(TimelineEventKind::Sideband(crate::SidebandSpawnEvent {
                sideband_id: sideband_id.into(),
                purpose: crate::SidebandPurpose::CompactionSummary,
                source_refs: vec![input_ref.clone()],
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Summary {
                id: id.into(),
                input_ref,
                result_ref: crate::TimelineRangeRef {
                    timeline_id: sideband_id.into(),
                    first_seq: 2,
                    last_seq: 2,
                },
                target: target.clone(),
                source_tokens: 100,
                summary_chars: 7,
            }))
            .unwrap();
        target
    }

    fn record_compaction_summary(timeline: &mut Timeline, id: &str) -> SurfaceRange {
        let target = SurfaceRange {
            start: *timeline.surface_ids().first().expect("non-empty Surface"),
            end: *timeline.surface_ids().last().expect("non-empty Surface"),
            shadowed: timeline.surface_ids().to_vec(),
        };
        record_compaction_summary_for(timeline, id, target)
    }

    fn record_image_description(
        timeline: &mut Timeline,
        source: SurfaceId,
    ) -> crate::TimelineRangeRef {
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let event = timeline
            .record(TimelineEventKind::Sideband(crate::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: crate::SidebandPurpose::ImageDescription,
                source_refs: vec![crate::TimelineRangeRef {
                    timeline_id: "test-timeline".into(),
                    first_seq: source.event.get(),
                    last_seq: source.event.get(),
                }],
            }))
            .unwrap();
        crate::TimelineRangeRef {
            timeline_id: sideband_id,
            first_seq: event.seq.get(),
            last_seq: event.seq.get(),
        }
    }

    fn user_identity() -> TurnIdentity {
        TurnIdentity {
            origin: "user".into(),
            turn_kind: "user".into(),
            goal_id: None,
            goal_definition_revision: None,
            stage_id: None,
        }
    }

    fn completed_terminal() -> TurnTerminal {
        TurnTerminal {
            stop_reason: "end_turn".into(),
            completion_kind: "completed".into(),
        }
    }

    fn goal_continuation_identity(goal_id: &str) -> TurnIdentity {
        TurnIdentity {
            origin: "goal_continuation".into(),
            turn_kind: "internal".into(),
            goal_id: Some(goal_id.into()),
            goal_definition_revision: Some(1),
            stage_id: None,
        }
    }

    fn record_input(
        timeline: &mut Timeline,
        id: u64,
        index: usize,
        text: &str,
        identity: TurnIdentity,
        input_kind: TurnInputKind,
    ) {
        let turn = TurnId(id);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity,
                model_id: "model".into(),
                input_message_count: timeline.surface_len(),
                prompt_index: index,
                prompt_text: text.into(),
                input_kind,
                redirect_kind: None,
            }))
            .unwrap();
        let mut item = ConversationItem::user(text);
        item.set_prompt_index(index);
        timeline.append(item, MessageCause::User).unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
    }

    fn record_prompt(timeline: &mut Timeline, id: u64, index: usize, text: &str) {
        record_input(
            timeline,
            id,
            index,
            text,
            user_identity(),
            TurnInputKind::Prompt,
        );
    }

    #[test]
    fn control_revision_must_increase() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 7,
                snapshot: serde_json::json!({ "control_revision": 7 }),
                retired_context_layers: vec![],
                model_contexts: vec![],
            }))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Control(ControlEvent {
                revision: 7,
                snapshot: serde_json::json!({ "control_revision": 7 }),
                retired_context_layers: vec![],
                model_contexts: vec![],
            })),
            Err(TimelineError::NonMonotonicControlRevision {
                previous: 7,
                actual: 7
            })
        ));
    }

    #[test]
    fn control_context_is_an_append_only_replayable_surface_fact() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "behavior": "plan" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::Behavior,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder(
                        "<behavior-context>plan</behavior-context>",
                    ),
                }],
            }))
            .unwrap();
        let first_request = serde_json::to_value(timeline.surface()).unwrap();

        timeline
            .append(
                ConversationItem::assistant("conditioned answer"),
                MessageCause::Assistant,
            )
            .unwrap();
        let second_request = serde_json::to_value(timeline.surface()).unwrap();
        let first = first_request.as_array().unwrap();
        let second = second_request.as_array().unwrap();
        assert_eq!(&second[..first.len()], first);

        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({ "behavior": "normal" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::Behavior,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder(
                        "<behavior-context>normal; earlier modes retired</behavior-context>",
                    ),
                }],
            }))
            .unwrap();
        assert_eq!(
            timeline.surface().last().unwrap().text_content(),
            "<behavior-context>normal; earlier modes retired</behavior-context>"
        );
        assert_eq!(
            serde_json::to_value(timeline.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );

        let replay = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert_eq!(
            serde_json::to_value(replay.surface()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
    }

    #[test]
    fn control_context_cannot_bypass_the_typed_surface_boundary() {
        let mut timeline = Timeline::default();
        assert!(matches!(
            timeline.record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "behavior": "plan" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::Behavior,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("plan"),
                }],
            })),
            Err(TimelineError::InvalidControlContext)
        ));
    }

    #[test]
    fn in_turn_control_context_activates_after_terminal_and_latest_wins() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        let turn = TurnId(42);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 2,
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "behavior": "plan" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::Behavior,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("plan"),
                }],
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({ "behavior": "normal" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::Behavior,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("normal"),
                }],
            }))
            .unwrap();
        assert_eq!(timeline.surface().len(), 2);

        timeline
            .append(
                ConversationItem::assistant("old Behavior output"),
                MessageCause::Assistant,
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        assert_eq!(
            timeline
                .surface()
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            vec![
                "system".to_string(),
                "task".to_string(),
                "old Behavior output".to_string(),
                "normal".to_string(),
            ]
        );
        assert_eq!(
            serde_json::to_value(timeline.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                Timeline::from_events(timeline.events().to_vec())
                    .unwrap()
                    .surface()
            )
            .unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
    }

    #[test]
    fn in_turn_control_context_keeps_the_latest_transition_per_layer() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        for (revision, layer, text) in [
            (1, ControlContextLayer::AgentRole, "role-v1"),
            (2, ControlContextLayer::Behavior, "behavior-normal"),
        ] {
            timeline
                .record(TimelineEventKind::Control(ControlEvent {
                    revision,
                    snapshot: serde_json::json!({ "revision": revision }),
                    retired_context_layers: vec![],
                    model_contexts: vec![ControlContext {
                        layer,
                        activation: ControlContextActivation::Transition,
                        item: ConversationItem::system_reminder(text),
                    }],
                }))
                .unwrap();
        }
        let turn = TurnId(43);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: timeline.surface().len(),
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let step = StepId { turn, index: 0 };
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        for (revision, layer, text) in [
            (3, ControlContextLayer::Behavior, "behavior-plan"),
            (4, ControlContextLayer::Behavior, "behavior-goal"),
            (5, ControlContextLayer::AgentRole, "role-v2"),
        ] {
            timeline
                .record(TimelineEventKind::Control(ControlEvent {
                    revision,
                    snapshot: serde_json::json!({ "revision": revision }),
                    retired_context_layers: vec![],
                    model_contexts: vec![ControlContext {
                        layer,
                        activation: ControlContextActivation::Transition,
                        item: ConversationItem::system_reminder(text),
                    }],
                }))
                .unwrap();
        }
        timeline
            .append(
                ConversationItem::assistant("output under the old layers"),
                MessageCause::Assistant,
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "continued".into(),
                duration_ms: 1,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();

        assert_eq!(
            timeline
                .surface()
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            [
                "system",
                "task",
                "role-v1",
                "behavior-normal",
                "output under the old layers",
                "role-v2",
                "behavior-goal",
            ]
        );
        let active = timeline.active_control_contexts();
        assert_eq!(
            active[&ControlContextLayer::AgentRole].item.text_content(),
            "role-v2"
        );
        assert_eq!(
            active[&ControlContextLayer::Behavior].item.text_content(),
            "behavior-goal"
        );
        assert_eq!(
            serde_json::to_value(timeline.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                Timeline::from_events(timeline.events().to_vec())
                    .unwrap()
                    .surface()
            )
            .unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
    }

    #[test]
    fn plan_phase_activates_at_step_boundary_and_latest_transition_wins() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        let turn = TurnId(44);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: timeline.surface().len(),
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let step = StepId { turn, index: 0 };
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        for (revision, text) in [(1, "awaiting approval"), (2, "executing")] {
            timeline
                .record(TimelineEventKind::Control(ControlEvent {
                    revision,
                    snapshot: serde_json::json!({ "revision": revision }),
                    retired_context_layers: vec![],
                    model_contexts: vec![ControlContext {
                        layer: ControlContextLayer::PlanPhase,
                        activation: ControlContextActivation::Transition,
                        item: ConversationItem::system_reminder(text),
                    }],
                }))
                .unwrap();
        }
        assert_eq!(timeline.surface().len(), 2);

        timeline
            .record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "control_resample".into(),
                duration_ms: 1,
            }))
            .unwrap();

        assert_eq!(
            timeline
                .surface()
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            ["system", "task", "executing"]
        );
        assert_eq!(
            timeline.active_control_contexts()[&ControlContextLayer::PlanPhase]
                .item
                .text_content(),
            "executing"
        );
    }

    #[test]
    fn edited_goal_definition_activates_after_step_not_after_turn() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ])
        .unwrap();
        let turn = TurnId(44);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: timeline.surface().len(),
                prompt_index: 0,
                prompt_text: "task".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let step = StepId { turn, index: 0 };
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();

        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "goal_revision": 2 }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::GoalDefinition,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::goal_directive(
                        "edited objective",
                        SyntheticReason::SystemReminder,
                        sampling_types::GoalDirectiveTag {
                            goal_id: "goal-1".into(),
                            definition_revision: 2,
                        },
                    ),
                }],
            }))
            .unwrap();

        assert_eq!(timeline.surface().len(), 2);
        assert!(
            !timeline
                .active_control_contexts()
                .contains_key(&ControlContextLayer::GoalDefinition),
            "the current sample and tool batch must retain the old Goal epoch"
        );

        timeline
            .record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "continued".into(),
                duration_ms: 1,
            }))
            .unwrap();

        let active = timeline.active_control_contexts();
        let item = &active[&ControlContextLayer::GoalDefinition].item;
        assert_eq!(item.text_content(), "edited objective");
        assert!(matches!(
            item,
            ConversationItem::User(user)
                if user.goal_directive.as_ref().is_some_and(|tag| {
                    tag.goal_id == "goal-1" && tag.definition_revision == 2
                })
        ));
        assert_eq!(
            timeline.surface().last().unwrap().text_content(),
            "edited objective"
        );
        assert_eq!(
            serde_json::to_value(timeline.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                Timeline::from_events(timeline.events().to_vec())
                    .unwrap()
                    .surface()
            )
            .unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
    }

    #[test]
    fn shadow_reprojection_activates_immediately_at_an_in_turn_compaction_boundary() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "agent_name": "reviewer" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::AgentRole,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("role-v1"),
                }],
            }))
            .unwrap();
        timeline
            .replace_all(
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::user("retained turn"),
                ],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: TurnId(77),
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 2,
                prompt_index: 0,
                prompt_text: "retained turn".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let step = StepId {
            turn: TurnId(77),
            index: 0,
        };
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({ "agent_name": "writer" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::AgentRole,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("role-v2"),
                }],
            }))
            .unwrap();
        let active = timeline.active_control_contexts();
        assert_eq!(
            active
                .get(&ControlContextLayer::AgentRole)
                .unwrap()
                .item
                .text_content(),
            "role-v1",
            "a pending transition must not masquerade as the effective context"
        );

        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 3,
                snapshot: serde_json::json!({ "agent_name": "writer" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::AgentRole,
                    activation: ControlContextActivation::Reprojection,
                    item: ConversationItem::system_reminder("role-v1"),
                }],
            }))
            .unwrap();

        assert_eq!(timeline.surface().last().unwrap().text_content(), "role-v1");
        timeline
            .record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "continued".into(),
                duration_ms: 1,
            }))
            .unwrap();
        assert_eq!(timeline.surface().last().unwrap().text_content(), "role-v2");
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: TurnId(77),
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        assert_eq!(timeline.surface().last().unwrap().text_content(), "role-v2");
    }

    #[test]
    fn control_reprojection_cannot_duplicate_a_current_context() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "agent_name": "reviewer" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::AgentRole,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("role"),
                }],
            }))
            .unwrap();

        assert!(matches!(
            timeline.record(TimelineEventKind::Control(ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({ "agent_name": "reviewer" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::AgentRole,
                    activation: ControlContextActivation::Reprojection,
                    item: ConversationItem::system_reminder("duplicate"),
                }],
            })),
            Err(TimelineError::InvalidControlReprojection)
        ));
    }

    #[test]
    fn retired_control_context_cannot_be_resurrected_by_reprojection() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 1,
                snapshot: serde_json::json!({ "behavior": "goal" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::GoalDefinition,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::goal_directive(
                        "goal-v1",
                        SyntheticReason::SystemReminder,
                        sampling_types::GoalDirectiveTag {
                            goal_id: "goal-1".into(),
                            definition_revision: 1,
                        },
                    ),
                }],
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Control(ControlEvent {
                revision: 2,
                snapshot: serde_json::json!({ "behavior": "normal" }),
                retired_context_layers: vec![ControlContextLayer::GoalDefinition],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::Behavior,
                    activation: ControlContextActivation::Transition,
                    item: ConversationItem::system_reminder("normal"),
                }],
            }))
            .unwrap();

        let active = timeline.active_control_contexts();
        assert!(!active.contains_key(&ControlContextLayer::GoalDefinition));
        assert_eq!(
            active[&ControlContextLayer::Behavior].item.text_content(),
            "normal"
        );

        timeline
            .replace_all(
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::user("retained turn"),
                ],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Control(ControlEvent {
                revision: 3,
                snapshot: serde_json::json!({ "behavior": "normal" }),
                retired_context_layers: vec![],
                model_contexts: vec![ControlContext {
                    layer: ControlContextLayer::GoalDefinition,
                    activation: ControlContextActivation::Reprojection,
                    item: ConversationItem::goal_directive(
                        "goal-v1",
                        SyntheticReason::SystemReminder,
                        sampling_types::GoalDirectiveTag {
                            goal_id: "goal-1".into(),
                            definition_revision: 1,
                        },
                    ),
                }],
            })),
            Err(TimelineError::InvalidControlReprojection)
        ));
    }

    #[test]
    fn workflow_lifecycle_is_epoch_strict_and_cannot_resume_after_close() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id: "wf_1".into(),
                execution_epoch: 0,
                name: "research".into(),
                objective: "find the cause".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
            }))
            .unwrap();
        assert_eq!(
            timeline.open_workflow_run_ids().collect::<Vec<_>>(),
            ["wf_1"]
        );
        assert_eq!(
            timeline.workflow_lifecycle("wf_1"),
            Some(WorkflowLifecycle {
                name: "research".into(),
                objective: "find the cause".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
                execution_epoch: 0,
                status: None,
                handoff: None,
                message: None,
                open: true,
                closed: false,
            })
        );
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: "wf_1".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Failed,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms: 8,
                message: Some("host failed".into()),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Resumed {
                run_id: "wf_1".into(),
                execution_epoch: 1,
            }))
            .unwrap();
        assert_eq!(timeline.workflow_lifecycle("wf_1").unwrap().status, None);
        assert!(matches!(
            timeline.record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: "wf_1".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Complete,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms: 1,
                message: None,
            })),
            Err(TimelineError::WorkflowEpochMismatch { .. })
        ));
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: "wf_1".into(),
                execution_epoch: 1,
                status: WorkflowExecutionStatus::Complete,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms: 1,
                message: None,
            }))
            .unwrap();
        let lifecycle = timeline.workflow_lifecycle("wf_1").unwrap();
        assert_eq!(lifecycle.execution_epoch, 1);
        assert_eq!(lifecycle.status, Some(WorkflowExecutionStatus::Complete));
        assert!(lifecycle.closed);
        assert!(matches!(
            timeline.record(TimelineEventKind::Workflow(WorkflowEvent::Resumed {
                run_id: "wf_1".into(),
                execution_epoch: 2,
            })),
            Err(TimelineError::WorkflowAlreadyClosed(_))
        ));
    }

    #[test]
    fn interrupted_workflow_execution_is_closed_by_recovery() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id: "wf_recover".into(),
                execution_epoch: 0,
                name: "research".into(),
                objective: "recover causality".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
            }))
            .unwrap();

        let repairs = timeline.recover_interrupted().unwrap();
        assert_eq!(repairs.len(), 2);
        assert!(matches!(
            repairs.last().map(|event| &event.kind),
            Some(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                status: WorkflowExecutionStatus::Interrupted,
                ..
            }))
        ));
        assert_eq!(timeline.open_workflow_run_ids().next(), None);
        let lifecycle = timeline.workflow_lifecycle("wf_recover").unwrap();
        assert_eq!(lifecycle.status, Some(WorkflowExecutionStatus::Interrupted));
        assert_eq!(lifecycle.message.as_deref(), Some("process_interrupted"));
    }

    #[test]
    fn interrupted_subagent_is_left_open_for_backend_reconciliation() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                subagent_spawn("sa-recover", "child-recover"),
            )))
            .unwrap();

        let repairs = timeline.recover_interrupted().unwrap();
        assert!(repairs.is_empty());
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Ended(
                SubagentTerminalEvent {
                    subagent_id: "sa-recover".into(),
                    child_session_id: "child-recover".into(),
                    outcome: SubagentOutcome::Cancelled,
                    duration_ms: 1,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: Some("backend reconciliation".into()),
                    result_ref: None,
                    snapshot_ref: None,
                },
            )))
            .expect("backend reconciliation remains authoritative");
    }

    #[test]
    fn paused_workflow_can_only_close_once_without_fake_resume() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id: "wf_pause".into(),
                execution_epoch: 0,
                name: "research".into(),
                objective: "wait for input".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: "wf_pause".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::UserPaused,
                handoff: WorkflowTurnHandoff::None,
                duration_ms: 2,
                message: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Closed {
                run_id: "wf_pause".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Cancelled,
                handoff: WorkflowTurnHandoff::None,
                duration_ms: 3,
                message: Some("stopped by user".into()),
            }))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Workflow(WorkflowEvent::Closed {
                run_id: "wf_pause".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Cancelled,
                handoff: WorkflowTurnHandoff::None,
                duration_ms: 3,
                message: None,
            })),
            Err(TimelineError::WorkflowAlreadyClosed(_))
        ));
    }

    #[test]
    fn workflow_owned_subagent_requires_an_open_run() {
        let mut spawn = subagent_spawn("sa-workflow", "child-workflow");
        spawn.workflow_run_id = Some("wf_owner".into());
        let mut timeline = Timeline::default();
        assert!(matches!(
            timeline.record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                spawn.clone()
            ))),
            Err(TimelineError::WorkflowNotFound(_))
        ));
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id: "wf_owner".into(),
                execution_epoch: 0,
                name: "owner".into(),
                objective: "spawn one child".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)))
            .unwrap();
    }

    #[test]
    fn workflow_cannot_end_before_its_subagents() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id: "wf_children".into(),
                execution_epoch: 0,
                name: "research".into(),
                objective: "join children".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
            }))
            .unwrap();
        let mut spawn = subagent_spawn("sa-child", "child-session");
        spawn.workflow_run_id = Some("wf_children".into());
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: "wf_children".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Failed,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms: 1,
                message: Some("child still running".into()),
            })),
            Err(TimelineError::OpenChildren {
                boundary: "workflow"
            })
        ));
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Ended(
                SubagentTerminalEvent {
                    subagent_id: "sa-child".into(),
                    child_session_id: "child-session".into(),
                    outcome: SubagentOutcome::Cancelled,
                    duration_ms: 1,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: Some("cancelled".into()),
                    result_ref: None,
                    snapshot_ref: None,
                },
            )))
            .unwrap();
        timeline
            .record(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: "wf_children".into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Failed,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms: 1,
                message: Some("child cancelled".into()),
            }))
            .unwrap();
    }

    #[test]
    fn replacement_keeps_transcript_immutable() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("old question"),
            ConversationItem::assistant("old answer"),
        ])
        .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 3,
                prompt_index: 0,
            }))
            .unwrap();
        let target = SurfaceRange {
            start: timeline.surface_ids()[1],
            end: timeline.surface_ids()[2],
            shadowed: timeline.surface_ids()[1..=2].to_vec(),
        };
        record_compaction_summary_for(&mut timeline, "compact", target.clone());
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact".into(),
                source_items: 3,
                result_items: 2,
                duration_ms: 1,
            }))
            .unwrap();

        assert_eq!(timeline.events().len(), 8);
        assert_eq!(timeline.surface_len(), 2);
        assert_eq!(timeline.branch_transcript().len(), 3);
        assert_eq!(timeline.surface()[1].text_content(), "summary");
    }

    #[test]
    fn lifecycle_rejects_unpaired_children() {
        let mut timeline = Timeline::default();
        let turn = TurnId(7);
        let step = StepId { turn, index: 0 };
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 1,
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: TurnInputKind::Prompt,
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
                input_message_count: 1,
                tool_count: 0,
            }))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "completed".into(),
                duration_ms: 1,
            })),
            Err(TimelineError::OpenChildren { boundary: "step" })
        ));
    }

    #[test]
    fn causal_identifiers_cannot_be_reused_after_terminal_events() {
        let mut timeline = Timeline::default();
        let turn = TurnId(7);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 1,
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();

        assert!(matches!(
            timeline.record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 1,
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            })),
            Err(TimelineError::TurnAlreadySeen(TurnId(7)))
        ));
    }

    #[test]
    fn schema_v1_is_deliberately_rejected() {
        let timeline = Timeline::from_seed(vec![ConversationItem::user("one")]).unwrap();
        let mut events = timeline.events().to_vec();
        events[0].version = 1;
        assert!(matches!(
            Timeline::from_events(events),
            Err(TimelineError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn prompt_records_keep_typed_user_inputs_and_skip_synthetic_gaps() {
        let mut timeline = Timeline::default();
        record_prompt(&mut timeline, 1, 0, "explain this");
        record_input(
            &mut timeline,
            2,
            1,
            "internal continuation",
            goal_continuation_identity("goal-1"),
            TurnInputKind::Prompt,
        );
        record_input(
            &mut timeline,
            3,
            2,
            "cargo check -p shell --lib",
            user_identity(),
            TurnInputKind::Bash,
        );

        assert_eq!(
            timeline.prompt_records(),
            vec![
                PromptRecord {
                    prompt_index: 0,
                    text: "explain this".into(),
                    input_kind: TurnInputKind::Prompt,
                },
                PromptRecord {
                    prompt_index: 2,
                    text: "cargo check -p shell --lib".into(),
                    input_kind: TurnInputKind::Bash,
                },
            ]
        );
    }

    #[test]
    fn turn_ids_are_wire_strings_so_javascript_cannot_round_them() {
        let id = TurnId(u64::MAX);
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            format!("\"{}\"", u64::MAX)
        );
        assert_eq!(
            serde_json::from_str::<TurnId>("\"42\"").unwrap(),
            TurnId(42)
        );
        assert!(serde_json::from_str::<TurnId>("42").is_err());
    }

    #[test]
    fn recovery_closes_open_request_step_and_turn_by_appending() {
        let mut timeline = Timeline::default();
        let turn = TurnId(1);
        let step = StepId { turn, index: 0 };
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Request(RequestEvent::Started {
                id: "r1".into(),
                turn,
                step,
                model_id: "model".into(),
                input_message_count: 0,
                tool_count: 0,
            }))
            .unwrap();
        let original = timeline.events().len();
        let repairs = timeline.recover_interrupted().unwrap();
        assert_eq!(repairs.len(), 4);
        assert!(matches!(repairs[0].kind, TimelineEventKind::Recovery(_)));
        assert!(matches!(
            repairs[1].kind,
            TimelineEventKind::Request(RequestEvent::Cancelled { .. })
        ));
        assert!(matches!(
            repairs[2].kind,
            TimelineEventKind::Step(StepEvent::Ended { .. })
        ));
        assert!(matches!(
            repairs[3].kind,
            TimelineEventKind::Turn(TurnEvent::Ended { .. })
        ));
        assert_eq!(timeline.events().len(), original + 4);
        assert!(timeline.active_turn().is_none());
        assert!(timeline.open_request_ids().next().is_none());
    }

    #[test]
    fn recovery_materializes_results_for_declared_but_unstarted_tools() {
        let assistant = ConversationItem::Assistant(sampling_types::AssistantItem {
            content: "".into(),
            tool_calls: vec![sampling_types::ToolCall {
                id: "call".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }],
            model_id: Some("model".into()),
            model_fingerprint: None,
            reasoning_effort: None,
        });
        let mut timeline = Timeline::from_seed(vec![assistant]).unwrap();

        let repairs = timeline.recover_surface_integrity().unwrap();

        assert_eq!(repairs.len(), 2);
        assert_eq!(timeline.surface_len(), 2);
        assert!(matches!(
            &timeline.surface()[1],
            ConversationItem::ToolResult(result)
                if result.tool_call_id == "call"
                    && result.content.contains("may not have started")
        ));
    }

    #[test]
    fn explicit_surface_repair_is_one_atomic_replacement_event() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::user("prompt"),
            ConversationItem::tool_result("orphan", "result"),
        ])
        .unwrap();

        let (report, events) = timeline.repair_surface_history().unwrap();

        assert_eq!(report.stripped_tool_result_ids, vec!["orphan"]);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            TimelineEventKind::Messages(MessageEvent {
                cause: MessageCause::IntegrityRepair,
                surface: SurfaceOp::Replace { .. },
                ..
            })
        ));
    }

    #[test]
    fn rewind_projection_uses_timeline_branch_not_compaction_surface() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("user-info"),
        ])
        .unwrap();
        record_prompt(&mut timeline, 10, 0, "p0");
        record_prompt(&mut timeline, 11, 1, "p1");
        record_prompt(&mut timeline, 12, 2, "p2");
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: timeline.surface_len(),
                prompt_index: 3,
            }))
            .unwrap();
        let target = SurfaceRange {
            start: timeline.surface_ids()[1],
            end: *timeline.surface_ids().last().unwrap(),
            shadowed: timeline.surface_ids()[1..].to_vec(),
        };
        let target = record_compaction_summary_for(&mut timeline, "compact", target);
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact".into(),
                source_items: 5,
                result_items: 2,
                duration_ms: 1,
            }))
            .unwrap();
        record_prompt(&mut timeline, 13, 3, "p3");

        assert_eq!(
            timeline
                .prompt_records()
                .into_iter()
                .map(|record| (record.prompt_index, record.text))
                .collect::<Vec<_>>(),
            vec![
                (0, "p0".into()),
                (1, "p1".into()),
                (2, "p2".into()),
                (3, "p3".into()),
            ]
        );
        assert_eq!(timeline.last_completed_compaction_prompt_index(), Some(3));
        let rewound = timeline.rewind_surface(2).unwrap();
        assert_eq!(
            rewound
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            vec!["system", "user-info", "p0", "p1"]
        );

        timeline.replace_all(rewound, MessageCause::Rewind).unwrap();
        record_prompt(&mut timeline, 14, 2, "new-p2");
        assert_eq!(
            timeline
                .prompt_records()
                .into_iter()
                .map(|record| (record.prompt_index, record.text))
                .collect::<Vec<_>>(),
            vec![(0, "p0".into()), (1, "p1".into()), (2, "new-p2".into())]
        );
        assert_eq!(timeline.last_completed_compaction_prompt_index(), None);
    }

    #[test]
    fn pre_turn_context_rebuild_finalizes_the_rewind_preamble() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .replace_all(
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::user("user-info"),
                    ConversationItem::project_instructions("instructions"),
                ],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        record_prompt(&mut timeline, 1, 0, "prompt");

        let rewound = timeline.rewind_surface(0).unwrap();
        assert_eq!(
            rewound
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            vec!["system", "user-info", "instructions"]
        );
        assert!(matches!(
            timeline.replace_all(timeline.surface().to_vec(), MessageCause::ContextRebuild),
            Err(TimelineError::ContextRebuildAfterTurn)
        ));

        timeline.replace_all(rewound, MessageCause::Rewind).unwrap();
        timeline
            .replace_all(
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::user("new-user-info"),
                ],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        assert_eq!(
            timeline
                .branch_transcript()
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            vec!["system", "new-user-info"]
        );
    }

    #[test]
    fn context_rebuild_cannot_replace_or_insert_a_system_head() {
        let mut with_head = Timeline::from_seed(vec![ConversationItem::system("stable")]).unwrap();
        assert!(matches!(
            with_head.replace_all(
                vec![ConversationItem::system("changed")],
                MessageCause::ContextRebuild,
            ),
            Err(TimelineError::InvalidMessageShape)
        ));

        let mut without_head =
            Timeline::from_seed(vec![ConversationItem::user("context")]).unwrap();
        assert!(matches!(
            without_head.replace_all(
                vec![
                    ConversationItem::system("inserted"),
                    ConversationItem::user("context"),
                ],
                MessageCause::ContextRebuild,
            ),
            Err(TimelineError::InvalidMessageShape)
        ));
    }

    #[test]
    fn memory_context_appends_without_mutating_the_stable_system_head() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("user-info"),
        ])
        .unwrap();
        record_prompt(&mut timeline, 1, 0, "prompt");
        timeline
            .append(
                ConversationItem::memory_context("<memory-context>remember</memory-context>"),
                MessageCause::MemoryContext,
            )
            .unwrap();

        assert_eq!(timeline.surface().first().unwrap().text_content(), "system");
        assert_eq!(
            timeline.surface().last().unwrap().text_content(),
            "<memory-context>remember</memory-context>"
        );
        assert_eq!(
            serde_json::to_value(timeline.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
    }

    #[test]
    fn child_prompt_zero_does_not_collide_with_inherited_seed_markers() {
        let mut inherited = ConversationItem::user("parent prompt");
        inherited.set_prompt_index(0);
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            inherited,
            ConversationItem::assistant("parent answer"),
        ])
        .unwrap();
        record_prompt(&mut timeline, 1, 0, "child prompt");

        let rewound = timeline.rewind_surface(0).unwrap();

        assert_eq!(
            rewound
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            vec!["system", "parent prompt", "parent answer"]
        );
        assert!(matches!(
            &timeline.surface()[1],
            ConversationItem::User(user) if user.prompt_index.is_none()
        ));
    }

    #[test]
    fn compaction_requires_exactly_one_replacement() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact".into(),
                source_items: 1,
                result_items: 1,
                duration_ms: 1,
            })),
            Err(TimelineError::MissingCompactionReplacement(id)) if id == "compact"
        ));

        let target = record_compaction_summary(&mut timeline, "compact");
        let expected_unloaded = target.shadowed.clone();

        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();
        let current_target = SurfaceRange {
            start: timeline.surface_ids()[0],
            end: timeline.surface_ids()[0],
            shadowed: timeline.surface_ids().to_vec(),
        };
        assert!(matches!(
            timeline.replace_compaction_range(
                current_target,
                vec![ConversationItem::user_meta("second summary")],
            ),
            Err(TimelineError::DuplicateCompactionReplacement(id)) if id == "compact"
        ));
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact".into(),
                source_items: 1,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();
        assert_eq!(
            timeline.completed_compaction_unloaded_branch_ids(),
            expected_unloaded
        );
    }

    #[test]
    fn failed_compaction_never_creates_recall_evidence() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        record_compaction_summary(&mut timeline, "compact");
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Failed {
                id: "compact".into(),
                duration_ms: 1,
                error: "provider failed after summary".into(),
            }))
            .unwrap();

        assert!(
            timeline
                .completed_compaction_unloaded_branch_ids()
                .is_empty()
        );
    }

    #[test]
    fn completed_compaction_resolves_content_rewrites_to_original_branch_leaves() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::user("inspect the migration"),
            ConversationItem::tool_result("read-migration", "full migration implementation"),
            ConversationItem::assistant("use the shadow-table swap"),
        ])
        .unwrap();
        let original_ids = timeline.surface_ids().to_vec();

        let mut pruned = timeline.surface().to_vec();
        let ConversationItem::ToolResult(result) = &mut pruned[1] else {
            panic!("expected tool result")
        };
        result.content = "[pruned]".into();
        timeline
            .replace_all(pruned, MessageCause::ToolResultPrune)
            .unwrap();
        assert_ne!(timeline.surface_ids(), original_ids);

        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact-after-prune".into(),
                source_items: timeline.surface().len(),
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact-after-prune");
        assert_ne!(target.shadowed, original_ids);
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact-after-prune".into(),
                source_items: original_ids.len(),
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();

        assert_eq!(
            timeline.branch_transcript_with_ids().0,
            original_ids,
            "the recall transcript keeps the full pre-prune facts"
        );
        assert_eq!(
            timeline.completed_compaction_unloaded_branch_ids(),
            original_ids,
            "the completed compaction must unload those same branch coordinates"
        );
    }

    #[test]
    fn message_causes_cannot_impersonate_other_surface_operations() {
        let mut timeline = Timeline::default();
        assert!(matches!(
            timeline.append(ConversationItem::assistant("forged"), MessageCause::User),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(matches!(
            timeline.append(
                ConversationItem::memory_context("forged memory"),
                MessageCause::User,
            ),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(matches!(
            timeline.append(
                ConversationItem::session_rules("forged rules"),
                MessageCause::User,
            ),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(matches!(
            timeline.append(
                ConversationItem::memory_context("  "),
                MessageCause::MemoryContext
            ),
            Err(TimelineError::InvalidMessageShape)
        ));

        timeline
            .append(ConversationItem::user("real"), MessageCause::User)
            .unwrap();
        assert!(matches!(
            timeline.replace_all(
                vec![ConversationItem::user("rewritten without governance")],
                MessageCause::User,
            ),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(matches!(
            timeline.append(ConversationItem::user("late seed"), MessageCause::Seed),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(matches!(
            timeline.append(
                ConversationItem::system("late system prompt"),
                MessageCause::MemoryContext,
            ),
            Err(TimelineError::InvalidMessageShape)
        ));
    }

    #[test]
    fn image_only_direct_user_input_is_causal_but_grants_no_text_authority() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: TurnId(1),
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: String::new(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();

        let mut image = ConversationItem::user("");
        image.set_prompt_index(0);
        image.set_permission_evidence(PermissionEvidence::direct_user(""));
        image.add_image("data:image/png;base64,original");
        timeline.append(image, MessageCause::DirectUser).unwrap();

        assert!(timeline.direct_user_permission_evidence().is_empty());
        assert!(timeline.permission_classifier_context().is_empty());

        let mut forged = ConversationItem::user("");
        forged.set_prompt_index(0);
        forged.set_permission_evidence(PermissionEvidence::direct_user("delete files"));
        forged.add_image("data:image/png;base64,forged");
        assert!(matches!(
            timeline.append(forged, MessageCause::DirectUser),
            Err(TimelineError::InvalidMessageShape)
        ));

        let mut empty = ConversationItem::user("");
        empty.set_prompt_index(0);
        empty.set_permission_evidence(PermissionEvidence::direct_user(""));
        assert!(matches!(
            timeline.append(empty, MessageCause::DirectUser),
            Err(TimelineError::InvalidMessageShape)
        ));
    }

    #[test]
    fn image_only_interjection_is_causal_but_grants_no_text_authority() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: TurnId(1),
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "running".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();

        let mut image = ConversationItem::interjection("", "");
        image.add_image("data:image/png;base64,original");
        timeline.append(image, MessageCause::Interjection).unwrap();

        assert!(timeline.direct_user_permission_evidence().is_empty());
        assert!(timeline.permission_classifier_context().is_empty());

        let mut forged = ConversationItem::interjection("", "delete files");
        forged.add_image("data:image/png;base64,forged");
        assert!(matches!(
            timeline.append(forged, MessageCause::Interjection),
            Err(TimelineError::InvalidMessageShape)
        ));

        assert!(matches!(
            timeline.append(
                ConversationItem::interjection("", ""),
                MessageCause::Interjection
            ),
            Err(TimelineError::InvalidMessageShape)
        ));
    }

    #[test]
    fn image_shadow_is_irreversible_and_bound_to_a_live_surface_item() {
        use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

        let image = ConversationItem::User(UserItem {
            content: vec![ContentPart::Image {
                url: "data:image/png;base64,original".into(),
            }],
            ..Default::default()
        });
        let mut timeline = Timeline::from_seed(vec![image.clone()]).unwrap();
        let before_revision = timeline.surface_revision();
        let group = conversation_image_groups(timeline.surface()).remove(0);
        let runtime = sampling_types::ModelImageInputKey::new("text-model", "messages", "endpoint");
        let source = timeline.surface_ids()[0];
        let result_ref = record_image_description(&mut timeline, source);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: runtime.clone(),
                source_revision: before_revision,
                shadows: vec![ImageShadow {
                    source,
                    fingerprint: group.fingerprint.clone(),
                    image_count: group.image_count(),
                    replacement: "durable image description".into(),
                    provenance: ImageShadowSource::Description {
                        result_ref: result_ref.clone(),
                    },
                }],
                tool_calls: vec![],
            }))
            .unwrap();

        assert!(conversation_image_groups(timeline.surface()).is_empty());
        assert_eq!(timeline.surface_revision(), before_revision + 1);
        assert!(matches!(
            timeline.replace_all(vec![image.clone()], MessageCause::ContextRebuild),
            Err(TimelineError::ContextRebuildAfterTurn)
        ));
        let raw_source = timeline.events()[0]
            .messages()
            .expect("seed is immutable raw evidence");
        assert_eq!(
            serde_json::to_vec(&raw_source.items).unwrap(),
            serde_json::to_vec(std::slice::from_ref(&image)).unwrap(),
        );
        assert!(conversation_image_groups(&timeline.branch_transcript()).is_empty());
        record_prompt(&mut timeline, 9, 1, "next");
        assert!(matches!(
            timeline.replace_all(vec![image.clone()], MessageCause::Rewind),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(matches!(
            timeline.replace_all(vec![image.clone()], MessageCause::IntegrityRepair),
            Err(TimelineError::InvalidMessageShape)
        ));
        let rewound = timeline.rewind_surface(1).unwrap();
        assert!(conversation_image_groups(&rewound).is_empty());
        assert!(
            rewound[0]
                .text_content()
                .contains("durable image description")
        );
        assert!(matches!(
            timeline.replace_all(rewound, MessageCause::Rewind),
            Err(TimelineError::InvalidMessageShape)
        ));
        assert!(conversation_image_groups(timeline.surface()).is_empty());

        let source = timeline.surface_ids()[0];
        assert!(matches!(
            timeline.record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: runtime,
                source_revision: before_revision,
                shadows: vec![ImageShadow {
                    source,
                    fingerprint: "wrong-source".into(),
                    image_count: 1,
                    replacement: "forged".into(),
                    provenance: ImageShadowSource::Description {
                        result_ref: result_ref.clone(),
                    },
                }],
                tool_calls: vec![],
            })),
            Err(TimelineError::InvalidImageProjection)
        ));
        assert!(matches!(
            timeline.record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "", "messages", "endpoint",
                ),
                source_revision: before_revision,
                shadows: vec![ImageShadow {
                    source,
                    fingerprint: group.fingerprint.clone(),
                    image_count: group.image_count(),
                    replacement: "forged".into(),
                    provenance: ImageShadowSource::Description { result_ref },
                }],
                tool_calls: vec![],
            })),
            Err(TimelineError::InvalidImageProjection)
        ));
    }

    #[test]
    fn image_projection_atomically_redacts_parallel_tool_call_paths() {
        use sampling_types::conversation::{
            BackendToolCallItem, BackendToolKind, ContentPart, ToolCall, conversation_image_groups,
        };

        let mut assistant = ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_a".into(),
                name: "read_file".into(),
                arguments: r#"{"target_file":"/secret/a.png"}"#.into(),
            },
            ToolCall {
                id: "call_b".into(),
                name: "read_file".into(),
                arguments: r#"{"target_file":"/secret/b.png"}"#.into(),
            },
        ]);
        let ConversationItem::Assistant(assistant_item) = &mut assistant else {
            unreachable!()
        };
        assistant_item.content = "Reading /secret/from-assistant.png".into();
        let seed = vec![
            ConversationItem::Reasoning(sampling_types::conversation::synthesized_reasoning_item(
                "Reading /secret/from-reasoning.png",
            )),
            ConversationItem::BackendToolCall(BackendToolCallItem {
                kind: BackendToolKind::CodeInterpreter(
                    sampling_types::rs::CodeInterpreterToolCall {
                        code: Some("open('/secret/from-backend-tool.png')".into()),
                        container_id: "container_1".into(),
                        id: "ci_1".into(),
                        outputs: None,
                        status: sampling_types::rs::CodeInterpreterToolCallStatus::Completed,
                    },
                ),
            }),
            ConversationItem::Reasoning(sampling_types::conversation::synthesized_reasoning_item(
                "Continuing /secret/from-reasoning-2.png",
            )),
            assistant,
            ConversationItem::tool_result_with_images(
                "call_a",
                "Read /secret/a.png",
                vec![ContentPart::Image {
                    url: "data:image/png;base64,a".into(),
                }],
            ),
            ConversationItem::tool_result_with_images(
                "call_b",
                "Read /secret/b.png",
                vec![ContentPart::Image {
                    url: "data:image/png;base64,b".into(),
                }],
            ),
        ];
        let mut timeline = Timeline::from_seed(seed).unwrap();
        let before_revision = timeline.surface_revision();
        let groups = conversation_image_groups(timeline.surface());
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| {
            group
                .tool_call
                .as_ref()
                .is_some_and(|call| call.item_index == 3)
        }));

        let shadows = groups
            .iter()
            .map(|group| {
                let source = timeline.surface_ids()[group.item_index];
                ImageShadow {
                    source,
                    fingerprint: group.fingerprint.clone(),
                    image_count: group.image_count(),
                    replacement: format!("description for {}", group.item_index),
                    provenance: ImageShadowSource::Description {
                        result_ref: record_image_description(&mut timeline, source),
                    },
                }
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            timeline.record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: before_revision,
                shadows: shadows.clone(),
                tool_calls: vec![ImageToolCallShadow {
                    source: timeline.surface_ids()[3],
                    tool_call_ids: vec!["call_a".into(), "call_b".into()],
                    carrier_sources: vec![timeline.surface_ids()[0], timeline.surface_ids()[2],],
                }],
            })),
            Err(TimelineError::InvalidImageProjection)
        ));

        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: before_revision,
                shadows,
                tool_calls: vec![ImageToolCallShadow {
                    source: timeline.surface_ids()[3],
                    tool_call_ids: vec!["call_a".into(), "call_b".into()],
                    carrier_sources: vec![
                        timeline.surface_ids()[0],
                        timeline.surface_ids()[1],
                        timeline.surface_ids()[2],
                    ],
                }],
            }))
            .unwrap();

        let serialized_surface = serde_json::to_string(timeline.surface()).unwrap();
        assert!(!serialized_surface.contains("/secret/a.png"));
        assert!(!serialized_surface.contains("/secret/b.png"));
        assert!(!serialized_surface.contains("/secret/from-assistant.png"));
        assert!(!serialized_surface.contains("/secret/from-reasoning.png"));
        assert!(!serialized_surface.contains("/secret/from-backend-tool.png"));
        assert!(!serialized_surface.contains("/secret/from-reasoning-2.png"));
        let serialized_branch = serde_json::to_string(&timeline.branch_transcript()).unwrap();
        assert!(!serialized_branch.contains("/secret/a.png"));
        assert!(!serialized_branch.contains("/secret/b.png"));
        assert!(!serialized_branch.contains("/secret/from-assistant.png"));
        assert!(!serialized_branch.contains("/secret/from-reasoning.png"));
        assert!(!serialized_branch.contains("/secret/from-backend-tool.png"));
        assert!(!serialized_branch.contains("/secret/from-reasoning-2.png"));
        let raw_events = serde_json::to_string(timeline.events()).unwrap();
        assert!(raw_events.contains("/secret/a.png"));
        assert!(raw_events.contains("/secret/b.png"));
        assert!(raw_events.contains("/secret/from-assistant.png"));
        assert!(raw_events.contains("/secret/from-reasoning.png"));
        assert!(raw_events.contains("/secret/from-backend-tool.png"));
        assert!(raw_events.contains("/secret/from-reasoning-2.png"));
        assert!(raw_events.contains("\"carrier_sources\""));

        let replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert_eq!(
            serde_json::to_value(replayed.surface()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(replayed.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.branch_transcript()).unwrap()
        );
    }

    #[test]
    fn image_projection_scrubs_tool_source_paths_from_completed_compaction_summary() {
        use sampling_types::conversation::{ContentPart, ToolCall, conversation_image_groups};

        let seed = vec![
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_image".into(),
                name: "read_file".into(),
                arguments: r#"{"target_file":"/secret/tool-image.png","query":"keep-this-query"}"#
                    .into(),
            }]),
            ConversationItem::tool_result_with_images(
                "call_image",
                "image loaded",
                vec![ContentPart::Image {
                    url: "data:image/png;base64,tool".into(),
                }],
            ),
        ];
        let mut timeline = Timeline::from_seed(seed).unwrap();
        let assistant_source = timeline.surface_ids()[0];
        let result_source = timeline.surface_ids()[1];
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact-tool-image".into(),
                source_items: 2,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact-tool-image");
        timeline
            .replace_compaction_range(
                target,
                vec![ConversationItem::user_meta(
                    "Keep architecture decision keep-this-query; image was /secret/tool-image.png",
                )],
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact-tool-image".into(),
                source_items: 2,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();

        let group = conversation_image_groups(&timeline.branch_transcript()).remove(0);
        let image_count = group.image_count();
        let result_ref = record_image_description(&mut timeline, result_source);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: timeline.surface_revision(),
                shadows: vec![ImageShadow {
                    source: result_source,
                    fingerprint: group.fingerprint,
                    image_count,
                    replacement: "a tool-produced diagram".into(),
                    provenance: ImageShadowSource::Description { result_ref },
                }],
                tool_calls: vec![ImageToolCallShadow {
                    source: assistant_source,
                    tool_call_ids: vec!["call_image".into()],
                    carrier_sources: vec![],
                }],
            }))
            .unwrap();

        let summary = timeline.surface()[0].text_content();
        assert!(summary.contains("Keep architecture decision keep-this-query"));
        assert!(!summary.contains("/secret/tool-image.png"));
        assert!(summary.contains("Image tool source projected to durable text"));
        assert!(
            timeline
                .branch_transcript()
                .iter()
                .all(|item| !item.text_content().contains("/secret/tool-image.png"))
        );
        let raw_events = serde_json::to_string(timeline.events()).unwrap();
        assert!(raw_events.contains("/secret/tool-image.png"));
    }

    #[test]
    fn image_projection_composes_multiple_leaves_owned_by_one_compaction_summary() {
        use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

        let assets = ["/secret/first.png", "/secret/second.png"];
        let seed = assets
            .iter()
            .map(|asset| {
                ConversationItem::User(UserItem {
                    content: vec![
                        ContentPart::Text {
                            text: format!("<image_files>\n1. {asset}\n</image_files>\ninspect")
                                .into(),
                        },
                        ContentPart::Image {
                            url: format!("data:image/png;base64,{asset}").into(),
                        },
                    ],
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        let mut timeline = Timeline::from_seed(seed).unwrap();
        let sources = timeline.surface_ids().to_vec();

        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact-two-images".into(),
                source_items: 2,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact-two-images");
        timeline
            .replace_compaction_range(
                target,
                vec![ConversationItem::user_meta(format!(
                    "Both diagrams remain at {} and {}.",
                    assets[0], assets[1]
                ))],
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact-two-images".into(),
                source_items: 2,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();

        let groups = conversation_image_groups(&timeline.branch_transcript());
        assert_eq!(groups.len(), 2);
        let shadows = groups
            .iter()
            .enumerate()
            .map(|(index, group)| ImageShadow {
                source: sources[index],
                fingerprint: group.fingerprint.clone(),
                image_count: group.image_count(),
                replacement: format!("durable description {index}"),
                provenance: ImageShadowSource::Description {
                    result_ref: record_image_description(&mut timeline, sources[index]),
                },
            })
            .collect();
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: timeline.surface_revision(),
                shadows,
                tool_calls: vec![],
            }))
            .unwrap();

        let surface = timeline.surface()[0].text_content();
        for (index, asset) in assets.iter().enumerate() {
            assert!(!surface.contains(asset));
            assert!(surface.contains(&format!("durable description {index}")));
        }
        let branch = serde_json::to_string(&timeline.branch_transcript()).unwrap();
        assert!(assets.iter().all(|asset| !branch.contains(asset)));
        assert!(conversation_image_groups(&timeline.branch_transcript()).is_empty());

        let replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert_eq!(
            serde_json::to_value(replayed.surface()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(replayed.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.branch_transcript()).unwrap()
        );
    }

    #[test]
    fn image_projection_scrubs_response_carrier_paths_from_compaction_summary() {
        use sampling_types::conversation::{ContentPart, ToolCall, conversation_image_groups};

        let carrier_path = "/secret/carrier-only.png";
        let tool_path = "/secret/tool-only.png";
        let seed = vec![
            ConversationItem::Reasoning(sampling_types::conversation::synthesized_reasoning_item(
                format!("Inspecting {carrier_path}"),
            )),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_image".into(),
                name: "read_file".into(),
                arguments: format!(r#"{{"target_file":"{tool_path}"}}"#).into(),
            }]),
            ConversationItem::tool_result_with_images(
                "call_image",
                "image loaded",
                vec![ContentPart::Image {
                    url: "data:image/png;base64,tool".into(),
                }],
            ),
        ];
        let mut timeline = Timeline::from_seed(seed).unwrap();
        let carrier_source = timeline.surface_ids()[0];
        let assistant_source = timeline.surface_ids()[1];
        let result_source = timeline.surface_ids()[2];

        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact-carrier".into(),
                source_items: 3,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact-carrier");
        timeline
            .replace_compaction_range(
                target,
                vec![ConversationItem::user_meta(format!(
                    "Reasoning referenced {carrier_path}; the tool used {tool_path}."
                ))],
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact-carrier".into(),
                source_items: 3,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();

        let group = conversation_image_groups(&timeline.branch_transcript()).remove(0);
        let image_count = group.image_count();
        let result_ref = record_image_description(&mut timeline, result_source);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: timeline.surface_revision(),
                shadows: vec![ImageShadow {
                    source: result_source,
                    fingerprint: group.fingerprint,
                    image_count,
                    replacement: "durable tool image description".into(),
                    provenance: ImageShadowSource::Description { result_ref },
                }],
                tool_calls: vec![ImageToolCallShadow {
                    source: assistant_source,
                    tool_call_ids: vec!["call_image".into()],
                    carrier_sources: vec![carrier_source],
                }],
            }))
            .unwrap();

        let surface = timeline.surface()[0].text_content();
        assert!(!surface.contains(carrier_path));
        assert!(!surface.contains(tool_path));
        assert!(surface.contains("response carrier projected to durable text"));
        let branch = serde_json::to_string(&timeline.branch_transcript()).unwrap();
        assert!(!branch.contains(carrier_path));
        assert!(!branch.contains(tool_path));

        let replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert_eq!(
            serde_json::to_value(replayed.surface()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(replayed.branch_transcript()).unwrap(),
            serde_json::to_value(timeline.branch_transcript()).unwrap()
        );
    }

    #[test]
    fn image_projection_survives_intermediate_surface_identity_replacement_and_rewind() {
        use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: TurnId(40),
                identity: user_identity(),
                model_id: "vision-model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "inspect".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let mut image = ConversationItem::User(UserItem {
            content: vec![
                ContentPart::Text {
                    text: "inspect".into(),
                },
                ContentPart::Image {
                    url: "data:image/png;base64,original".into(),
                },
            ],
            ..Default::default()
        });
        image.set_prompt_index(0);
        timeline.append(image, MessageCause::User).unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: TurnId(40),
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        record_prompt(&mut timeline, 41, 1, "later");

        let original_source = timeline.surface_ids()[0];
        timeline
            .replace_all(timeline.surface().to_vec(), MessageCause::IntegrityRepair)
            .unwrap();
        let replacement_source = timeline.surface_ids()[0];
        assert_ne!(replacement_source, original_source);
        let group = conversation_image_groups(timeline.surface()).remove(0);
        let image_count = group.image_count();
        let result_ref = record_image_description(&mut timeline, original_source);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: timeline.surface_revision(),
                shadows: vec![ImageShadow {
                    source: original_source,
                    fingerprint: group.fingerprint,
                    image_count,
                    replacement: "durable image description".into(),
                    provenance: ImageShadowSource::Description { result_ref },
                }],
                tool_calls: vec![],
            }))
            .unwrap();

        assert!(conversation_image_groups(&timeline.branch_transcript()).is_empty());
        let rewound = timeline.rewind_surface(1).unwrap();
        assert!(conversation_image_groups(&rewound).is_empty());
        assert!(
            rewound[0]
                .text_content()
                .contains("durable image description")
        );
    }

    #[test]
    fn latent_image_projection_updates_completed_compaction_recall_coordinates() {
        use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

        let image = ConversationItem::User(UserItem {
            content: vec![ContentPart::Image {
                url: "data:image/png;base64,original".into(),
            }],
            ..Default::default()
        });
        let mut timeline = Timeline::from_seed(vec![image]).unwrap();
        let original_source = timeline.surface_ids()[0];

        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact-image".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact-image");
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact-image".into(),
                source_items: 1,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();

        let group = conversation_image_groups(&timeline.branch_transcript()).remove(0);
        let image_count = group.image_count();
        let result_ref = record_image_description(&mut timeline, original_source);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: timeline.surface_revision(),
                shadows: vec![ImageShadow {
                    source: original_source,
                    fingerprint: group.fingerprint,
                    image_count,
                    replacement: "durable image description".into(),
                    provenance: ImageShadowSource::Description { result_ref },
                }],
                tool_calls: vec![],
            }))
            .unwrap();

        assert_eq!(timeline.surface()[0].text_content(), "summary");
        let (branch_ids, branch) = timeline.branch_transcript_with_ids();
        assert_eq!(branch_ids.len(), 1);
        assert_ne!(branch_ids[0], original_source);
        assert!(conversation_image_groups(&branch).is_empty());
        assert!(
            branch[0]
                .text_content()
                .contains("durable image description")
        );
        assert_eq!(
            timeline.completed_compaction_unloaded_branch_ids(),
            branch_ids,
            "recall must address the translated leaf rather than its retired image identity"
        );
    }

    #[test]
    fn image_projection_scrubs_exact_asset_paths_from_completed_compaction_summary() {
        use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

        let asset = "/sessions/example/assets/image-secret.png";
        let image = ConversationItem::User(UserItem {
            content: vec![
                ContentPart::Text {
                    text: format!(
                        "<image_files>\nThe following images were provided by the user and saved to the workspace for future use:\n1. {asset}\n\nThese images can be copied for use in other locations.\n</image_files>\ninspect this"
                    )
                    .into(),
                },
                ContentPart::Image {
                    url: "data:image/png;base64,original".into(),
                },
            ],
            ..Default::default()
        });
        let mut timeline = Timeline::from_seed(vec![image]).unwrap();
        let original_source = timeline.surface_ids()[0];

        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact-image-path".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact-image-path");
        timeline
            .replace_compaction_range(
                target,
                vec![ConversationItem::user_meta(format!(
                    "Keep the architecture notes. The image remains at {asset}."
                ))],
            )
            .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact-image-path".into(),
                source_items: 1,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();

        let group = conversation_image_groups(&timeline.branch_transcript()).remove(0);
        let image_count = group.image_count();
        let result_ref = record_image_description(&mut timeline, original_source);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: sampling_types::ModelImageInputKey::new(
                    "text-model",
                    "messages",
                    "endpoint",
                ),
                source_revision: timeline.surface_revision(),
                shadows: vec![ImageShadow {
                    source: original_source,
                    fingerprint: group.fingerprint,
                    image_count,
                    replacement: "a diagram of the architecture".into(),
                    provenance: ImageShadowSource::Description { result_ref },
                }],
                tool_calls: vec![],
            }))
            .unwrap();

        let summary = timeline.surface()[0].text_content();
        assert!(summary.contains("Keep the architecture notes."));
        assert!(summary.contains("a diagram of the architecture"));
        assert!(!summary.contains(asset));
        assert!(
            timeline
                .branch_transcript()
                .iter()
                .all(|item| !item.text_content().contains(asset))
        );

        let replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert_eq!(
            replayed.surface()[0].text_content(),
            timeline.surface()[0].text_content()
        );
        assert!(!replayed.surface()[0].text_content().contains(asset));
    }

    #[test]
    fn compaction_replacement_requires_a_linked_summary() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();

        let target = SurfaceRange {
            start: timeline.surface_ids()[0],
            end: timeline.surface_ids()[0],
            shadowed: timeline.surface_ids().to_vec(),
        };
        assert!(matches!(
            timeline.replace_compaction_range(
                target,
                vec![ConversationItem::user("unlinked summary")],
            ),
            Err(TimelineError::CompactionReplacementBeforeSummary)
        ));
    }

    #[test]
    fn compaction_replacement_must_match_the_summarized_range() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::user("old"),
            ConversationItem::assistant("tail"),
        ])
        .unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 2,
                prompt_index: 0,
            }))
            .unwrap();
        record_compaction_summary(&mut timeline, "compact");
        let wrong_target = SurfaceRange {
            start: timeline.surface_ids()[1],
            end: timeline.surface_ids()[1],
            shadowed: vec![timeline.surface_ids()[1]],
        };

        assert!(matches!(
            timeline.replace_compaction_range(
                wrong_target,
                vec![ConversationItem::user_meta("summary")],
            ),
            Err(TimelineError::CompactionTargetMismatch)
        ));
        assert_eq!(timeline.surface().len(), 2);
    }

    #[test]
    fn compaction_summary_requires_its_sideband_spawn() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();

        assert!(matches!(
            timeline.record(TimelineEventKind::Compaction(CompactionEvent::Summary {
                id: "compact".into(),
                input_ref: crate::TimelineRangeRef {
                    timeline_id: "test-timeline".into(),
                    first_seq: 0,
                    last_seq: 1,
                },
                result_ref: crate::TimelineRangeRef {
                    timeline_id: "00000000-0000-0000-0000-000000000001".into(),
                    first_seq: 2,
                    last_seq: 2,
                },
                target: SurfaceRange {
                    start: timeline.surface_ids()[0],
                    end: timeline.surface_ids()[0],
                    shadowed: vec![timeline.surface_ids()[0]],
                },
                source_tokens: 100,
                summary_chars: 7,
            })),
            Err(TimelineError::InvalidCompactionSummary(id)) if id == "compact"
        ));
    }

    #[test]
    fn compaction_rejects_a_second_summary() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        record_compaction_summary(&mut timeline, "compact");
        let duplicate = timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Compaction(summary @ CompactionEvent::Summary { .. }) => {
                    Some(summary.clone())
                }
                _ => None,
            })
            .unwrap();

        assert!(matches!(
            timeline.record(TimelineEventKind::Compaction(duplicate)),
            Err(TimelineError::DuplicateCompactionSummary(id)) if id == "compact"
        ));
    }

    #[test]
    fn committed_compaction_cannot_be_relabelled_failed() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact");
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();

        assert!(matches!(
            timeline.record(TimelineEventKind::Compaction(CompactionEvent::Failed {
                id: "compact".into(),
                duration_ms: 1,
                error: "too large".into(),
            })),
            Err(TimelineError::FailedCompactionHasReplacement(id)) if id == "compact"
        ));
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Completed {
                id: "compact".into(),
                source_items: 1,
                result_items: 1,
                duration_ms: 1,
            }))
            .unwrap();
    }

    #[test]
    fn recovery_completes_a_compaction_whose_replacement_was_committed() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact");
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();

        let repairs = timeline.recover_interrupted().unwrap();
        assert!(repairs.iter().any(|event| matches!(
            &event.kind,
            TimelineEventKind::Compaction(CompactionEvent::Completed {
                id,
                source_items: 1,
                result_items: 1,
                ..
            }) if id == "compact"
        )));
    }

    fn record_started_turn_and_step(timeline: &mut Timeline, turn_id: u64, step_index: u32) {
        let turn = TurnId(turn_id);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                identity: user_identity(),
                model_id: "model".into(),
                input_message_count: timeline.surface().len(),
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started {
                id: StepId {
                    turn,
                    index: step_index,
                },
            }))
            .unwrap();
    }

    #[test]
    fn in_process_stop_fails_an_uncommitted_compaction_without_closing_the_turn() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        record_started_turn_and_step(&mut timeline, 1, 0);
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();

        let terminal = timeline
            .settle_open_compaction("cancelled_by_stop")
            .unwrap()
            .expect("open compaction terminal");
        assert!(matches!(
            terminal.kind,
            TimelineEventKind::Compaction(CompactionEvent::Failed { ref error, .. })
                if error == "cancelled_by_stop"
        ));
        assert!(timeline.lifecycle.active_turn.is_some());
        assert!(timeline.lifecycle.active_step.is_some());
        assert!(timeline.lifecycle.open_compaction.is_none());
    }

    #[test]
    fn in_process_stop_completes_a_compaction_after_replacement_commit() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("prompt")]).unwrap();
        record_started_turn_and_step(&mut timeline, 1, 0);
        timeline
            .record(TimelineEventKind::Compaction(CompactionEvent::Started {
                id: "compact".into(),
                source_items: 1,
                prompt_index: 0,
            }))
            .unwrap();
        let target = record_compaction_summary(&mut timeline, "compact");
        timeline
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .unwrap();

        assert!(matches!(
            timeline
                .settle_open_compaction("cancelled_by_stop")
                .unwrap(),
            Some(TimelineEvent {
                kind: TimelineEventKind::Compaction(CompactionEvent::Completed { .. }),
                ..
            })
        ));
        assert!(timeline.lifecycle.open_compaction.is_none());
    }

    #[test]
    fn user_title_supersedes_generated_title_in_one_timeline() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Sideband(SidebandSpawnEvent {
                sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
                purpose: crate::SidebandPurpose::SessionTitle,
                source_refs: Vec::new(),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "Generated title".into(),
                source: SessionTitleSource::Generated {
                    sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
                    result_seq: 2,
                },
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "User title".into(),
                source: SessionTitleSource::User,
            }))
            .unwrap();

        let (seq, title) = timeline.session_title().unwrap();
        assert_eq!(seq.get(), 2);
        assert_eq!(title.title, "User title");
        assert_eq!(title.source, SessionTitleSource::User);
    }

    #[test]
    fn automatic_title_cannot_race_past_a_user_title() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "User title".into(),
                source: SessionTitleSource::User,
            }))
            .unwrap();

        let generated = timeline.record(TimelineEventKind::SessionTitle(SessionTitleEvent {
            title: "Late generated title".into(),
            source: SessionTitleSource::Generated {
                sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
                result_seq: 2,
            },
        }));
        assert!(matches!(
            generated,
            Err(TimelineError::GeneratedTitleAfterUserTitle)
        ));
        let fallback = timeline.record(TimelineEventKind::SessionTitle(SessionTitleEvent {
            title: "Late fallback".into(),
            source: SessionTitleSource::Fallback {
                sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
                terminal_seq: 3,
            },
        }));
        assert!(matches!(
            fallback,
            Err(TimelineError::GeneratedTitleAfterUserTitle)
        ));
        assert_eq!(timeline.events().len(), 1);
        assert_eq!(timeline.session_title().unwrap().1.title, "User title");
    }

    #[test]
    fn invalid_title_payloads_fail_without_mutating_timeline() {
        let mut timeline = Timeline::default();
        assert!(matches!(
            timeline.record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "   ".into(),
                source: SessionTitleSource::User,
            })),
            Err(TimelineError::InvalidSessionTitle)
        ));
        assert!(matches!(
            timeline.record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "title".into(),
                source: SessionTitleSource::Generated {
                    sideband_id: "not-a-uuid".into(),
                    result_seq: 0,
                },
            })),
            Err(TimelineError::InvalidSessionTitleSource)
        ));
        assert!(timeline.events().is_empty());
    }

    #[test]
    fn automatic_title_requires_one_prior_session_title_sideband() {
        let sideband_id = "018f0000-0000-7000-8000-000000000001";
        let mut timeline = Timeline::default();
        assert!(matches!(
            timeline.record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "unproven".into(),
                source: SessionTitleSource::Generated {
                    sideband_id: sideband_id.into(),
                    result_seq: 2,
                },
            })),
            Err(TimelineError::InvalidSessionTitleSource)
        ));

        timeline
            .record(TimelineEventKind::Sideband(SidebandSpawnEvent {
                sideband_id: sideband_id.into(),
                purpose: crate::SidebandPurpose::PermissionJudgment,
                source_refs: Vec::new(),
            }))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                title: "wrong purpose".into(),
                source: SessionTitleSource::Generated {
                    sideband_id: sideband_id.into(),
                    result_seq: 2,
                },
            })),
            Err(TimelineError::InvalidSessionTitleSource)
        ));
    }

    #[test]
    fn sideband_spawn_identity_is_unique_per_timeline() {
        let spawn = SidebandSpawnEvent {
            sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
            purpose: crate::SidebandPurpose::PermissionJudgment,
            source_refs: Vec::new(),
        };
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Sideband(spawn.clone()))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Sideband(spawn)),
            Err(TimelineError::DuplicateSidebandSpawn(_))
        ));
        assert_eq!(timeline.events().len(), 1);
    }

    fn subagent_spawn(id: &str, child: &str) -> SubagentSpawnEvent {
        SubagentSpawnEvent {
            subagent_id: id.into(),
            child_session_id: child.into(),
            security_parent_session_id: "parent-session".into(),
            subagent_type: "explore".into(),
            description: "inspect architecture".into(),
            prompt: "trace the canonical state".into(),
            context_source: SubagentContextSource::Forked,
            source_ref: None,
            context_normalized: false,
            resumed_from: None,
            parent_prompt_id: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            goal_definition_revision: None,
            surface_completion: true,
            child_cwd: "/workspace".into(),
            worktree_path: None,
            effective_model_id: "grow-3".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "grow-3",
                "responses",
                "test-endpoint",
            ),
            reasoning_effort: None,
        }
    }

    #[test]
    fn subagent_security_parent_must_be_nonempty_in_both_ledgers() {
        let mut parent = Timeline::default();
        let mut spawn = subagent_spawn("sa-1", "child-1");
        spawn.security_parent_session_id = "  ".into();
        assert!(matches!(
            parent.record(TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))),
            Err(TimelineError::InvalidSubagent)
        ));

        let mut child = Timeline::default();
        assert!(matches!(
            child.record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "parent-1".into(),
                parent_spawn_seq: 1,
                subagent_id: "sa-1".into(),
                security_parent_session_id: String::new(),
                context_source: SubagentContextSource::New,
                source_ref: None,
                normalized: false,
            })),
            Err(TimelineError::InvalidSubagent)
        ));
    }

    #[test]
    fn subagent_parent_and_child_ledgers_close_through_exact_result_ref() {
        let mut parent = Timeline::default();
        let spawn = parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                subagent_spawn("sa-1", "child-1"),
            )))
            .unwrap();
        let mut child = Timeline::default();
        child
            .record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "parent-1".into(),
                parent_spawn_seq: spawn.seq.get(),
                subagent_id: "sa-1".into(),
                security_parent_session_id: "parent-session".into(),
                context_source: SubagentContextSource::Forked,
                source_ref: None,
                normalized: false,
            }))
            .unwrap();
        let result = child
            .record(TimelineEventKind::SubagentResult(SubagentResultEvent {
                subagent_id: "sa-1".into(),
                outcome: SubagentOutcome::Completed,
                duration_ms: 25,
                tool_calls: 2,
                turns: 1,
                tokens_used: 90,
                error: None,
                output_ref: Some(format!(
                    "artifact:subagent-output:blake3:{}",
                    "a".repeat(64)
                )),
            }))
            .unwrap();
        let terminal = SubagentTerminalEvent {
            subagent_id: "sa-1".into(),
            child_session_id: "child-1".into(),
            outcome: SubagentOutcome::Completed,
            duration_ms: 25,
            tool_calls: 2,
            turns: 1,
            tokens_used: 90,
            error: None,
            result_ref: Some(crate::TimelineRangeRef {
                timeline_id: "child-1".into(),
                first_seq: result.seq.get(),
                last_seq: result.seq.get(),
            }),
            snapshot_ref: None,
        };
        child
            .validate_subagent_result_link(
                "parent-1",
                spawn.seq,
                &subagent_spawn("sa-1", "child-1"),
                &terminal,
            )
            .unwrap();
        parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Ended(terminal)))
            .unwrap();
    }

    #[test]
    fn one_child_timeline_can_belong_to_only_one_parent_spawn() {
        let mut parent = Timeline::default();
        parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                subagent_spawn("sa-1", "child-1"),
            )))
            .unwrap();

        assert!(matches!(
            parent.record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                subagent_spawn("sa-2", "child-1"),
            ))),
            Err(TimelineError::DuplicateSubagentChild(child)) if child == "child-1"
        ));
        assert_eq!(parent.events().len(), 1);
    }

    #[test]
    fn cross_timeline_link_rejects_a_foreign_seed_and_terminal_drift() {
        let spawn = subagent_spawn("sa-1", "child-1");
        let mut child = Timeline::default();
        child
            .record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "other-parent".into(),
                parent_spawn_seq: 7,
                subagent_id: "sa-1".into(),
                security_parent_session_id: "parent-session".into(),
                context_source: SubagentContextSource::Forked,
                source_ref: None,
                normalized: false,
            }))
            .unwrap();
        let result = child
            .record(TimelineEventKind::SubagentResult(SubagentResultEvent {
                subagent_id: "sa-1".into(),
                outcome: SubagentOutcome::Completed,
                duration_ms: 5,
                tool_calls: 1,
                turns: 1,
                tokens_used: 8,
                error: None,
                output_ref: None,
            }))
            .unwrap();
        let mut terminal = SubagentTerminalEvent {
            subagent_id: "sa-1".into(),
            child_session_id: "child-1".into(),
            outcome: SubagentOutcome::Completed,
            duration_ms: 5,
            tool_calls: 1,
            turns: 1,
            tokens_used: 8,
            error: None,
            result_ref: Some(crate::TimelineRangeRef {
                timeline_id: "child-1".into(),
                first_seq: result.seq.get(),
                last_seq: result.seq.get(),
            }),
            snapshot_ref: None,
        };
        assert!(matches!(
            child.validate_subagent_result_link("parent-1", EventSeq(7), &spawn, &terminal),
            Err(TimelineError::InvalidSubagentSeedLink)
        ));

        let mut linked_child = Timeline::default();
        linked_child
            .record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "parent-1".into(),
                parent_spawn_seq: 7,
                subagent_id: "sa-1".into(),
                security_parent_session_id: "parent-session".into(),
                context_source: SubagentContextSource::Forked,
                source_ref: None,
                normalized: false,
            }))
            .unwrap();
        let linked_result = linked_child
            .record(TimelineEventKind::SubagentResult(SubagentResultEvent {
                subagent_id: "sa-1".into(),
                outcome: SubagentOutcome::Completed,
                duration_ms: 5,
                tool_calls: 1,
                turns: 1,
                tokens_used: 8,
                error: None,
                output_ref: None,
            }))
            .unwrap();
        terminal.result_ref = Some(crate::TimelineRangeRef {
            timeline_id: "child-1".into(),
            first_seq: linked_result.seq.get(),
            last_seq: linked_result.seq.get(),
        });
        terminal.tokens_used = 9;
        assert!(matches!(
            linked_child.validate_subagent_result_link("parent-1", EventSeq(7), &spawn, &terminal),
            Err(TimelineError::InvalidSubagentResultLink)
        ));
    }

    #[test]
    fn schema_v5_rejects_unknown_event_fields() {
        let event = Timeline::default()
            .record(TimelineEventKind::Observation(ObservationEvent {
                scope: "test".into(),
                name: "strict-schema".into(),
                turn: None,
                step: None,
                data: None,
            }))
            .unwrap();

        let mut nested = serde_json::to_value(&event).unwrap();
        nested["event"]["legacy_field"] = serde_json::json!(true);
        assert!(serde_json::from_value::<TimelineEvent>(nested).is_err());

        let mut envelope = serde_json::to_value(&event).unwrap();
        envelope["legacy_field"] = serde_json::json!(true);
        assert!(serde_json::from_value::<TimelineEvent>(envelope).is_err());
    }

    #[test]
    fn child_result_requires_one_matching_seed_and_closes_the_timeline() {
        let result = SubagentResultEvent {
            subagent_id: "sa-1".into(),
            outcome: SubagentOutcome::Cancelled,
            duration_ms: 1,
            tool_calls: 0,
            turns: 0,
            tokens_used: 0,
            error: Some("cancelled".into()),
            output_ref: None,
        };
        let mut child = Timeline::default();
        assert!(matches!(
            child.record(TimelineEventKind::SubagentResult(result.clone())),
            Err(TimelineError::MissingSubagentSeed)
        ));
        child
            .record(TimelineEventKind::SubagentSeed(SubagentSeedEvent {
                parent_timeline_id: "parent-1".into(),
                parent_spawn_seq: 4,
                subagent_id: "sa-1".into(),
                security_parent_session_id: "parent-session".into(),
                context_source: SubagentContextSource::New,
                source_ref: None,
                normalized: false,
            }))
            .unwrap();
        child
            .record(TimelineEventKind::SubagentResult(result.clone()))
            .unwrap();
        assert!(matches!(
            child.record(TimelineEventKind::SubagentResult(result)),
            Err(TimelineError::SubagentTimelineEnded)
        ));
        assert!(matches!(
            child.record(TimelineEventKind::Observation(ObservationEvent {
                scope: "late".into(),
                name: "must-not-append".into(),
                turn: None,
                step: None,
                data: None,
            })),
            Err(TimelineError::SubagentTimelineEnded)
        ));
    }

    #[test]
    fn completed_parent_terminal_requires_child_result_reference() {
        let mut parent = Timeline::default();
        parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                subagent_spawn("sa-1", "child-1"),
            )))
            .unwrap();
        assert!(matches!(
            parent.record(TimelineEventKind::Subagent(SubagentEvent::Ended(
                SubagentTerminalEvent {
                    subagent_id: "sa-1".into(),
                    child_session_id: "child-1".into(),
                    outcome: SubagentOutcome::Completed,
                    duration_ms: 1,
                    tool_calls: 0,
                    turns: 1,
                    tokens_used: 1,
                    error: None,
                    result_ref: None,
                    snapshot_ref: None,
                },
            ))),
            Err(TimelineError::InvalidSubagent)
        ));
        assert_eq!(parent.events().len(), 1);
    }

    fn notification_payload(text: &str) -> NotificationPayloadRef {
        NotificationPayloadRef {
            blake3: blake3::hash(text.as_bytes()).to_hex().to_string(),
            bytes: text.len() as u64,
        }
    }

    fn receive_notification(
        timeline: &mut Timeline,
        source: NotificationSource,
        source_version: NotificationSourceVersion,
        text: &str,
    ) -> String {
        let id = notification_id("session-1", &source, &source_version).unwrap();
        timeline
            .record(TimelineEventKind::Notification(
                NotificationEvent::Received {
                    id: id.clone(),
                    owner_session_id: "session-1".into(),
                    source,
                    source_version,
                    payload_ref: notification_payload(text),
                },
            ))
            .unwrap();
        id
    }

    fn start_notification_turn(timeline: &mut Timeline, id: TurnId) {
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id,
                identity: TurnIdentity {
                    origin: "notification_drain".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "test-model".into(),
                input_message_count: timeline.surface_len(),
                prompt_index: 0,
                prompt_text: "notification available".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
    }

    #[test]
    fn notification_inbox_replays_from_received_minus_consumed() {
        let mut timeline = Timeline::default();
        let source = NotificationSource::TaskCompleted {
            task_id: "task-1".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Session,
        };
        let version = NotificationSourceVersion::Ordinal { value: 1 };
        let id = receive_notification(&mut timeline, source.clone(), version.clone(), "done");
        assert_eq!(timeline.pending_notifications()[0].id, id);

        let duplicate = TimelineEventKind::Notification(NotificationEvent::Received {
            id: notification_id("session-1", &source, &version).unwrap(),
            owner_session_id: "session-1".into(),
            source: source.clone(),
            source_version: version.clone(),
            payload_ref: notification_payload("done"),
        });
        assert!(matches!(
            timeline.record(duplicate),
            Err(TimelineError::InvalidNotification | TimelineError::DuplicateNotificationSource)
        ));

        let turn = TurnId(7);
        start_notification_turn(&mut timeline, turn);
        let mut input = ConversationItem::task_completed("done");
        let ConversationItem::User(user) = &mut input else {
            unreachable!()
        };
        user.prompt_index = Some(0);
        timeline
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![id.clone()],
                    turn,
                    input: Some(input),
                },
            ))
            .unwrap();
        assert!(timeline.pending_notifications().is_empty());
        assert_eq!(
            timeline.received_notification_id(&source, &version),
            Some(id.as_str()),
            "consumption must not erase producer idempotence evidence"
        );
        assert_eq!(timeline.surface().len(), 1);

        let mut replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert!(replayed.pending_notifications().is_empty());
        assert_eq!(
            replayed.received_notification_id(&source, &version),
            Some(id.as_str()),
        );
        assert_eq!(
            replayed.surface()[0].text_content(),
            timeline.surface()[0].text_content()
        );
        assert!(matches!(
            replayed.record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![id],
                    turn,
                    input: None,
                },
            )),
            Err(TimelineError::NotificationAlreadyConsumed(_))
        ));
    }

    #[test]
    fn dismissed_notification_replays_as_resolved_without_surface_input() {
        let mut timeline = Timeline::default();
        let source = NotificationSource::TaskCompleted {
            task_id: "goal-task".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Goal {
                goal_id: "goal-1".into(),
                definition_revision: 1,
            },
        };
        let version = NotificationSourceVersion::Ordinal { value: 1 };
        let id = receive_notification(&mut timeline, source.clone(), version.clone(), "done");

        timeline
            .record(TimelineEventKind::Notification(
                NotificationEvent::Dismissed {
                    notification_ids: vec![id.clone()],
                    reason: NotificationDismissReason::GoalOwnedAutostart {
                        goal_id: "goal-1".into(),
                        definition_revision: 1,
                    },
                },
            ))
            .unwrap();

        assert!(timeline.pending_notifications().is_empty());
        assert!(timeline.surface().is_empty());
        assert_eq!(
            timeline.received_notification_id(&source, &version),
            Some(id.as_str())
        );
        let replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert!(replayed.pending_notifications().is_empty());
        assert!(replayed.surface().is_empty());
    }

    #[test]
    fn goal_dismissal_rejects_session_or_foreign_goal_receipts() {
        let mut timeline = Timeline::default();
        let session_source = NotificationSource::TaskCompleted {
            task_id: "session-task".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Session,
        };
        let session_id = receive_notification(
            &mut timeline,
            session_source,
            NotificationSourceVersion::Ordinal { value: 1 },
            "done",
        );
        assert!(matches!(
            timeline.record(TimelineEventKind::Notification(
                NotificationEvent::Dismissed {
                    notification_ids: vec![session_id],
                    reason: NotificationDismissReason::GoalOwnedAutostart {
                        goal_id: "goal-1".into(),
                        definition_revision: 1,
                    },
                },
            )),
            Err(TimelineError::InvalidNotification)
        ));

        let goal_source = NotificationSource::TaskCompleted {
            task_id: "goal-task".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Goal {
                goal_id: "goal-2".into(),
                definition_revision: 1,
            },
        };
        let goal_id = receive_notification(
            &mut timeline,
            goal_source,
            NotificationSourceVersion::Ordinal { value: 1 },
            "done",
        );
        assert!(matches!(
            timeline.record(TimelineEventKind::Notification(
                NotificationEvent::Dismissed {
                    notification_ids: vec![goal_id],
                    reason: NotificationDismissReason::GoalOwnedAutostart {
                        goal_id: "goal-1".into(),
                        definition_revision: 1,
                    },
                },
            )),
            Err(TimelineError::InvalidNotification)
        ));
        assert_eq!(timeline.pending_notifications().len(), 2);
    }

    #[test]
    fn goal_notification_input_requires_matching_goal_turn_and_receipt_owner() {
        fn start_goal_turn(
            timeline: &mut Timeline,
            id: TurnId,
            goal_id: &str,
            definition_revision: u64,
        ) {
            timeline
                .record(TimelineEventKind::Turn(TurnEvent::Started {
                    id,
                    identity: TurnIdentity {
                        origin: "goal_continuation".into(),
                        turn_kind: "internal".into(),
                        goal_id: Some(goal_id.into()),
                        goal_definition_revision: Some(definition_revision),
                        stage_id: None,
                    },
                    model_id: "test-model".into(),
                    input_message_count: timeline.surface_len(),
                    prompt_index: 0,
                    prompt_text: "continue goal".into(),
                    input_kind: TurnInputKind::Prompt,
                    redirect_kind: None,
                }))
                .unwrap();
        }

        fn goal_input(goal_id: &str, definition_revision: u64) -> ConversationItem {
            let mut input = ConversationItem::goal_directive(
                "continue with durable evidence",
                SyntheticReason::SystemReminder,
                sampling_types::GoalDirectiveTag {
                    goal_id: goal_id.into(),
                    definition_revision,
                },
            );
            let ConversationItem::User(user) = &mut input else {
                unreachable!()
            };
            user.prompt_index = Some(0);
            input
        }

        let source = NotificationSource::TaskCompleted {
            task_id: "goal-task".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Goal {
                goal_id: "goal-1".into(),
                definition_revision: 1,
            },
        };
        let mut accepted = Timeline::default();
        let accepted_id = receive_notification(
            &mut accepted,
            source.clone(),
            NotificationSourceVersion::Ordinal { value: 1 },
            "done",
        );
        let accepted_turn = TurnId(17);
        start_goal_turn(&mut accepted, accepted_turn, "goal-1", 1);
        accepted
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![accepted_id],
                    turn: accepted_turn,
                    input: Some(goal_input("goal-1", 1)),
                },
            ))
            .unwrap();

        let mut rejected = Timeline::default();
        let rejected_id = receive_notification(
            &mut rejected,
            source,
            NotificationSourceVersion::Ordinal { value: 1 },
            "done",
        );
        let rejected_turn = TurnId(18);
        start_goal_turn(&mut rejected, rejected_turn, "goal-2", 1);
        assert!(matches!(
            rejected.record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![rejected_id],
                    turn: rejected_turn,
                    input: Some(goal_input("goal-2", 1)),
                },
            )),
            Err(TimelineError::InvalidNotification)
        ));

        let source = NotificationSource::TaskCompleted {
            task_id: "goal-task".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Goal {
                goal_id: "goal-1".into(),
                definition_revision: 1,
            },
        };
        let mut revised = Timeline::default();
        let revised_id = receive_notification(
            &mut revised,
            source,
            NotificationSourceVersion::Ordinal { value: 1 },
            "done",
        );
        let revised_turn = TurnId(19);
        start_goal_turn(&mut revised, revised_turn, "goal-1", 2);
        assert!(matches!(
            revised.record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![revised_id],
                    turn: revised_turn,
                    input: Some(goal_input("goal-1", 2)),
                },
            )),
            Err(TimelineError::InvalidNotification)
        ));
    }

    #[test]
    fn plan_notification_input_requires_a_dedicated_plan_handoff_turn() {
        fn start_turn(timeline: &mut Timeline, id: TurnId, origin: &str) {
            timeline
                .record(TimelineEventKind::Turn(TurnEvent::Started {
                    id,
                    identity: TurnIdentity {
                        origin: origin.into(),
                        turn_kind: "internal".into(),
                        goal_id: None,
                        goal_definition_revision: None,
                        stage_id: None,
                    },
                    model_id: "test-model".into(),
                    input_message_count: timeline.surface_len(),
                    prompt_index: 0,
                    prompt_text: "continue plan".into(),
                    input_kind: TurnInputKind::Prompt,
                    redirect_kind: None,
                }))
                .unwrap();
        }

        fn input() -> ConversationItem {
            let mut input = ConversationItem::notification_drain("continue approved plan");
            let ConversationItem::User(user) = &mut input else {
                unreachable!()
            };
            user.prompt_index = Some(0);
            input
        }

        let source = NotificationSource::PlanHandoff {
            artifact_hash: blake3::hash(b"# plan").to_hex().to_string(),
            artifact_revision: 1,
            handoff: PlanHandoffKind::Execute,
        };
        let mut accepted = Timeline::default();
        let accepted_id = receive_notification(
            &mut accepted,
            source.clone(),
            NotificationSourceVersion::Ordinal { value: 1 },
            "execute",
        );
        let accepted_turn = TurnId(20);
        start_turn(&mut accepted, accepted_turn, "plan_handoff");
        accepted
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![accepted_id],
                    turn: accepted_turn,
                    input: Some(input()),
                },
            ))
            .unwrap();

        let mut rejected = Timeline::default();
        let rejected_id = receive_notification(
            &mut rejected,
            source,
            NotificationSourceVersion::Ordinal { value: 1 },
            "execute",
        );
        let rejected_turn = TurnId(21);
        start_turn(&mut rejected, rejected_turn, "notification_drain");
        assert!(matches!(
            rejected.record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![rejected_id],
                    turn: rejected_turn,
                    input: Some(input()),
                },
            )),
            Err(TimelineError::InvalidNotification)
        ));
    }

    #[test]
    fn running_task_checkpoints_use_opaque_epochs_and_can_repeat() {
        let mut timeline = Timeline::default();
        let source = NotificationSource::TaskStillRunning {
            task_id: "task-1".into(),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Session,
        };
        assert!(matches!(
            notification_id(
                "session-1",
                &source,
                &NotificationSourceVersion::Ordinal { value: 1 }
            ),
            Err(TimelineError::InvalidNotification)
        ));

        let first = receive_notification(
            &mut timeline,
            source.clone(),
            NotificationSourceVersion::Opaque {
                value: "checkpoint-1".into(),
            },
            "still running",
        );
        let second = receive_notification(
            &mut timeline,
            source,
            NotificationSourceVersion::Opaque {
                value: "checkpoint-2".into(),
            },
            "still running again",
        );
        assert_ne!(first, second);
        assert_eq!(timeline.pending_notifications().len(), 2);

        let replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        assert_eq!(replayed.pending_notifications().len(), 2);
    }

    #[test]
    fn workflow_handoff_boundaries_use_opaque_epoch_status_and_kind_identity() {
        let mut timeline = Timeline::default();
        let source = NotificationSource::WorkflowHandoff {
            run_id: "workflow-1".into(),
            handoff: WorkflowTurnHandoff::Completion,
        };
        assert!(matches!(
            notification_id(
                "session-1",
                &source,
                &NotificationSourceVersion::Ordinal { value: 1 }
            ),
            Err(TimelineError::InvalidNotification)
        ));

        let first = receive_notification(
            &mut timeline,
            source.clone(),
            NotificationSourceVersion::Opaque {
                value: "workflow-handoff-v1:1:failed:completion".into(),
            },
            "failed",
        );
        let second = receive_notification(
            &mut timeline,
            source,
            NotificationSourceVersion::Opaque {
                value: "workflow-handoff-v1:2:complete:completion".into(),
            },
            "complete",
        );

        assert_ne!(first, second);
        assert_eq!(timeline.pending_notifications().len(), 2);
    }

    #[test]
    fn monitor_terminal_supersedes_unconsumed_progress_without_a_capacity_drop() {
        let mut timeline = Timeline::default();
        receive_notification(
            &mut timeline,
            NotificationSource::MonitorProgress {
                task_id: "monitor-1".into(),
                owner: NotificationOwner::Session,
            },
            NotificationSourceVersion::Opaque {
                value: "event-1".into(),
            },
            "progress",
        );
        let terminal = receive_notification(
            &mut timeline,
            NotificationSource::TaskCompleted {
                task_id: "monitor-1".into(),
                task_kind: NotificationTaskKind::Monitor,
                owner: NotificationOwner::Session,
            },
            NotificationSourceVersion::Ordinal { value: 1 },
            "completed",
        );
        let pending = timeline.pending_notifications();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, terminal);
    }

    #[test]
    fn task_terminal_supersedes_checkpoints_and_replay_rejects_late_checkpoints() {
        let mut timeline = Timeline::default();
        receive_notification(
            &mut timeline,
            NotificationSource::TaskStillRunning {
                task_id: "task-1".into(),
                task_kind: NotificationTaskKind::Task,
                owner: NotificationOwner::Goal {
                    goal_id: "goal-1".into(),
                    definition_revision: 1,
                },
            },
            NotificationSourceVersion::Opaque {
                value: "checkpoint-1".into(),
            },
            "still running",
        );
        let terminal = receive_notification(
            &mut timeline,
            NotificationSource::TaskCompleted {
                task_id: "task-1".into(),
                task_kind: NotificationTaskKind::Task,
                owner: NotificationOwner::Goal {
                    goal_id: "goal-1".into(),
                    definition_revision: 1,
                },
            },
            NotificationSourceVersion::Ordinal { value: 1 },
            "completed",
        );
        assert_eq!(timeline.pending_notifications()[0].id, terminal);
        assert_eq!(timeline.pending_notifications().len(), 1);

        let mut replayed = Timeline::from_events(timeline.events().to_vec()).unwrap();
        receive_notification(
            &mut replayed,
            NotificationSource::TaskStillRunning {
                task_id: "task-1".into(),
                task_kind: NotificationTaskKind::Task,
                owner: NotificationOwner::Session,
            },
            NotificationSourceVersion::Opaque {
                value: "checkpoint-after-terminal".into(),
            },
            "stale checkpoint",
        );
        let pending = replayed.pending_notifications();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, terminal);
    }

    #[test]
    fn pending_monitor_progress_keeps_a_bounded_recent_window() {
        let mut timeline = Timeline::default();
        for index in 0..(MAX_PENDING_MONITOR_PROGRESS_PER_TASK + 5) {
            receive_notification(
                &mut timeline,
                NotificationSource::MonitorProgress {
                    task_id: "monitor-1".into(),
                    owner: NotificationOwner::Session,
                },
                NotificationSourceVersion::Opaque {
                    value: format!("event-{index}"),
                },
                "progress",
            );
        }
        let pending = timeline.pending_notifications();
        assert_eq!(pending.len(), MAX_PENDING_MONITOR_PROGRESS_PER_TASK);
        assert!(matches!(
            &pending[0].source_version,
            NotificationSourceVersion::Opaque { value } if value == "event-5"
        ));
        assert!(matches!(
            &pending.last().unwrap().source_version,
            NotificationSourceVersion::Opaque { value }
                if value == &format!("event-{}", MAX_PENDING_MONITOR_PROGRESS_PER_TASK + 4)
        ));
    }

    #[test]
    fn consumed_monitor_terminal_still_suppresses_late_progress() {
        let mut timeline = Timeline::default();
        let terminal = receive_notification(
            &mut timeline,
            NotificationSource::TaskCompleted {
                task_id: "monitor-1".into(),
                task_kind: NotificationTaskKind::Monitor,
                owner: NotificationOwner::Session,
            },
            NotificationSourceVersion::Ordinal { value: 1 },
            "completed",
        );
        let turn = TurnId(11);
        start_notification_turn(&mut timeline, turn);
        timeline
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![terminal],
                    turn,
                    input: None,
                },
            ))
            .unwrap();
        receive_notification(
            &mut timeline,
            NotificationSource::MonitorProgress {
                task_id: "monitor-1".into(),
                owner: NotificationOwner::Session,
            },
            NotificationSourceVersion::Opaque {
                value: "late-event".into(),
            },
            "late progress",
        );
        assert!(timeline.pending_notifications().is_empty());
    }

    #[test]
    fn terminal_notification_inbox_has_no_lossy_capacity_limit() {
        let mut timeline = Timeline::default();
        for index in 0..64 {
            receive_notification(
                &mut timeline,
                NotificationSource::TaskCompleted {
                    task_id: format!("task-{index}"),
                    task_kind: NotificationTaskKind::Task,
                    owner: NotificationOwner::Session,
                },
                NotificationSourceVersion::Ordinal { value: 1 },
                "completed",
            );
        }
        let pending = timeline.pending_notifications();
        assert_eq!(pending.len(), 64);
        assert!(
            pending
                .windows(2)
                .all(|pair| { pair[0].received_seq.get() < pair[1].received_seq.get() })
        );
    }
}
