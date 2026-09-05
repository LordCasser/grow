//! Handle to communicate with ChatStateActor.

use std::collections::BTreeSet;

use sampling_types::{
    ConversationItem, ConversationRequest, DanglingToolCallReason, GoalDirectiveTag,
    JsonOutputFormat, SamplingConfig, TokenUsage, ToolSpec,
};
use tokio::sync::{mpsc, oneshot};

use crate::commands::{
    ChatStateCommand, ConditionalToolResultOutcome, ImageProjectionReport, PruneError, PruneReport,
    RepairHistoryError, TimelineWriteError,
};
use crate::types::{
    AutoCompactTrigger, ChatStateSnapshot, ConversationCounts, Credentials, NotificationMeta,
    TurnCapture,
};
use crate::{TimelineEventKind, TrajectorySnapshot};

/// Handle to communicate with ChatStateActor.
/// This is cheap to clone and can be shared across tasks.
#[derive(Clone)]
pub struct ChatStateHandle {
    cmd_tx: mpsc::UnboundedSender<ChatStateCommand>,
    _lifetime: std::sync::Arc<HandleLifetime>,
}

struct HandleLifetime {
    cancellation_token: tokio_util::sync::CancellationToken,
}

impl Drop for HandleLifetime {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

impl ChatStateHandle {
    /// Create a new handle with the given command sender.
    pub(crate) fn new(
        cmd_tx: mpsc::UnboundedSender<ChatStateCommand>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self {
            cmd_tx,
            _lifetime: std::sync::Arc::new(HandleLifetime { cancellation_token }),
        }
    }

    /// Create a no-op handle that discards all commands.
    /// Useful for tests and situations where chat state tracking is not needed.
    pub fn noop() -> Self {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        Self::new(cmd_tx, tokio_util::sync::CancellationToken::new())
    }

    /// Whether the actor mailbox has closed. A permanent Timeline writer
    /// failure closes it to prevent a later fact from reusing an uncommitted
    /// sequence number.
    pub fn is_closed(&self) -> bool {
        self.cmd_tx.is_closed()
    }

    // ═══ Fire-and-forget mutations ═══

    /// Push a user message into the conversation.
    pub fn push_user_message(&self, item: ConversationItem) {
        let _ = self.cmd_tx.send(ChatStateCommand::PushUserMessage { item });
    }

    pub fn record_timeline_event(&self, kind: TimelineEventKind) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RecordTimelineEvent { kind });
    }

    pub async fn record_timeline_event_durably(
        &self,
        kind: TimelineEventKind,
    ) -> Result<crate::TimelineEvent, TimelineWriteError> {
        self.query("RecordTimelineEventDurably", |reply| {
            ChatStateCommand::RecordTimelineEventDurably { kind, reply }
        })
        .await
        .unwrap_or(Err(TimelineWriteError::AcknowledgementLost))
    }

    pub async fn receive_notification_durably(
        &self,
        owner_session_id: String,
        source: crate::NotificationSource,
        source_version: crate::NotificationSourceVersion,
        payload_ref: crate::NotificationPayloadRef,
    ) -> Result<crate::TimelineEvent, TimelineWriteError> {
        self.query("ReceiveNotificationDurably", |reply| {
            ChatStateCommand::ReceiveNotificationDurably {
                owner_session_id,
                source,
                source_version,
                payload_ref,
                reply,
            }
        })
        .await
        .unwrap_or(Err(TimelineWriteError::AcknowledgementLost))
    }

    pub async fn submit_input_durably(
        &self,
        input_id: String,
        intent: crate::InputIntent,
        payload_ref: crate::InputPayloadRef,
    ) -> Result<crate::TimelineEvent, TimelineWriteError> {
        self.query("SubmitInputDurably", |reply| {
            ChatStateCommand::SubmitInputDurably {
                input_id,
                intent,
                payload_ref,
                reply,
            }
        })
        .await
        .unwrap_or(Err(TimelineWriteError::AcknowledgementLost))
    }

    pub async fn recover_interrupted_durably(
        &self,
    ) -> Result<Vec<crate::TimelineEvent>, TimelineWriteError> {
        self.query("RecoverInterruptedDurably", |reply| {
            ChatStateCommand::RecoverInterruptedDurably { reply }
        })
        .await
        .unwrap_or(Err(TimelineWriteError::AcknowledgementLost))
    }

    pub async fn settle_open_compaction_durably(
        &self,
        reason: impl Into<String>,
    ) -> Result<Option<crate::TimelineEvent>, TimelineWriteError> {
        let reason = reason.into();
        self.query("SettleOpenCompactionDurably", |reply| {
            ChatStateCommand::SettleOpenCompactionDurably { reason, reply }
        })
        .await
        .unwrap_or(Err(TimelineWriteError::AcknowledgementLost))
    }

    /// Push a user message and return only after its Timeline event is durable.
    pub async fn push_user_message_durably(
        &self,
        item: ConversationItem,
    ) -> Result<(), TimelineWriteError> {
        self.query("PushUserMessageDurably", |reply| {
            ChatStateCommand::PushUserMessageDurably { item, reply }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    /// Push a user message with an explicit dangling-repair reason.
    pub fn push_user_message_with_repair_reason(
        &self,
        item: ConversationItem,
        reason: DanglingToolCallReason,
    ) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::PushUserMessageWithRepairReason { item, reason });
    }

    /// Record the assistant's response.
    pub fn push_assistant_response(&self, item: ConversationItem) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::PushAssistantResponse { item });
    }

    /// Return the number of quarantined exchanges only after both the raw
    /// response and its repair are durable. A nonzero result forbids dispatch.
    pub async fn push_response_durably(
        &self,
        items: Vec<ConversationItem>,
        native_continuation: Option<sampling_types::NativeContinuationFragment>,
    ) -> Result<usize, TimelineWriteError> {
        self.query("PushResponseDurably", |reply| {
            ChatStateCommand::PushResponseDurably {
                items,
                native_continuation,
                reply,
            }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    /// Record a tool result.
    pub fn push_tool_result(&self, item: ConversationItem) {
        let _ = self.cmd_tx.send(ChatStateCommand::PushToolResult { item });
    }

    pub async fn push_tool_result_conditionally(
        &self,
        item: ConversationItem,
        rejection_item: ConversationItem,
        expected_surface_revision: u64,
        max_context_tokens: u64,
        max_result_tokens: u64,
    ) -> Result<ConditionalToolResultOutcome, TimelineWriteError> {
        self.query("PushToolResultConditionally", |reply| {
            ChatStateCommand::PushToolResultConditionally {
                item,
                rejection_item,
                expected_surface_revision,
                max_context_tokens,
                max_result_tokens,
                reply,
            }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    /// Record the provider's canonical current-context total.
    pub fn record_provider_context_anchor(&self, provider_total_tokens: u64) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RecordProviderContextAnchor {
                provider_total_tokens,
            });
    }

    /// Stash the per-turn `TokenUsage` from the most recent model response.
    /// Fire-and-forget — no ack returned.
    pub fn record_last_turn_usage(&self, usage: TokenUsage) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RecordLastTurnUsage { usage });
    }

    pub fn record_model_call_usage(
        &self,
        model_id: Option<String>,
        usage: TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let _ = self.cmd_tx.send(ChatStateCommand::RecordModelCallUsage {
            model_id,
            usage,
            api_duration_ms,
            cost_usd_ticks,
        });
    }

    /// Apply subagent usage; returns false if the actor did not acknowledge.
    pub async fn record_subagent_usage(
        &self,
        by_model: Vec<(String, crate::usage::UsageTotals)>,
        attribute_to_prompt: bool,
        incomplete: bool,
    ) -> bool {
        self.query("RecordSubagentUsage", |reply| {
            ChatStateCommand::RecordSubagentUsage {
                by_model,
                attribute_to_prompt,
                incomplete,
                reply,
            }
        })
        .await
        .is_some()
    }

    /// Mark open prompt and/or session ledgers incomplete.
    pub async fn mark_usage_incomplete(&self, prompt: bool, session: bool) -> bool {
        self.query("MarkUsageIncomplete", |reply| {
            ChatStateCommand::MarkUsageIncomplete {
                prompt,
                session,
                reply,
            }
        })
        .await
        .is_some()
    }

    /// Replace the active provider route and start a fresh continuation epoch.
    pub fn replace_sampling_route(&self, config: SamplingConfig) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::ReplaceSamplingRoute { config });
    }

    /// Update sampling parameters without invalidating same-route continuation.
    pub fn update_sampling_config(&self, config: SamplingConfig) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::UpdateSamplingConfig { config });
    }

    /// Acknowledged continuation reset used before a silent portable retry.
    pub async fn reset_continuation(&self) -> bool {
        self.query("ResetContinuation", |reply| {
            ChatStateCommand::ResetContinuation { reply }
        })
        .await
        .is_some()
    }

    /// Track that the agent edited a file path.
    pub fn record_agent_edited_path(&self, path: String) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RecordAgentEditedPath { path });
    }

    /// Record stream timing metadata.
    pub fn record_stream_start(&self, timestamp_ms: i64) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RecordStreamStart { timestamp_ms });
    }

    /// Record turn timing metadata.
    pub fn record_turn_start(&self, timestamp_ms: i64) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RecordTurnStart { timestamp_ms });
    }

    /// Rebuild runtime context as one durable Timeline Surface event.
    pub async fn replace_context_durably(
        &self,
        items: Vec<ConversationItem>,
        expected_surface_revision: u64,
    ) -> Result<(), TimelineWriteError> {
        self.send_replace_durably(
            items,
            crate::MessageCause::ContextRebuild,
            expected_surface_revision,
        )
        .await
    }

    /// Shadow one summary-declared Surface range for compaction.
    pub async fn replace_compaction_range(
        &self,
        target: crate::SurfaceRange,
        items: Vec<ConversationItem>,
    ) -> Result<(), TimelineWriteError> {
        self.query("ReplaceCompactionRangeDurably", |reply| {
            ChatStateCommand::ReplaceCompactionRangeDurably {
                target,
                items,
                reply,
            }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    pub async fn rewind_durably(
        &self,
        target_prompt_index: usize,
    ) -> Result<(), TimelineWriteError> {
        self.query("RewindDurably", |reply| ChatStateCommand::RewindDurably {
            target_prompt_index,
            reply,
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    async fn send_replace_durably(
        &self,
        items: Vec<ConversationItem>,
        cause: crate::MessageCause,
        expected_surface_revision: u64,
    ) -> Result<(), TimelineWriteError> {
        self.query("ReplaceSurfaceDurably", |reply| {
            ChatStateCommand::ReplaceSurfaceDurably {
                items,
                cause,
                expected_surface_revision,
                reply,
            }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    /// Durably record irreversible model-facing ImageShadows while preserving
    /// source images only as immutable Timeline evidence. The actor rejects
    /// stale source revisions.
    pub async fn record_image_projection_and_ack(
        &self,
        projection: crate::ImageProjectionEvent,
    ) -> Result<ImageProjectionReport, TimelineWriteError> {
        self.query("RecordImageProjectionAndAck", |reply| {
            ChatStateCommand::RecordImageProjectionAndAck { projection, reply }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    /// Prune the tool results selected by `plan` inside the actor and await
    /// the report. The actor trims each selected item's content to
    /// head + marker + tail, projects the signed Surface token delta, and
    /// persists the canonical Timeline replacement.
    ///
    /// Returns `Err(PruneError::ActorUnavailable)` when the actor is dead or
    /// drops the reply — pruning is never silently skipped.
    pub async fn prune_tool_results(
        &self,
        plan: compaction::PrunePlan,
    ) -> Result<PruneReport, PruneError> {
        self.query("PruneToolResults", |reply| {
            ChatStateCommand::PruneToolResults { plan, reply }
        })
        .await
        .ok_or(PruneError::ActorUnavailable)?
    }

    /// Out-of-band history repair (`grow/session/repair`); see
    /// [`ChatStateCommand::RepairHistory`]. Returns `None` if the actor is
    /// dead, `Some(Err(_))` if a turn was in flight at processing time.
    pub async fn repair_history(
        &self,
        dry_run: bool,
        turn_active: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Option<Result<crate::compaction_utils::HistoryRepairReport, RepairHistoryError>> {
        self.query("RepairHistory", |reply| ChatStateCommand::RepairHistory {
            dry_run,
            turn_active,
            reply,
        })
        .await
    }

    /// Flush pending persistence writes to disk.
    pub fn flush(&self) {
        let _ = self.cmd_tx.send(ChatStateCommand::Flush);
    }

    /// Update opaque credential secrets held by the actor.
    pub fn update_credentials(&self, credentials: Credentials) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::UpdateCredentials { credentials });
    }

    /// Begin capturing turn messages. Call at the start of a real user turn
    /// (in `handle_prompt`), before `push_user_message`.
    pub fn begin_turn_capture(&self) {
        let _ = self.cmd_tx.send(ChatStateCommand::BeginTurnCapture);
    }

    /// Repair dangling tool calls after a harness-initiated halt.
    pub fn repair_dangling_after_harness_halt(&self, class: &'static str) {
        let _ = self
            .cmd_tx
            .send(ChatStateCommand::RepairDanglingAfterHarnessHalt { class });
    }

    // ═══ Async queries (via oneshot) ═══

    /// Send a query to the actor and await the reply.
    ///
    /// Returns `None` when the actor is dead (channel send failure or reply
    /// dropped due to panic/cancellation). Both failure modes are logged at
    /// `error` level with `cmd_name` for post-mortem diagnostics.
    async fn query<T>(
        &self,
        cmd_name: &str,
        make_cmd: impl FnOnce(oneshot::Sender<T>) -> ChatStateCommand,
    ) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        if self.cmd_tx.send(make_cmd(tx)).is_err() {
            tracing::error!(cmd_name, "ChatStateActor dead: send failed");
            return None;
        }
        match rx.await {
            Ok(v) => Some(v),
            Err(_) => {
                tracing::error!(cmd_name, "ChatStateActor dead: reply dropped");
                None
            }
        }
    }

    /// Build the final provider request from current Surface plus request-only
    /// Goal/image/schema projections, updating context pressure atomically.
    pub async fn build_request(
        &self,
        timeline_id: &str,
        tool_definitions: Vec<ToolSpec>,
        memory_reminder: Option<String>,
        active_goal: Option<GoalDirectiveTag>,
        json_output: Option<JsonOutputFormat>,
    ) -> Result<ConversationRequest, TimelineWriteError> {
        self.query("BuildConversationRequest", |reply| {
            ChatStateCommand::BuildConversationRequest {
                timeline_id: timeline_id.to_owned(),
                tool_definitions,
                memory_reminder,
                active_goal,
                json_output,
                reply,
            }
        })
        .await
        .ok_or(TimelineWriteError::AcknowledgementLost)?
    }

    /// Get a clone of the full conversation.
    pub async fn get_conversation(&self) -> Vec<ConversationItem> {
        self.query("GetConversation", |reply| {
            ChatStateCommand::GetConversation { reply }
        })
        .await
        .unwrap_or_default()
    }

    /// Get a coherent Surface plus the revision required for a later
    /// optimistic compaction commit.
    pub async fn get_conversation_with_revision(&self) -> Option<(Vec<ConversationItem>, u64)> {
        self.query("GetConversationWithRevision", |reply| {
            ChatStateCommand::GetConversationWithRevision { reply }
        })
        .await
    }

    pub async fn trajectory(&self) -> Option<TrajectorySnapshot> {
        self.query("GetTrajectory", |reply| ChatStateCommand::GetTrajectory {
            reply,
        })
        .await
    }

    /// Read a completed or in-flight Hook occurrence from the Timeline fold.
    pub async fn hook_projection(
        &self,
        occurrence_id: impl Into<String>,
    ) -> Option<crate::HookLifecycleProjection> {
        let occurrence_id = occurrence_id.into();
        self.query("GetHookProjection", |reply| {
            ChatStateCommand::GetHookProjection {
                occurrence_id,
                reply,
            }
        })
        .await
        .flatten()
    }

    /// Read every durably completed Hook occurrence in completion order.
    pub async fn completed_hook_projections(&self) -> Vec<crate::HookLifecycleProjection> {
        self.query("GetCompletedHookProjections", |reply| {
            ChatStateCommand::GetCompletedHookProjections { reply }
        })
        .await
        .unwrap_or_default()
    }

    pub async fn timeline_events(&self) -> Option<Vec<crate::TimelineEvent>> {
        self.query("GetTimelineEvents", |reply| {
            ChatStateCommand::GetTimelineEvents { reply }
        })
        .await
    }

    pub async fn pending_notifications(&self) -> Option<Vec<crate::PendingNotification>> {
        self.query("GetPendingNotifications", |reply| {
            ChatStateCommand::GetPendingNotifications { reply }
        })
        .await
    }

    pub async fn get_pending_allowed_inputs(&self) -> Option<Vec<crate::PendingAllowedInput>> {
        self.query("GetPendingAllowedInputs", |reply| {
            ChatStateCommand::GetPendingAllowedInputs { reply }
        })
        .await
    }

    pub async fn submitted_input_payload_hashes(&self) -> Option<BTreeSet<String>> {
        self.query("GetSubmittedInputPayloadHashes", |reply| {
            ChatStateCommand::GetSubmittedInputPayloadHashes { reply }
        })
        .await
    }

    /// Query the immutable receipt fold rather than the pending projection.
    /// The outer `Option` reports actor availability; the inner value remains
    /// present after the receipt is consumed.
    pub async fn received_notification_id(
        &self,
        source: crate::NotificationSource,
        source_version: crate::NotificationSourceVersion,
    ) -> Option<Option<String>> {
        self.query("GetReceivedNotificationId", |reply| {
            ChatStateCommand::GetReceivedNotificationId {
                source,
                source_version,
                reply,
            }
        })
        .await
    }

    /// Freeze the reference and materialize exactly that committed Surface in
    /// one actor command, so auxiliary assembly cannot race a parent append.
    pub async fn materialize_timeline(
        &self,
        timeline_id: String,
    ) -> Option<crate::TimelineMaterialization> {
        self.query("MaterializeTimeline", |reply| {
            ChatStateCommand::MaterializeTimeline { timeline_id, reply }
        })
        .await
        .flatten()
    }

    /// Freeze and materialize the uncompressed transcript for the currently
    /// selected branch. Recall Sidebands consume this projection without
    /// changing the model-facing Surface.
    pub async fn materialize_branch_transcript(
        &self,
        timeline_id: String,
    ) -> Option<crate::RecallMaterialization> {
        self.query("MaterializeBranchTranscript", |reply| {
            ChatStateCommand::MaterializeBranchTranscript { timeline_id, reply }
        })
        .await
        .flatten()
    }

    /// Get current prompt index.
    pub async fn get_prompt_index(&self) -> usize {
        self.query("GetPromptIndex", |reply| ChatStateCommand::GetPromptIndex {
            reply,
        })
        .await
        .unwrap_or(0)
    }

    /// Read the current Surface revision without cloning conversation data.
    pub async fn get_surface_revision(&self) -> Option<u64> {
        self.query("GetSurfaceRevision", |reply| {
            ChatStateCommand::GetSurfaceRevision { reply }
        })
        .await
    }

    /// Get the prompt index at which the last compaction occurred.
    /// `Some` means the context currently holds a compaction summary.
    pub async fn get_last_compaction_prompt_index(&self) -> Option<usize> {
        self.query("GetLastCompactionPromptIndex", |reply| {
            ChatStateCommand::GetLastCompactionPromptIndex { reply }
        })
        .await
        .flatten()
    }

    /// Get provider-anchored projected context pressure.
    pub async fn get_projected_tokens(&self) -> u64 {
        self.query("GetProjectedTokens", |reply| {
            ChatStateCommand::GetProjectedTokens { reply }
        })
        .await
        .unwrap_or(0)
    }

    /// Retrieve the most recent stashed per-turn `TokenUsage`. Returns
    /// `None` if no model turn has completed in this session yet, or if
    /// the actor channel is closed.
    pub async fn get_last_turn_usage(&self) -> Option<TokenUsage> {
        self.query("GetLastTurnUsage", |reply| {
            ChatStateCommand::GetLastTurnUsage { reply }
        })
        .await
        .flatten()
    }

    /// Fail-closed prompt bill read.
    /// `Ok(None)` means the actor answered "no ledger"; `Err(())` means it did
    /// not answer at all. Never collapse `Err` to `None`: an unreadable bill
    /// must not be mistaken for a free prompt.
    pub async fn try_get_prompt_usage(&self) -> Result<Option<crate::usage::UsageLedger>, ()> {
        self.query("GetPromptUsage", |reply| ChatStateCommand::GetPromptUsage {
            reply,
        })
        .await
        .ok_or(())
    }

    /// Fail-closed session bill read. `Err(())` if the actor is dead.
    pub async fn try_get_session_usage(&self) -> Result<crate::usage::UsageLedger, ()> {
        self.query("GetSessionUsage", |reply| {
            ChatStateCommand::GetSessionUsage { reply }
        })
        .await
        .ok_or(())
    }

    /// Bytes/4 estimate of all non-system conversation items.
    pub async fn get_estimated_messages_tokens(&self) -> u64 {
        self.query("GetEstimatedMessagesTokens", |reply| {
            ChatStateCommand::GetEstimatedMessagesTokens { reply }
        })
        .await
        .unwrap_or(0)
    }

    /// Get sampling config.
    pub async fn get_sampling_config(&self) -> Option<SamplingConfig> {
        self.query("GetSamplingConfig", |reply| {
            ChatStateCommand::GetSamplingConfig { reply }
        })
        .await
    }

    /// Get the set of agent-edited file paths.
    pub async fn get_agent_edited_paths(&self) -> BTreeSet<String> {
        self.query("GetAgentEditedPaths", |reply| {
            ChatStateCommand::GetAgentEditedPaths { reply }
        })
        .await
        .unwrap_or_default()
    }

    /// Get notification meta (timing info).
    pub async fn get_notification_meta(&self) -> Option<NotificationMeta> {
        self.query("GetNotificationMeta", |reply| {
            ChatStateCommand::GetNotificationMeta { reply }
        })
        .await
    }

    /// Snapshot state for forking or rewind.
    pub async fn snapshot(&self) -> Option<ChatStateSnapshot> {
        self.query("Snapshot", |reply| ChatStateCommand::Snapshot { reply })
            .await
    }

    /// Truncate conversation to a target prompt index (for rewind).
    /// Get credential secrets.
    pub async fn get_credentials(&self) -> Credentials {
        self.query("GetCredentials", |reply| ChatStateCommand::GetCredentials {
            reply,
        })
        .await
        .unwrap_or_default()
    }

    pub async fn get_last_model_metadata(&self) -> crate::commands::ModelMetadata {
        self.query("GetLastModelMetadata", |reply| {
            ChatStateCommand::GetLastModelMetadata { reply }
        })
        .await
        .unwrap_or_default()
    }

    /// Take the accumulated turn messages and end the capture.
    /// Returns `None` if no capture was active.
    pub async fn take_turn_messages(&self) -> Option<TurnCapture> {
        self.query("TakeTurnMessages", |reply| {
            ChatStateCommand::TakeTurnMessages { reply }
        })
        .await
        .flatten()
    }

    /// Check if auto-compact is needed.
    pub async fn check_auto_compact_needed(
        &self,
        threshold_percent: u8,
    ) -> Option<AutoCompactTrigger> {
        self.query("CheckAutoCompactNeeded", |reply| {
            ChatStateCommand::CheckAutoCompactNeeded {
                threshold_percent,
                reply,
            }
        })
        .await
        .flatten()
    }

    // ═══ Narrow targeted queries ═══

    /// Get the number of items in the conversation.
    ///
    /// Cheaper than [`get_conversation`] when only the length is needed —
    /// the actor returns a single `usize` without cloning any items.
    pub async fn get_conversation_len(&self) -> usize {
        self.query("GetConversationLen", |reply| {
            ChatStateCommand::GetConversationLen { reply }
        })
        .await
        .unwrap_or(0)
    }

    /// Whether any assistant tool call lacks a matching `ToolResult` (the
    /// dangling-tool-call repair would fire on the next request build).
    ///
    /// Returns `false` if the actor is dead. Cheaper than [`get_conversation`]
    /// — the actor scans in place and returns a single `bool`.
    pub async fn has_dangling_tool_calls(&self) -> bool {
        self.query("HasDanglingToolCalls", |reply| {
            ChatStateCommand::HasDanglingToolCalls { reply }
        })
        .await
        .unwrap_or(false)
    }

    /// Get the text content of the last assistant message with non-empty text.
    ///
    /// Returns `None` if no such message exists or the actor is dead.
    /// Cheaper than [`get_conversation`] when only the final assistant
    /// response text is needed.
    pub async fn get_last_assistant_text(&self) -> Option<String> {
        self.query("GetLastAssistantText", |reply| {
            ChatStateCommand::GetLastAssistantText { reply }
        })
        .await
        .flatten()
    }

    /// Get the current turn's last assistant message text, or `None` when the
    /// turn produced none (or the actor is dead). Turn-scoped, unlike
    /// [`get_last_assistant_text`], and cheaper than [`get_conversation`].
    ///
    /// [`get_conversation`]: Self::get_conversation
    /// [`get_last_assistant_text`]: Self::get_last_assistant_text
    pub async fn get_last_assistant_text_in_turn(&self) -> Option<String> {
        self.query("GetLastAssistantTextInTurn", |reply| {
            ChatStateCommand::GetLastAssistantTextInTurn { reply }
        })
        .await
        .flatten()
    }

    /// Get the text of the first `Text` content part in the first `User` message.
    ///
    /// Returns `None` if no user message with text content exists or the actor
    /// is dead. Cheaper than [`get_conversation`] when only the initial user
    /// query text is needed (e.g. for memory context search).
    pub async fn get_first_user_text(&self) -> Option<String> {
        self.query("GetFirstUserText", |reply| {
            ChatStateCommand::GetFirstUserText { reply }
        })
        .await
        .flatten()
    }

    /// Get a single conversation item by index (0-based).
    ///
    /// Returns `None` if the index is out of bounds or the actor is dead.
    /// Cheaper than [`get_conversation`] when only one specific item is needed
    /// (e.g. item[1] for the original user-info block after compaction).
    pub async fn get_conversation_item_at(&self, index: usize) -> Option<ConversationItem> {
        self.query("GetConversationItemAt", |reply| {
            ChatStateCommand::GetConversationItemAt { index, reply }
        })
        .await
        .flatten()
    }

    /// Get the processed text of the last user query (metadata tags stripped).
    ///
    /// Equivalent to `extract_last_user_query(&full_conv)` but without cloning
    /// the full conversation. Returns `None` if there are no user messages or
    /// the last user message is empty after processing.
    pub async fn get_last_user_query_text(&self) -> Option<String> {
        self.query("GetLastUserQueryText", |reply| {
            ChatStateCommand::GetLastUserQueryText { reply }
        })
        .await
        .flatten()
    }

    /// Get item counts for the conversation by role.
    ///
    /// Returns a [`ConversationCounts`] struct without cloning any items.
    /// Suitable for diagnostics / logging that only needs totals.
    pub async fn get_conversation_counts(&self) -> ConversationCounts {
        self.query("GetConversationCounts", |reply| {
            ChatStateCommand::GetConversationCounts { reply }
        })
        .await
        .unwrap_or_default()
    }

    /// Get the first `System` message in the conversation, if any.
    ///
    /// Cheaper than [`get_conversation`] when only the system prompt is needed
    /// (e.g. for compaction setup or error validation).
    pub async fn get_system_message(&self) -> Option<ConversationItem> {
        self.query("GetSystemMessage", |reply| {
            ChatStateCommand::GetSystemMessage { reply }
        })
        .await
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_handle_does_not_panic() {
        let handle = ChatStateHandle::noop();
        handle.push_user_message(ConversationItem::user("test"));
        handle.flush();
        drop(handle);
    }

    #[test]
    fn handle_is_clone() {
        let handle = ChatStateHandle::noop();
        let clone = handle.clone();
        clone.push_user_message(ConversationItem::user("from clone"));
    }
}
