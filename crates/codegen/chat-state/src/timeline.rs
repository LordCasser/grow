//! Append-only agent timeline and its deterministic folds.
//!
//! The timeline is the durable causal ledger for a session. Streaming deltas
//! are transport-only; complete messages and lifecycle boundaries are facts.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use sampling_types::{ConversationItem, DanglingToolCallReason};
use serde::{Deserialize, Serialize};

use crate::SidebandSpawnEvent;

pub const TIMELINE_SCHEMA_VERSION: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventSeq(u64);

impl EventSeq {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageCause {
    Seed,
    User,
    Assistant,
    ToolResult,
    WorkingDirectory,
    IntegrityRepair,
    Compaction,
    ToolResultPrune,
    ImageRewrite,
    SystemPrompt,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    FirstToken {
        id: String,
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

/// Durable Behavior/Goal control-plane state.
///
/// The payload stays shell-owned, while Timeline owns its identity, ordering,
/// durability and revision monotonicity. This keeps the core crate independent
/// of concrete Behavior and Goal types without creating a second state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEvent {
    pub revision: u64,
    pub snapshot: serde_json::Value,
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
    pub child_cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    pub effective_model_id: String,
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
    Compaction(CompactionEvent),
    Recovery(RecoveryEvent),
    Observation(ObservationEvent),
    Control(ControlEvent),
    SessionTitle(SessionTitleEvent),
    Sideband(SidebandSpawnEvent),
    Subagent(SubagentEvent),
    SubagentSeed(SubagentSeedEvent),
    SubagentResult(SubagentResultEvent),
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
    open_requests: BTreeMap<String, (TurnId, StepId)>,
    open_tools: BTreeMap<String, (TurnId, StepId, String)>,
    open_compaction: Option<OpenCompaction>,
    control_revision: Option<u64>,
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
    #[error("rewind target {0} has no branch-local user prompt marker")]
    MissingPromptMarker(usize),
    #[error("tool-result prune must replace exactly one tool result")]
    InvalidToolResultPrune,
    #[error("tool-result prune changed fields other than content")]
    ToolResultIdentityChanged,
    #[error("control revision {actual} must be greater than the previous revision {previous}")]
    NonMonotonicControlRevision { previous: u64, actual: u64 },
    #[error("turn {actual:?} cannot start while {active:?} is active")]
    TurnAlreadyActive { active: TurnId, actual: TurnId },
    #[error("turn {0:?} already has a start event")]
    TurnAlreadySeen(TurnId),
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
        let mut branch = Vec::new();
        for event in &self.events {
            let TimelineEventKind::Messages(messages) = &event.kind else {
                continue;
            };
            match (&messages.surface, messages.cause) {
                (SurfaceOp::Append, _) => branch.extend(messages.items.iter().cloned()),
                (SurfaceOp::Replace { .. }, MessageCause::Rewind) => {
                    branch.clone_from(&messages.items);
                }
                (SurfaceOp::Replace { .. }, MessageCause::IntegrityRepair) => {
                    let _ = crate::compaction_utils::repair_history(&mut branch);
                }
                (SurfaceOp::Replace { .. }, MessageCause::ContextRebuild) => {
                    // ContextRebuild is accepted only before the first turn. It
                    // finalizes the deferred session preamble as one atomic
                    // projection, so the branch must adopt the whole result.
                    branch.clone_from(&messages.items);
                }
                (
                    SurfaceOp::Replace { .. },
                    MessageCause::SystemPrompt | MessageCause::MemoryContext,
                ) => {
                    if let Some(system) = messages
                        .items
                        .iter()
                        .find(|item| matches!(item, ConversationItem::System(_)))
                        .cloned()
                    {
                        if let Some(index) = branch
                            .iter()
                            .position(|item| matches!(item, ConversationItem::System(_)))
                        {
                            branch[index] = system;
                        } else {
                            branch.insert(0, system);
                        }
                    }
                }
                (SurfaceOp::Replace { .. }, _) => {}
            }
        }
        branch
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

    pub fn prompt_texts(&self) -> Vec<String> {
        self.prompt_records()
            .into_iter()
            .map(|record| record.text)
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

    /// Whether the ledger proves that the named Goal's final report reached a
    /// successful terminal. This closes the crash window between committing
    /// the turn terminal and committing the subsequent Control receipt.
    pub fn has_successful_goal_finalization(&self, goal_id: &str) -> bool {
        let mut identities = BTreeMap::new();
        for event in &self.events {
            match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started { id, identity, .. }) => {
                    identities.insert(*id, identity);
                }
                TimelineEventKind::Turn(TurnEvent::Ended { id, terminal, .. }) => {
                    if identities.get(id).is_some_and(|identity| {
                        identity.origin == "goal_finalization"
                            && identity.goal_id.as_deref() == Some(goal_id)
                    }) && terminal.stop_reason == "end_turn"
                        && terminal.completion_kind == "completed"
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
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

    /// Append deterministic terminal facts for work left open by an interrupted
    /// process. Physical history is never truncated or rewritten.
    pub fn recover_interrupted(&mut self) -> Result<Vec<TimelineEvent>, TimelineError> {
        if self.lifecycle.active_turn.is_none()
            && self.lifecycle.active_step.is_none()
            && self.lifecycle.open_requests.is_empty()
            && self.lifecycle.open_tools.is_empty()
            && self.lifecycle.open_compaction.is_none()
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
        let mut captured = Vec::<(SurfaceId, ConversationItem)>::new();
        for event in self.events.iter().skip(start.get() as usize) {
            let Some(messages) = event.messages() else {
                continue;
            };
            match &messages.surface {
                SurfaceOp::Append => {
                    captured.extend(messages.items.iter().cloned().enumerate().map(
                        |(item, value)| {
                            (
                                SurfaceId {
                                    event: event.seq,
                                    item: item as u32,
                                },
                                value,
                            )
                        },
                    ));
                }
                SurfaceOp::Replace { shadowed, .. } if shadowed.len() == messages.items.len() => {
                    for (replacement_index, (shadowed_id, replacement)) in
                        shadowed.iter().zip(messages.items.iter()).enumerate()
                    {
                        if let Some((id, item)) =
                            captured.iter_mut().find(|(id, _)| id == shadowed_id)
                        {
                            *id = SurfaceId {
                                event: event.seq,
                                item: replacement_index as u32,
                            };
                            *item = replacement.clone();
                        }
                    }
                }
                SurfaceOp::Replace { .. } => {}
            }
        }
        captured.into_iter().map(|(_, item)| item).collect()
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
        if let TimelineEventKind::Messages(messages) = &event.kind {
            self.apply_messages(event.seq, messages);
        }
        self.lifecycle = lifecycle;
        self.events.push(event);
        Ok(())
    }

    fn validate(&self, event: &TimelineEvent) -> Result<LifecycleFold, TimelineError> {
        if self
            .events
            .iter()
            .any(|event| matches!(event.kind, TimelineEventKind::SubagentResult(_)))
        {
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
                    && spawn.input_refs.iter().any(|source| source == input_ref)
            }) {
                return Err(TimelineError::InvalidCompactionSummary(id.clone()));
            }
            self.validate_surface_range(target)?;
        }
        if let TimelineEventKind::Messages(messages) = &event.kind {
            if messages.cause == MessageCause::ContextRebuild && self.next_prompt_index() != 0 {
                return Err(TimelineError::ContextRebuildAfterTurn);
            }
            self.validate_messages(messages)?;
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
            for input_ref in &sideband.input_refs {
                if input_ref.last_seq >= event.seq.get() {
                    return Err(TimelineError::InvalidSideband(
                        crate::SidebandError::FutureInputRef {
                            last: input_ref.last_seq,
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
                        || spawn.subagent_type.trim().is_empty()
                        || spawn.description.trim().is_empty()
                        || spawn.prompt.trim().is_empty()
                        || spawn.child_cwd.trim().is_empty()
                        || spawn.effective_model_id.trim().is_empty()
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
            if seed.parent_timeline_id.trim().is_empty() || seed.subagent_id.trim().is_empty() {
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

    fn validate_messages(&self, messages: &MessageEvent) -> Result<(), TimelineError> {
        let _ = u32::try_from(messages.items.len()).map_err(|_| TimelineError::TooManyItems)?;
        match &messages.surface {
            SurfaceOp::Append => {
                if messages.items.is_empty() {
                    return Err(TimelineError::EmptyAppend);
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
                if messages.cause == MessageCause::ToolResultPrune {
                    validate_tool_result_prune(&self.surface[start_index..=end_index], messages)?;
                }
            }
        }
        Ok(())
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
            SurfaceOp::Append => {
                self.surface.extend(messages.items.iter().cloned());
                self.surface_ids
                    .extend((0..item_count).map(|item| SurfaceId {
                        event: event_seq,
                        item,
                    }));
            }
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
            TimelineEventKind::Request(RequestEvent::FirstToken { id })
            | TimelineEventKind::Request(RequestEvent::Retrying { id, .. }) => {
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
            | TimelineEventKind::Recovery(_)
            | TimelineEventKind::Observation(_)
            | TimelineEventKind::SessionTitle(_)
            | TimelineEventKind::Sideband(_)
            | TimelineEventKind::Subagent(_)
            | TimelineEventKind::SubagentSeed(_)
            | TimelineEventKind::SubagentResult(_) => {}
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
                input_refs: vec![input_ref.clone()],
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

    fn user_identity() -> TurnIdentity {
        TurnIdentity {
            origin: "user".into(),
            turn_kind: "user".into(),
            goal_id: None,
            stage_id: None,
        }
    }

    fn completed_terminal() -> TurnTerminal {
        TurnTerminal {
            stop_reason: "end_turn".into(),
            completion_kind: "completed".into(),
        }
    }

    fn goal_identity(goal_id: &str) -> TurnIdentity {
        TurnIdentity {
            origin: "goal_finalization".into(),
            turn_kind: "internal".into(),
            goal_id: Some(goal_id.into()),
            stage_id: Some(4),
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
            }))
            .unwrap();
        assert!(matches!(
            timeline.record(TimelineEventKind::Control(ControlEvent {
                revision: 7,
                snapshot: serde_json::json!({ "control_revision": 7 }),
            })),
            Err(TimelineError::NonMonotonicControlRevision {
                previous: 7,
                actual: 7
            })
        ));
    }

    #[test]
    fn goal_finalization_requires_matching_identity_and_success_terminal() {
        let mut timeline = Timeline::default();
        let failed = TurnId(1);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: failed,
                identity: goal_identity("goal-1"),
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "finalize".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: failed,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: TurnTerminal {
                    stop_reason: "refusal".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        assert!(!timeline.has_successful_goal_finalization("goal-1"));

        let succeeded = TurnId(2);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: succeeded,
                identity: goal_identity("goal-1"),
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 1,
                prompt_text: "finalize again".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: succeeded,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: completed_terminal(),
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        assert!(timeline.has_successful_goal_finalization("goal-1"));
        assert!(!timeline.has_successful_goal_finalization("goal-2"));
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
            .replace_compaction_range(target, vec![ConversationItem::user("summary")])
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
            goal_identity("goal-1"),
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
        let target = record_compaction_summary(&mut timeline, "compact");
        timeline
            .replace_compaction_range(
                target,
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::user("summary"),
                ],
            )
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

        assert_eq!(timeline.prompt_texts(), vec!["p0", "p1", "p2", "p3"]);
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
        assert_eq!(timeline.prompt_texts(), vec!["p0", "p1", "new-p2"]);
        assert_eq!(timeline.last_completed_compaction_prompt_index(), None);
    }

    #[test]
    fn pre_turn_context_rebuild_finalizes_the_rewind_preamble() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("draft")]).unwrap();
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
                    ConversationItem::system("rebuilt"),
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
            vec!["rebuilt", "new-user-info"]
        );
    }

    #[test]
    fn memory_context_updates_preamble_without_erasing_uncompressed_branch() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("user-info"),
        ])
        .unwrap();
        record_prompt(&mut timeline, 1, 0, "prompt");
        let mut memory_surface = timeline.surface().to_vec();
        memory_surface[0] = ConversationItem::system("system + memory");
        timeline
            .replace_all(memory_surface, MessageCause::MemoryContext)
            .unwrap();

        let rewound = timeline.rewind_surface(0).unwrap();
        assert_eq!(
            rewound
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>(),
            vec!["system + memory", "user-info"]
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

        timeline
            .replace_compaction_range(target, vec![ConversationItem::user("summary")])
            .unwrap();
        let current_target = SurfaceRange {
            start: timeline.surface_ids()[0],
            end: timeline.surface_ids()[0],
            shadowed: timeline.surface_ids().to_vec(),
        };
        assert!(matches!(
            timeline.replace_compaction_range(
                current_target,
                vec![ConversationItem::user("second summary")],
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
            timeline
                .replace_compaction_range(wrong_target, vec![ConversationItem::user("summary")],),
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
            .replace_compaction_range(target, vec![ConversationItem::user("summary")])
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
            .replace_compaction_range(target, vec![ConversationItem::user("summary")])
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

    #[test]
    fn user_title_supersedes_generated_title_in_one_timeline() {
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Sideband(SidebandSpawnEvent {
                sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
                purpose: crate::SidebandPurpose::SessionTitle,
                input_refs: Vec::new(),
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
                input_refs: Vec::new(),
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
            input_refs: Vec::new(),
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
            child_cwd: "/workspace".into(),
            worktree_path: None,
            effective_model_id: "grow-3".into(),
        }
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
}
