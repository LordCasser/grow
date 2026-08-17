//! Append-only agent timeline and its deterministic folds.
//!
//! The timeline is the durable causal ledger for a session. Streaming deltas
//! are transport-only; complete messages and lifecycle boundaries are facts.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use sampling_types::{ConversationItem, DanglingToolCallReason};
use serde::{Deserialize, Serialize};

pub const TIMELINE_SCHEMA_VERSION: u8 = 2;

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
pub struct StepId {
    pub turn: TurnId,
    pub index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SurfaceId {
    pub event: EventSeq,
    pub item: u32,
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
    SessionRestore,
    Rewind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SurfaceOp {
    Append,
    Replace {
        start: SurfaceId,
        end: SurfaceId,
        shadowed: Vec<SurfaceId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub cause: MessageCause,
    pub items: Vec<ConversationItem>,
    pub surface: SurfaceOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TurnEvent {
    Started {
        id: TurnId,
        origin: String,
        model_id: String,
        input_message_count: usize,
        prompt_index: Option<usize>,
        prompt_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        redirect_kind: Option<String>,
    },
    Ended {
        id: TurnId,
        outcome: String,
        duration_ms: u64,
        tool_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cancellation_category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
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
#[serde(tag = "state", rename_all = "snake_case")]
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
#[serde(tag = "state", rename_all = "snake_case")]
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
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CompactionEvent {
    Started {
        id: String,
        source_items: usize,
        prompt_index: usize,
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
pub struct RecoveryEvent {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "event", rename_all = "snake_case")]
pub enum TimelineEventKind {
    Messages(MessageEvent),
    Turn(TurnEvent),
    Step(StepEvent),
    Request(RequestEvent),
    Tool(ToolEvent),
    Compaction(CompactionEvent),
    Recovery(RecoveryEvent),
    Observation(ObservationEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub version: u8,
    pub seq: EventSeq,
    pub at_ms: i64,
    #[serde(flatten)]
    pub kind: TimelineEventKind,
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
    open_compaction: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    events: Vec<TimelineEvent>,
    surface: Vec<ConversationItem>,
    surface_ids: Vec<SurfaceId>,
    replace_generation: u64,
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
    #[error("tool-result prune must replace exactly one tool result")]
    InvalidToolResultPrune,
    #[error("tool-result prune changed fields other than content")]
    ToolResultIdentityChanged,
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
        for item in items {
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

    pub fn replace_generation(&self) -> u64 {
        self.replace_generation
    }

    pub fn surface(&self) -> &[ConversationItem] {
        &self.surface
    }

    pub fn transcript(&self) -> Vec<ConversationItem> {
        self.events
            .iter()
            .filter_map(TimelineEvent::messages)
            .filter(|messages| matches!(messages.surface, SurfaceOp::Append))
            .flat_map(|messages| messages.items.iter().cloned())
            .collect()
    }

    /// Build the uncompressed current branch and cut it before prompt `target`.
    ///
    /// Compaction and content-only rewrites shadow Surface nodes but do not
    /// erase rewind history. A Rewind replacement is different: it selects a
    /// new branch root, so earlier discarded appends must not reappear.
    pub fn rewind_surface(&self, target: usize) -> Vec<ConversationItem> {
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
                (
                    SurfaceOp::Replace { .. },
                    MessageCause::SystemPrompt | MessageCause::SessionRestore,
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
        let keep = sampling_types::conversation_truncate_for_prompt(&branch, target);
        branch.truncate(keep);
        branch
    }

    /// Raw prompt texts for the selected branch, indexed by prompt number.
    pub fn prompt_texts(&self) -> Vec<String> {
        let mut prompts = BTreeMap::<usize, String>::new();
        for event in &self.events {
            match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started {
                    prompt_index: Some(index),
                    prompt_text: Some(text),
                    ..
                }) => {
                    prompts.insert(*index, text.clone());
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
        let mut result = Vec::new();
        while let Some(text) = prompts.remove(&result.len()) {
            result.push(text);
        }
        result
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

    pub fn surface_len(&self) -> usize {
        self.surface.len()
    }

    pub fn surface_item(&self, index: usize) -> Option<&ConversationItem> {
        self.surface.get(index)
    }

    pub(crate) fn current_surface_ids(&self) -> &[SurfaceId] {
        &self.surface_ids
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
                "compaction": &compaction,
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
        if let Some(id) = compaction {
            let duration_ms = duration_since(self.compaction_started_at(&id), now);
            self.record(TimelineEventKind::Compaction(CompactionEvent::Failed {
                id,
                duration_ms,
                error: "process_interrupted".into(),
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
        if let TimelineEventKind::Messages(messages) = &event.kind {
            self.validate_messages(messages)?;
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
                self.replace_generation = self.replace_generation.saturating_add(1);
            }
        }
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
            TimelineEventKind::Compaction(CompactionEvent::Started { id, .. }) => {
                if !self.seen_compactions.insert(id.clone()) {
                    return Err(TimelineError::CompactionAlreadySeen(id.clone()));
                }
                if let Some(active) = self.open_compaction.replace(id.clone()) {
                    return Err(TimelineError::CompactionAlreadyOpen(active));
                }
            }
            TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. })
            | TimelineEventKind::Compaction(CompactionEvent::Failed { id, .. }) => {
                if self.open_compaction.as_deref() != Some(id.as_str()) {
                    return Err(TimelineError::CompactionNotOpen(id.clone()));
                }
                self.open_compaction = None;
            }
            TimelineEventKind::Messages(_)
            | TimelineEventKind::Recovery(_)
            | TimelineEventKind::Observation(_) => {}
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
    if replaced.len() != 1 || replacement.items.len() != 1 {
        return Err(TimelineError::InvalidToolResultPrune);
    }
    let ConversationItem::ToolResult(before) = &replaced[0] else {
        return Err(TimelineError::InvalidToolResultPrune);
    };
    let ConversationItem::ToolResult(after) = &replacement.items[0] else {
        return Err(TimelineError::InvalidToolResultPrune);
    };
    let before_images =
        serde_json::to_value(&before.images).expect("conversation images serialize");
    let after_images = serde_json::to_value(&after.images).expect("conversation images serialize");
    if before.tool_call_id != after.tool_call_id || before_images != after_images {
        return Err(TimelineError::ToolResultIdentityChanged);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_prompt(timeline: &mut Timeline, id: u64, index: usize, text: &str) {
        let turn = TurnId(id);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                origin: "user".into(),
                model_id: "model".into(),
                input_message_count: timeline.surface_len(),
                prompt_index: Some(index),
                prompt_text: Some(text.into()),
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
                cancellation_category: None,
                details: None,
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
            .replace_range(
                1,
                2,
                vec![ConversationItem::user("summary")],
                MessageCause::Compaction,
            )
            .unwrap();

        assert_eq!(timeline.events().len(), 4);
        assert_eq!(timeline.surface_len(), 2);
        assert_eq!(timeline.transcript().len(), 3);
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
                origin: "user".into(),
                model_id: "model".into(),
                input_message_count: 1,
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
                origin: "user".into(),
                model_id: "model".into(),
                input_message_count: 1,
                prompt_index: Some(0),
                prompt_text: Some("prompt".into()),
                redirect_kind: None,
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                cancellation_category: None,
                details: None,
            }))
            .unwrap();

        assert!(matches!(
            timeline.record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                origin: "user".into(),
                model_id: "model".into(),
                input_message_count: 1,
                prompt_index: Some(0),
                prompt_text: Some("prompt".into()),
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
        timeline
            .replace_all(
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::user("summary"),
                ],
                MessageCause::Compaction,
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
        let rewound = timeline.rewind_surface(2);
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
}
