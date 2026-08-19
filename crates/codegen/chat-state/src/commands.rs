//! Commands sent to the ChatStateActor.

use std::collections::BTreeSet;

use compaction::PrunePlan;
use sampling_types::{
    ConversationItem, ConversationRequest, DanglingToolCallReason, SamplingConfig, TokenUsage,
    ToolSpec,
};
use tokio::sync::oneshot;

use crate::types::{
    AutoCompactTrigger, ChatStateSnapshot, ConversationCounts, Credentials, NotificationMeta,
    TurnCapture,
};
use crate::{MessageCause, TimelineEventKind, TrajectorySnapshot};

#[derive(Debug, thiserror::Error)]
pub enum TimelineWriteError {
    #[error("timeline event violates the causal fold: {0}")]
    Invalid(#[from] crate::TimelineError),
    #[error("timeline event was not durably committed: {0}")]
    Persistence(#[source] std::io::Error),
    #[error("timeline persistence acknowledgement was lost")]
    AcknowledgementLost,
    #[error("rewind target {target} is not before current prompt index {current}")]
    InvalidRewindTarget { target: usize, current: usize },
    #[error(
        "surface changed while transformation was in flight (expected revision {expected}, current {actual})"
    )]
    SurfaceChanged { expected: u64, actual: u64 },
}

#[derive(Debug, Clone, Default)]
pub struct ModelMetadata {
    pub resolved_model_id: Option<String>,
    pub model_fingerprint: Option<String>,
}

/// Compare-and-swap input for one canonical conversation image group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRewrite {
    pub item_index: usize,
    pub fingerprint: String,
    pub expected_image_count: usize,
    /// Sanitized auxiliary description. `None` means the group must be
    /// permanently removed because no usable description was produced.
    pub replacement: Option<String>,
}

/// Counts from an actor-serialized canonical image rewrite.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageRewriteReport {
    pub converted_images: usize,
    pub dropped_images: usize,
    pub unmatched_images: usize,
}

impl ImageRewriteReport {
    pub fn total_images(self) -> usize {
        self.converted_images + self.dropped_images
    }
}

/// Result of an actor-serialized tool-result prune.
///
/// `tokens_before` / `tokens_after` are the actor's `total_tokens` before and
/// after the command. `tokens_after` is the re-estimate clamped so pruning
/// never appears to increase usage, so `tokens_after <= tokens_before` always
/// holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Number of tool results actually trimmed in this execution.
    pub pruned_count: usize,
    /// `total_tokens` before pruning.
    pub tokens_before: u64,
    /// `total_tokens` after re-estimation (never higher than `tokens_before`).
    pub tokens_after: u64,
}

/// Failure modes for [`ChatStateCommand::PruneToolResults`].
#[derive(Debug, thiserror::Error)]
pub enum PruneError {
    /// The conversation holds no items, so there is nothing to prune.
    #[error("cannot prune tool results: conversation is empty")]
    EmptyConversation,

    /// The chat-state actor is dead or dropped the reply.
    #[error("chat-state actor is unavailable")]
    ActorUnavailable,

    /// The canonical Timeline replacement was rejected before becoming
    /// visible to readers.
    #[error(transparent)]
    Timeline(#[from] TimelineWriteError),
}

/// Failure modes for an explicit Surface integrity repair.
#[derive(Debug, thiserror::Error)]
pub enum RepairHistoryError {
    #[error("cannot repair history while a turn is in flight; stop the turn first")]
    TurnActive,
    #[error(transparent)]
    Timeline(#[from] TimelineWriteError),
}

/// Commands sent to the ChatStateActor via mpsc channel.
pub enum ChatStateCommand {
    // ═══ Mutations (fire-and-forget) ═══
    /// Push a user message into the conversation.
    PushUserMessage { item: ConversationItem },

    /// Append a non-message causal fact. Validation and ordering remain owned
    /// by the chat-state actor.
    RecordTimelineEvent { kind: TimelineEventKind },

    /// Prepare, durably commit, then accept a causal boundary. The actor does
    /// not process another command while awaiting this acknowledgement.
    RecordTimelineEventDurably {
        kind: TimelineEventKind,
        reply: oneshot::Sender<Result<crate::TimelineEvent, TimelineWriteError>>,
    },

    /// Persist the exact user-message event, then accept it into Timeline.
    PushUserMessageDurably {
        item: ConversationItem,
        reply: oneshot::Sender<Result<(), TimelineWriteError>>,
    },

    /// Push a user message with an explicit dangling-repair reason.
    PushUserMessageWithRepairReason {
        item: ConversationItem,
        reason: DanglingToolCallReason,
    },

    /// Record the assistant's response (text + tool calls).
    PushAssistantResponse { item: ConversationItem },

    /// Record a tool result.
    PushToolResult { item: ConversationItem },

    /// Record accumulated token usage from a streaming response.
    RecordTokenUsage { total_tokens: u64 },

    /// Stash the per-turn `TokenUsage` from the most recent model response.
    /// Overwrites any previously stashed value.
    RecordLastTurnUsage { usage: TokenUsage },

    RecordModelCallUsage {
        model_id: Option<String>,
        usage: TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    },

    /// Subagent usage into session (and prompt when attributable). Replies when applied.
    RecordSubagentUsage {
        by_model: Vec<(String, crate::usage::UsageTotals)>,
        attribute_to_prompt: bool,
        /// Nested subagent bill may under-count.
        incomplete: bool,
        reply: oneshot::Sender<()>,
    },

    /// Mark open prompt and/or session ledgers incomplete.
    MarkUsageIncomplete {
        prompt: bool,
        session: bool,
        reply: oneshot::Sender<()>,
    },

    /// Update the sampling config (e.g., model switch).
    UpdateSamplingConfig { config: SamplingConfig },

    /// Track that the agent edited a file path.
    RecordAgentEditedPath { path: String },

    /// Record stream timing metadata.
    RecordStreamStart { timestamp_ms: i64 },

    /// Record turn timing metadata.
    RecordTurnStart { timestamp_ms: i64 },

    /// Commit a complete Surface transformation before publishing it to the
    /// actor projection.
    ReplaceSurfaceDurably {
        items: Vec<ConversationItem>,
        cause: MessageCause,
        /// Optimistic guard for transformations computed outside the actor.
        expected_surface_revision: u64,
        reply: oneshot::Sender<Result<(), TimelineWriteError>>,
    },

    /// Commit the exact Surface range declared by the active compaction
    /// summary. Both the optimistic revision and stable range identities are
    /// checked inside the actor before the Timeline event is persisted.
    ReplaceCompactionRangeDurably {
        target: crate::SurfaceRange,
        items: Vec<ConversationItem>,
        expected_surface_revision: u64,
        reply: oneshot::Sender<Result<(), TimelineWriteError>>,
    },

    /// Seed provider token accounting from the session summary. Conversation,
    /// prompt coordinates, and compaction state remain Timeline-derived.
    SeedTokenAccounting {
        total_tokens: u64,
        reply: oneshot::Sender<()>,
    },

    /// Select an earlier prompt boundary as the active Surface. The Timeline
    /// replacement is committed before any derived actor state is changed.
    RewindDurably {
        target_prompt_index: usize,
        reply: oneshot::Sender<Result<(), TimelineWriteError>>,
    },

    /// Atomically replace every canonical user/tool-result image with either
    /// an auxiliary description or an explicit removal marker. Matching and
    /// mutation happen inside the actor so concurrent appends cannot be lost.
    RewriteImagesAndAck {
        rewrites: Vec<ImageRewrite>,
        dropped_placeholder: String,
        reply: oneshot::Sender<Option<ImageRewriteReport>>,
    },

    /// Atomically prune oversized tool-result contents in the stored
    /// conversation (head + marker + tail) and persist the Timeline event.
    /// Runs inside the actor so it
    /// serializes with turn pushes — no read-modify-write race. Idempotent
    /// per plan: already-pruned items are skipped on repeat execution.
    ///
    /// Emits no UI/notification events: the pager renders streamed wire
    /// events, and pruning must not disturb what the user already saw.
    PruneToolResults {
        plan: PrunePlan,
        reply: oneshot::Sender<Result<PruneReport, PruneError>>,
    },

    /// Out-of-band history repair (`grow/session/repair`): run
    /// [`crate::compaction_utils::repair_history`] and persist when changed;
    /// `dry_run` only reports.
    ///
    /// `turn_active` (the session's shared flag, set at turn start BEFORE the
    /// turn pushes anything here) is re-checked inside the command handler:
    /// a caller-side check alone races turn start, whereas at processing time
    /// the command is either refused or runs on pre-turn state with the
    /// turn's pushes serialized after it.
    RepairHistory {
        dry_run: bool,
        turn_active: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        reply: oneshot::Sender<
            Result<crate::compaction_utils::HistoryRepairReport, RepairHistoryError>,
        >,
    },

    /// Atomically align the leading `System` message with `prompt` (inserting
    /// one if absent), persisting the conversation. Executed inside the actor so
    /// it serializes with concurrent turn pushes (`PushAssistantResponse` /
    /// `PushToolResult`) — a mid-turn reconnect cannot lose those updates the
    /// way a read-modify-write via `GetConversation` + `ReplaceConversation`
    /// would. Replies `true` iff the conversation changed (no-op when the head
    /// already matches modulo trailing newlines). A changed head goes through
    /// the Timeline Surface replacement, which re-bases `total_tokens` to a fresh static
    /// estimate — acceptable because a changed head invalidates the KV prefix
    /// anyway.
    ReplaceSystemHead {
        prompt: String,
        reply: oneshot::Sender<Result<bool, TimelineWriteError>>,
    },

    /// Flush pending persistence writes to disk (end of turn).
    Flush,

    /// Update opaque credential secrets held by the actor.
    UpdateCredentials { credentials: Credentials },

    /// Start capturing turn messages. Clears any previous buffer.
    BeginTurnCapture,

    /// Repair dangling tool calls after a harness-initiated halt.
    RepairDanglingAfterHarnessHalt { class: &'static str },

    // ═══ Queries (request/response via oneshot) ═══
    /// Build a ConversationRequest ready to send to the API.
    /// Clones the conversation, prunes old tool results, repairs dangling
    /// tool calls, injects memory reminder, and assembles the request.
    BuildConversationRequest {
        tool_definitions: Vec<ToolSpec>,
        memory_reminder: Option<String>,
        persist_memory_reminder: bool,
        reply: oneshot::Sender<Result<ConversationRequest, TimelineWriteError>>,
    },

    /// Get a clone of the full conversation.
    GetConversation {
        reply: oneshot::Sender<Vec<ConversationItem>>,
    },

    /// Get one coherent compaction input and its optimistic commit revision.
    GetConversationWithRevision {
        reply: oneshot::Sender<(Vec<ConversationItem>, u64)>,
    },

    /// Build the independent debug/read model directly from Timeline.
    GetTrajectory {
        reply: oneshot::Sender<TrajectorySnapshot>,
    },

    /// Atomically freeze a Timeline range and materialize its current Surface.
    MaterializeTimeline {
        timeline_id: String,
        reply: oneshot::Sender<Option<crate::TimelineMaterialization>>,
    },

    /// Atomically freeze the current Timeline range and materialize the
    /// uncompressed transcript of its selected rewind branch for a read-only
    /// recall Sideband.
    MaterializeBranchTranscript {
        timeline_id: String,
        reply: oneshot::Sender<Option<crate::RecallMaterialization>>,
    },

    /// Get current prompt index.
    GetPromptIndex { reply: oneshot::Sender<usize> },

    /// Get the current model-visible Surface revision without cloning it.
    GetSurfaceRevision { reply: oneshot::Sender<u64> },

    /// Get the prompt index at which the last compaction occurred.
    /// `Some` means the context currently holds a compaction summary.
    GetLastCompactionPromptIndex {
        reply: oneshot::Sender<Option<usize>>,
    },

    /// Get total accumulated tokens.
    GetTotalTokens { reply: oneshot::Sender<u64> },

    /// Retrieve the most recent stashed per-turn `TokenUsage`. Returns
    /// `None` until at least one `RecordLastTurnUsage` has been processed.
    GetLastTurnUsage {
        reply: oneshot::Sender<Option<TokenUsage>>,
    },

    GetPromptUsage {
        reply: oneshot::Sender<Option<crate::usage::UsageLedger>>,
    },

    GetSessionUsage {
        reply: oneshot::Sender<crate::usage::UsageLedger>,
    },

    /// `total_tokens` + bytes/4 delta from tool results since last model response.
    GetEstimatedTotalTokens { reply: oneshot::Sender<u64> },

    /// Bytes/4 estimate of all non-system conversation items.
    GetEstimatedMessagesTokens { reply: oneshot::Sender<u64> },

    /// Get sampling config.
    GetSamplingConfig {
        reply: oneshot::Sender<SamplingConfig>,
    },

    /// Get the set of agent-edited file paths.
    GetAgentEditedPaths {
        reply: oneshot::Sender<BTreeSet<String>>,
    },

    /// Get notification meta (timing info).
    GetNotificationMeta {
        reply: oneshot::Sender<NotificationMeta>,
    },

    /// Snapshot state for forking or rewind.
    Snapshot {
        reply: oneshot::Sender<ChatStateSnapshot>,
    },

    /// Check if auto-compact is needed (returns token info).
    CheckAutoCompactNeeded {
        threshold_percent: u8,
        reply: oneshot::Sender<Option<AutoCompactTrigger>>,
    },

    /// Get credential secrets.
    GetCredentials { reply: oneshot::Sender<Credentials> },

    GetLastModelMetadata {
        reply: oneshot::Sender<ModelMetadata>,
    },

    /// Take the accumulated turn messages and end the capture.
    /// Returns `None` if no capture was active.
    TakeTurnMessages {
        reply: oneshot::Sender<Option<TurnCapture>>,
    },

    // ═══ Narrow targeted queries (avoid full-conversation clone) ═══
    /// Get the number of items in the conversation.
    /// Cheaper than `GetConversation` when only the length is needed.
    GetConversationLen { reply: oneshot::Sender<usize> },

    /// Whether any assistant tool call lacks a matching `ToolResult` (i.e. the
    /// dangling-tool-call repair would fire on the next request build).
    /// Cheaper than `GetConversation` when only this predicate is needed.
    HasDanglingToolCalls { reply: oneshot::Sender<bool> },

    /// Get the text content of the last assistant message with non-empty text.
    /// Returns `None` if no such message exists.
    /// Cheaper than `GetConversation` when only the final assistant response is needed.
    GetLastAssistantText {
        reply: oneshot::Sender<Option<String>>,
    },

    /// Like `GetLastAssistantText`, but bounded to the current prompt turn:
    /// returns `None` when the turn produced no assistant text (the walk stops
    /// at the first turn-starting user item).
    GetLastAssistantTextInTurn {
        reply: oneshot::Sender<Option<String>>,
    },

    /// Get the text of the first `Text` content part in the first `User` message.
    /// Returns `None` if the conversation has no user messages or the first user
    /// message has no text content part.
    /// Cheaper than `GetConversation` when only the initial user query is needed.
    GetFirstUserText {
        reply: oneshot::Sender<Option<String>>,
    },

    /// Get a single conversation item by index (0-based).
    /// Returns `None` if the index is out of bounds.
    /// Cheaper than `GetConversation` when only one item is needed.
    GetConversationItemAt {
        index: usize,
        reply: oneshot::Sender<Option<ConversationItem>>,
    },

    /// Get the processed text of the last user query (metadata tags stripped).
    ///
    /// Equivalent to `extract_last_user_query(&conversation)` but without
    /// cloning the full conversation on the caller side.
    GetLastUserQueryText {
        reply: oneshot::Sender<Option<String>>,
    },

    /// Get item counts for the conversation by role.
    ///
    /// Returns a `ConversationCounts` struct without cloning any items.
    /// Suitable for diagnostics / logging that only needs totals.
    GetConversationCounts {
        reply: oneshot::Sender<ConversationCounts>,
    },

    /// Get the first `System` message in the conversation, if any.
    ///
    /// Cheaper than `GetConversation` when only the system prompt is needed
    /// (e.g. for compaction setup or error guards).
    GetSystemMessage {
        reply: oneshot::Sender<Option<ConversationItem>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that every command variant is constructible (compile-time check).
    #[test]
    fn command_variants_are_constructible() {
        // Mutations
        let _ = ChatStateCommand::PushUserMessage {
            item: ConversationItem::user("hello"),
        };
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::PushUserMessageDurably {
            item: ConversationItem::user("hello"),
            reply: tx,
        };
        let _ = ChatStateCommand::PushAssistantResponse {
            item: ConversationItem::assistant("hi"),
        };
        let _ = ChatStateCommand::PushToolResult {
            item: ConversationItem::tool_result("call-1", "result"),
        };
        let _ = ChatStateCommand::RecordTokenUsage { total_tokens: 100 };
        let _ = ChatStateCommand::UpdateSamplingConfig {
            config: SamplingConfig {
                base_url: String::new(),
                model: String::new(),
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
        };
        let _ = ChatStateCommand::RecordAgentEditedPath {
            path: "src/main.rs".to_string(),
        };
        let _ = ChatStateCommand::RecordStreamStart {
            timestamp_ms: 12345,
        };
        let _ = ChatStateCommand::RecordTurnStart {
            timestamp_ms: 12345,
        };
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::ReplaceSurfaceDurably {
            items: vec![],
            cause: MessageCause::ContextRebuild,
            expected_surface_revision: 0,
            reply: tx,
        };
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::SeedTokenAccounting {
            total_tokens: 100,
            reply: tx,
        };
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::RewindDurably {
            target_prompt_index: 0,
            reply: tx,
        };
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::PruneToolResults {
            plan: PrunePlan::default(),
            reply: tx,
        };
        let _ = ChatStateCommand::Flush;

        // Queries
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetConversation { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetPromptIndex { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetLastCompactionPromptIndex { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetTotalTokens { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetEstimatedTotalTokens { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetSamplingConfig { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetAgentEditedPaths { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::BuildConversationRequest {
            tool_definitions: vec![],
            memory_reminder: None,
            persist_memory_reminder: false,
            reply: tx,
        };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetNotificationMeta { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::MaterializeTimeline {
            timeline_id: "main".into(),
            reply: tx,
        };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::MaterializeBranchTranscript {
            timeline_id: "main".into(),
            reply: tx,
        };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::Snapshot { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::CheckAutoCompactNeeded {
            threshold_percent: 85,
            reply: tx,
        };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetLastModelMetadata { reply: tx };

        let _ = ChatStateCommand::BeginTurnCapture;

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::TakeTurnMessages { reply: tx };

        // Narrow targeted queries
        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetConversationLen { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetLastAssistantText { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetLastAssistantTextInTurn { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetFirstUserText { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetConversationItemAt {
            index: 0,
            reply: tx,
        };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetLastUserQueryText { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetConversationCounts { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetSystemMessage { reply: tx };

        let (tx, _rx) = oneshot::channel();
        let _ = ChatStateCommand::GetEstimatedMessagesTokens { reply: tx };
    }
}
