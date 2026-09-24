//! Mutation handlers for the ChatStateActor.

use sampling_types::{
    ConversationItem, DanglingToolCallReason, NativeContinuationFragment, TokenUsage,
};

use super::ChatStateActor;
use crate::MessageCause;
use crate::actor::state::{
    AttemptUsageSettlement, AuxiliaryAttemptKey, AuxiliaryAttemptSettlement, AuxiliaryAttemptStart,
    SubagentUsageSettlement,
};
use crate::events::ChatStateEvent;

/// Static string label for tracing on `ConversationItem` (avoids pulling
/// the `Role` enum into the format string).
fn item_kind_str(item: &ConversationItem) -> &'static str {
    match item {
        ConversationItem::System(_) => "system",
        ConversationItem::User(_) => "user",
        ConversationItem::Assistant(_) => "assistant",
        ConversationItem::ToolResult(_) => "tool_result",
        ConversationItem::AgentMessage(_) => "agent_message",
        ConversationItem::BackendToolCall(_) => "backend_tool_call",
        ConversationItem::Reasoning(_) => "reasoning",
    }
}

fn message_cause(item: &ConversationItem) -> Result<MessageCause, crate::TimelineError> {
    match item {
        ConversationItem::System(_) | ConversationItem::AgentMessage(_) => {
            Err(crate::TimelineError::InvalidMessageShape)
        }
        ConversationItem::User(user)
            if matches!(
                user.synthetic_reason.as_ref(),
                Some(
                    sampling_types::SyntheticReason::ProjectInstructions
                        | sampling_types::SyntheticReason::SessionRules
                        | sampling_types::SyntheticReason::MemoryContext
                )
            ) =>
        {
            Err(crate::TimelineError::InvalidMessageShape)
        }
        ConversationItem::User(user) => Ok(match user.permission_evidence {
            Some(sampling_types::PermissionEvidence::DirectUser { .. }) => MessageCause::DirectUser,
            Some(sampling_types::PermissionEvidence::Interjection { .. }) => {
                MessageCause::Interjection
            }
            None => MessageCause::User,
        }),
        ConversationItem::Assistant(_)
        | ConversationItem::BackendToolCall(_)
        | ConversationItem::Reasoning(_) => Ok(MessageCause::Assistant),
        ConversationItem::ToolResult(_) => Ok(MessageCause::ToolResult),
    }
}

/// Marker inserted between the head and tail of a pruned tool-result text.
///
/// Chosen so genuine tool output essentially never contains it verbatim:
/// idempotency checks and tests recognize already-pruned content by its
/// presence. It must stay under the marker room of realistic per-item
/// budgets (a quarter of `budget_tokens * 4` bytes); [`compaction`]'s prune
/// clips an oversized marker to its reserved room on a char boundary, so a
/// longer marker degrades gracefully rather than eating the head/tail shares.
pub(super) const PRUNE_MARKER: &str = "\n\n[... tool result middle pruned ...]\n\n";

impl ChatStateActor {
    async fn append_message_fact(&mut self, item: ConversationItem) -> bool {
        let Ok(cause) = message_cause(&item) else {
            tracing::error!(
                item_kind = item_kind_str(&item),
                "rejected message outside its Timeline-owned write path"
            );
            return false;
        };
        let item_tokens = super::state::estimate_item_tokens(&item);
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Messages(crate::MessageEvent {
                cause,
                items: vec![item],
                surface: crate::SurfaceOp::Append,
                response_admission: None,
            }))
            .expect("an assembled conversation item must append to the timeline");
        let committed = self.commit_buffered_timeline_event(event).await;
        if committed {
            self.apply_projected_token_delta(0, item_tokens);
        }
        committed
    }

    /// Repair malformed exchanges and tool pairing at a closed write boundary.
    ///
    /// A "dangling" tool call is an assistant message with tool call IDs that
    /// lack matching `ToolResult` entries. This can happen when:
    /// - The user cancels (Ctrl+C) mid-tool-execution in a live session
    /// - The process crashes between pushing the assistant and tool results
    /// - The tokio task is aborted at an `.await` point
    ///
    /// This method repairs the state in-place and persists the fix to disk.
    /// It is idempotent — a clean conversation produces no Timeline event.
    ///
    /// Only call at write boundaries where the previous turn is definitively
    /// over (`push_user_message()` or a harness-declared halt).
    /// Do NOT call from read handlers — background tasks run concurrently with
    /// tool execution and would misidentify in-flight calls as dangling.
    /// Use the buffered append path for boundaries without a caller acknowledgement.
    pub(super) async fn ensure_conversation_integrity_with_reason(
        &mut self,
        reason: DanglingToolCallReason,
    ) {
        let mut conversation = self.state.timeline.surface().to_vec();
        let report = crate::compaction_utils::repair_history_with_reason(&mut conversation, reason);
        if report.changed() {
            tracing::info!(?report, "Repaired conversation at a closed write boundary");
            self.install_conversation_buffered(conversation, MessageCause::IntegrityRepair)
                .await;
        }
    }

    /// Repair the current Surface through the acknowledged Timeline path.
    /// Callers that are themselves durable boundaries must never expose a
    /// repair that the ledger did not commit.
    pub(super) async fn ensure_conversation_integrity_durably(
        &mut self,
        reason: DanglingToolCallReason,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let mut conversation = self.state.timeline.surface().to_vec();
        let report = crate::compaction_utils::repair_history_with_reason(&mut conversation, reason);
        if !report.changed() {
            return Ok(());
        }
        tracing::info!(?report, "Repaired conversation before durable boundary");
        self.replace_conversation_durably(conversation, MessageCause::IntegrityRepair)
            .await
    }

    /// The command loop remains serialized across the two durable appends:
    /// raw response first, deterministic repair second. A crash between them
    /// leaves recover_surface_integrity a complete, replayable source fact.
    /// Healthy pending calls must NOT receive synthetic results here.
    pub(super) async fn push_response_durably(
        &mut self,
        items: Vec<ConversationItem>,
        response_admission: Option<crate::ResponseAdmissionIdentity>,
        native_continuation: Option<NativeContinuationFragment>,
    ) -> Result<usize, crate::commands::TimelineWriteError> {
        if items.is_empty() {
            self.state.continuation.reset(
                self.state.sampling_config.api_backend.clone(),
                self.state.timeline.surface_len(),
                "empty_response_admission",
            );
            return Ok(0);
        }

        if let Some(response_admission) = response_admission.as_ref()
            && let Some(existing) = self.state.timeline.response_admission(response_admission)
        {
            let same_items = serde_json::to_value(&existing.items)
                .expect("response items must serialize")
                == serde_json::to_value(&items).expect("response items must serialize");
            if !same_items {
                return Err(crate::commands::TimelineWriteError::ResponseAdmissionConflict);
            }
            let quarantined = existing
                .response_admission
                .as_ref()
                .expect("response admission lookup always has metadata")
                .quarantined_tool_exchanges;
            if crate::compaction_utils::has_malformed_tool_identity(self.state.timeline.surface()) {
                let mut repaired = self.state.timeline.surface().to_vec();
                let repaired_count =
                    crate::compaction_utils::quarantine_malformed_tool_exchanges(&mut repaired);
                debug_assert!(repaired_count > 0);
                self.replace_conversation_durably(repaired, MessageCause::IntegrityRepair)
                    .await?;
            }
            return Ok(quarantined);
        }

        let first_new_index = self.state.timeline.surface_len();
        let tokens = super::state::estimate_conversation_tokens(&items);
        let repair = if crate::compaction_utils::has_malformed_tool_identity(
            self.state.timeline.surface().iter().chain(&items),
        ) {
            let mut candidate = self.state.timeline.surface().to_vec();
            candidate.extend(items.iter().cloned());
            let quarantined =
                crate::compaction_utils::quarantine_malformed_tool_exchanges(&mut candidate);
            debug_assert!(quarantined > 0);
            Some((candidate, quarantined))
        } else {
            None
        };
        let quarantined = repair.as_ref().map_or(0, |(_, count)| *count);
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Messages(crate::MessageEvent {
                cause: MessageCause::Assistant,
                items,
                surface: crate::SurfaceOp::Append,
                response_admission: response_admission.map(|identity| crate::ResponseAdmission {
                    identity,
                    quarantined_tool_exchanges: quarantined,
                }),
            }))?;
        self.commit_timeline_event(event).await?;
        self.apply_projected_token_delta(0, tokens);

        if let Some((candidate, _)) = repair {
            self.replace_conversation_durably(candidate, MessageCause::IntegrityRepair)
                .await?;
            return Ok(quarantined);
        }

        let surface_ids = self.state.timeline.surface_ids()[first_new_index..].to_vec();
        let installed = native_continuation
            .is_some_and(|fragment| self.state.continuation.install(surface_ids, fragment));
        if !installed {
            self.state.continuation.reset(
                self.state.sampling_config.api_backend.clone(),
                self.state.timeline.surface_len(),
                "native_continuation_missing_or_mismatched",
            );
        }
        Ok(quarantined)
    }

    /// Repair dangling tool calls after a harness-initiated halt.
    pub(super) async fn repair_dangling_after_harness_halt(&mut self, class: &'static str) {
        self.ensure_conversation_integrity_with_reason(DanglingToolCallReason::HarnessHalted {
            class,
        })
        .await;
    }

    /// Out-of-band history repair (`grow/session/repair`): run
    /// [`crate::compaction_utils::repair_history`] and persist changes via
    /// the Timeline replacement transaction, using the same quarantine and
    /// pairing transform as automatic turn-boundary repair.
    /// `dry_run` only reports.
    pub(super) async fn repair_history(
        &mut self,
        dry_run: bool,
    ) -> Result<crate::compaction_utils::HistoryRepairReport, crate::commands::TimelineWriteError>
    {
        let mut candidate = self.state.timeline.clone();
        let (report, events) = candidate.repair_surface_history()?;
        if dry_run {
            return Ok(report);
        }
        if report.changed() {
            tracing::warn!(
                quarantined_tool_exchanges = report.quarantined_tool_exchanges,
                duplicates_removed = report.duplicates_removed,
                stripped_tool_result_ids = ?report.stripped_tool_result_ids,
                synthetic_results_inserted = report.synthetic_results_inserted,
                "History repair modified the conversation"
            );
            let surface_tokens_before =
                super::state::estimate_conversation_tokens(self.state.timeline.surface());
            debug_assert_eq!(events.len(), 1, "explicit repair is one Surface event");
            for event in events {
                self.commit_timeline_event(event).await?;
            }
            self.finish_surface_replacement(surface_tokens_before);
        }
        Ok(report)
    }

    /// Push any conversation item (user, assistant, or tool result) and persist it.
    pub(super) async fn push_message(&mut self, item: ConversationItem) {
        if !self.append_message_fact(item).await {
            return;
        }
    }

    pub(super) async fn push_tool_result_durably(
        &mut self,
        item: ConversationItem,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let tokens = super::state::estimate_item_tokens(&item);
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Messages(crate::MessageEvent {
                cause: MessageCause::ToolResult,
                items: vec![item],
                surface: crate::SurfaceOp::Append,
                response_admission: None,
            }))?;
        self.commit_timeline_event(event).await?;
        self.apply_projected_token_delta(0, tokens);
        Ok(())
    }

    pub(super) async fn push_tool_result_conditionally(
        &mut self,
        item: ConversationItem,
        rejection_item: ConversationItem,
        expected_surface_revision: u64,
        max_context_tokens: u64,
        max_result_tokens: u64,
    ) -> Result<crate::commands::ConditionalToolResultOutcome, crate::commands::TimelineWriteError>
    {
        use crate::commands::ConditionalToolResultOutcome;

        let actual_revision = self.state.timeline.surface_revision();
        let current_tokens = self.state.projected_tokens;
        let item_tokens = super::state::estimate_item_tokens(&item);
        let outcome = if actual_revision != expected_surface_revision {
            ConditionalToolResultOutcome::RejectedSurfaceChanged
        } else if item_tokens > max_result_tokens
            || current_tokens.saturating_add(item_tokens) > max_context_tokens
        {
            ConditionalToolResultOutcome::RejectedHeadroom
        } else {
            ConditionalToolResultOutcome::Accepted
        };
        let selected = if outcome == ConditionalToolResultOutcome::Accepted {
            item
        } else {
            rejection_item
        };
        let selected_tokens = super::state::estimate_item_tokens(&selected);
        let mut candidate = self.state.timeline.clone();
        let event = candidate.append(selected, MessageCause::ToolResult)?;
        self.commit_timeline_event(event).await?;
        self.apply_projected_token_delta(0, selected_tokens);
        Ok(outcome)
    }

    /// Push a user message, ensuring conversation integrity first.
    ///
    /// When the user cancels a turn while the model was executing parallel
    /// tool calls, the conversation may have dangling tool call IDs. This
    /// method repairs them before appending the new message so the on-disk
    /// and in-memory state stay consistent.
    ///
    pub(super) async fn push_user_message(&mut self, item: ConversationItem) {
        self.push_user_message_with_repair_reason(item, DanglingToolCallReason::UserCancelled)
            .await;
    }

    /// Commit the user-message fact before exposing it through Surface.
    pub(super) async fn push_user_message_durably(
        &mut self,
        item: ConversationItem,
    ) -> Result<crate::TimelineEvent, crate::commands::TimelineWriteError> {
        self.ensure_conversation_integrity_durably(DanglingToolCallReason::UserCancelled)
            .await?;
        let item_tokens = super::state::estimate_item_tokens(&item);
        let cause = message_cause(&item)?;
        let mut candidate = self.state.timeline.clone();
        let event = candidate.append(item, cause)?;
        let event = self.commit_timeline_event(event).await?;
        self.apply_projected_token_delta(0, item_tokens);
        Ok(event)
    }

    /// Like [`Self::push_user_message`] but takes an explicit repair reason.
    pub(super) async fn push_user_message_with_repair_reason(
        &mut self,
        item: ConversationItem,
        reason: DanglingToolCallReason,
    ) {
        self.ensure_conversation_integrity_with_reason(reason).await;
        if !self.append_message_fact(item).await {
            return;
        }
    }

    /// Apply a [`compaction::PrunePlan`] to the stored conversation in one
    /// actor transaction: replace the `content` of each planned `ToolResult`
    /// with head + marker + tail as one durable Timeline replacement. `images`,
    /// `tool_call_id`, and every other structural
    /// field are preserved; conversation length and item identity never
    /// change.
    ///
    /// # Serialization
    ///
    /// Runs inside the actor's command loop, so it cannot interleave with
    /// `PushToolResult` / `PushAssistantResponse` mid-turn — unlike a
    /// `GetConversation` + `ReplaceConversation` read-modify-write, which can
    /// lose concurrently appended items.
    ///
    /// # UI, logging, and turn capture
    ///
    /// No `ChatStateEvent` is published: the pager renders streamed wire
    /// events, so pruning stored state must not disturb what the user already
    /// saw. Rewind expands the shadowed Timeline node and restores the
    /// unpruned content (correct time-travel semantics).
    /// `snapshot_turn_slice` / `rebase_turn_capture_offset` need
    /// no action: pruning swaps only `content` in place, so the captured turn
    /// tail and its `turn_start_offset` slicing stay valid.
    ///
    /// # Defensive behavior
    ///
    /// Plan indices outside the conversation or onto a non-`ToolResult` item
    /// are skipped with a diagnostic (never panic, never touch the item). A
    /// `budget_tokens == 0` plan item is clamped to `1` so content is trimmed
    /// but never silently emptied. `item.tokens_before` is advisory — the
    /// actual current content decides whether pruning applies.
    ///
    /// # Idempotency and usage accounting
    ///
    /// Items whose content already contains [`PRUNE_MARKER`] or already fits
    /// the budget are skipped, so replaying the same plan never re-prunes.
    /// The before/after Surface estimate is applied as a signed delta to the
    /// latest provider anchor and clamped at zero. This is the same projection
    /// transaction used by compaction, rewind, and repair.
    pub(super) async fn prune_tool_results(
        &mut self,
        plan: compaction::PrunePlan,
    ) -> Result<crate::commands::PruneReport, crate::commands::PruneError> {
        use crate::commands::{PruneError, PruneReport};

        if self.state.timeline.surface().is_empty() {
            return Err(PruneError::EmptyConversation);
        }

        let tokens_before = self.state.projected_tokens;
        let surface_tokens_before =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        let mut pruned_count = 0usize;
        let mut conversation = self.state.timeline.surface().to_vec();

        for item in &plan.items {
            let conversation_len = conversation.len();
            let Some(slot) = conversation.get_mut(item.index) else {
                tracing::warn!(
                    index = item.index,
                    conversation_len,
                    "PruneToolResults: plan index out of bounds; skipped"
                );
                continue;
            };
            // The actual current content wins over the plan's `tokens_before`
            // (the conversation may have moved since planning); never panic.
            let actual_before = super::state::estimate_item_tokens(slot);
            if actual_before != u64::from(item.tokens_before) {
                tracing::debug!(
                    index = item.index,
                    plan_tokens_before = item.tokens_before,
                    actual_tokens_before = actual_before,
                    "PruneToolResults: plan token count is stale; using actual content"
                );
            }
            let ConversationItem::ToolResult(tr) = slot else {
                tracing::warn!(
                    index = item.index,
                    "PruneToolResults: plan index does not hold a tool result; skipped"
                );
                continue;
            };
            // Idempotency: never re-prune content that already carries the
            // marker. Content that already fits the budget is reported by
            // `prune_tool_result_content` as `None` and skipped as well.
            if tr.content.contains(PRUNE_MARKER) {
                continue;
            }
            let budget_tokens = item.budget_tokens.max(1);
            if let Some(pruned) =
                compaction::prune_tool_result_content(&tr.content, budget_tokens, PRUNE_MARKER)
            {
                tr.content = std::sync::Arc::<str>::from(pruned);
                pruned_count += 1;
            }
        }

        let mut tokens_after = tokens_before;
        if pruned_count > 0 {
            let mut candidate = self.state.timeline.clone();
            let event = candidate
                .replace_all(conversation, MessageCause::ToolResultPrune)
                .map_err(crate::commands::TimelineWriteError::Invalid)?;
            self.commit_timeline_event(event).await?;
            tokens_after = self.apply_surface_token_delta(surface_tokens_before);
            tracing::info!(
                pruned_count,
                tokens_before,
                tokens_after,
                conversation_len = self.state.timeline.surface_len(),
                "PruneToolResults: pruned oversized tool results"
            );
        }

        Ok(PruneReport {
            pruned_count,
            tokens_before,
            tokens_after,
        })
    }

    /// Replace projected context pressure with the provider's canonical total
    /// for the just-completed response. Billing remains in `UsageLedger`.
    pub(super) fn record_provider_context_anchor(&mut self, provider_total_tokens: u64) {
        let surface_tokens =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        let heuristic_minimum = self.state.projected_request_input_tokens.saturating_add(
            surface_tokens.saturating_sub(self.state.projected_request_surface_tokens),
        );
        if provider_total_tokens < heuristic_minimum {
            tracing::warn!(
                provider_total_tokens,
                heuristic_minimum,
                "ignored provider context anchor below its final request estimate"
            );
            return;
        }
        self.state.projected_tokens = provider_total_tokens;
        self.send_event(ChatStateEvent::ContextPressureUpdated {
            projected_tokens: provider_total_tokens,
        });
    }

    /// Stash the per-turn `TokenUsage` from the most recent model response.
    /// No event is emitted — this slot is read on demand at `PromptResponse`
    /// construction time, not pushed to subscribers.
    pub(super) fn record_last_turn_usage(&mut self, usage: sampling_types::TokenUsage) {
        self.state.last_turn_usage = Some(usage);
    }

    pub(super) fn record_model_call_usage(
        &mut self,
        model_id: Option<String>,
        usage: &sampling_types::TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let model_key = match model_id.as_deref() {
            Some(id) if !id.is_empty() => id,
            _ => self.state.sampling_config.model.as_str(),
        }
        .to_owned();
        self.state
            .prompt_usage
            .get_or_insert_default()
            .record_main_loop_call(&model_key, usage, api_duration_ms, cost_usd_ticks);
        self.state.session_usage.record_main_loop_call(
            &model_key,
            usage,
            api_duration_ms,
            cost_usd_ticks,
        );
        self.publish_session_usage();
    }

    /// Durably record and then apply one real model-attempt usage report.
    /// Timeline is the commit point: the in-memory dedup index and ordinary
    /// ledgers are changed only after the observation receives persistence ACK.
    pub(super) async fn settle_model_attempt_usage(
        &mut self,
        attempt_key: String,
        captured_prompt_index: usize,
        model_id: String,
        usage: Option<TokenUsage>,
        cost_usd_ticks: Option<i64>,
        api_duration_ms: Option<u64>,
    ) -> Result<bool, crate::commands::TimelineWriteError> {
        let settlement = AttemptUsageSettlement {
            attempt_key,
            model_id,
            captured_prompt_index,
            usage,
            cost_usd_ticks,
            api_duration_ms,
        };
        if settlement.check_duplicate(
            self.state
                .settled_model_attempts
                .get(&settlement.attempt_key),
        )? {
            return Ok(false);
        }
        let data = serde_json::to_value(&settlement).expect("attempt usage is serializable");
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: "sampling_usage".into(),
                    name: "attempt_settled".into(),
                    turn: None,
                    step: None,
                    data: Some(data),
                },
            ))?;
        self.commit_timeline_event(event).await?;

        let current_prompt =
            self.state.timeline.current_prompt_index() == Some(settlement.captured_prompt_index);
        if let Some(usage) = settlement.usage.as_ref() {
            let model_key = if settlement.model_id.is_empty() {
                self.state.sampling_config.model.clone()
            } else {
                settlement.model_id.clone()
            };
            self.state.session_usage.record_main_loop_call(
                &model_key,
                usage,
                settlement.api_duration_ms,
                settlement.cost_usd_ticks,
            );
            if current_prompt {
                self.state
                    .prompt_usage
                    .get_or_insert_default()
                    .record_main_loop_call(
                        &model_key,
                        usage,
                        settlement.api_duration_ms,
                        settlement.cost_usd_ticks,
                    );
                self.state.last_turn_usage = Some(usage.clone());
            }
        } else {
            self.state.session_usage.mark_incomplete();
            if current_prompt {
                self.state
                    .prompt_usage
                    .get_or_insert_default()
                    .mark_incomplete();
            }
        }
        self.state
            .settled_model_attempts
            .insert(settlement.attempt_key.clone(), settlement);
        self.publish_session_usage();
        Ok(true)
    }

    pub(super) async fn begin_auxiliary_attempt_usage(
        &mut self,
        sideband_id: String,
        attempt_no: u32,
        model_id: String,
        captured_prompt_index: Option<usize>,
    ) -> Result<bool, crate::commands::TimelineWriteError> {
        let start = AuxiliaryAttemptStart {
            key: AuxiliaryAttemptKey {
                sideband_id,
                attempt_no,
            },
            model_id,
            captured_prompt_index,
        };
        start
            .validate()
            .map_err(crate::commands::TimelineWriteError::InvalidAuxiliaryUsage)?;
        match self.state.auxiliary_attempts.get(&start.key) {
            Some((existing, _)) if existing == &start => return Ok(false),
            Some(_) => return Err(crate::commands::TimelineWriteError::AttemptUsageConflict),
            None => {}
        }
        if !self.state.timeline.events().iter().any(|event| matches!(
            &event.kind,
            crate::TimelineEventKind::Sideband(spawn) if spawn.sideband_id == start.key.sideband_id
        )) {
            return Err(crate::commands::TimelineWriteError::InvalidAuxiliaryUsage("Sideband has no durable spawn".into()));
        }
        let segment_index = self.state.session_usage.current_segment_index();
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: "sampling_usage".into(),
                    name: "aux_attempt_started".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::to_value(&start).expect("auxiliary start serializable")),
                },
            ))?;
        self.commit_timeline_event(event).await?;
        self.state
            .auxiliary_attempts
            .insert(start.key.clone(), (start, segment_index));
        Ok(true)
    }

    pub(super) async fn settle_auxiliary_attempt_usage(
        &mut self,
        sideband_id: String,
        attempt_no: u32,
        usage: Option<TokenUsage>,
        cost_usd_ticks: Option<i64>,
        api_duration_ms: Option<u64>,
    ) -> Result<bool, crate::commands::TimelineWriteError> {
        let settlement = AuxiliaryAttemptSettlement {
            key: AuxiliaryAttemptKey {
                sideband_id,
                attempt_no,
            },
            usage,
            cost_usd_ticks,
            api_duration_ms,
        };
        settlement
            .validate()
            .map_err(crate::commands::TimelineWriteError::InvalidAuxiliaryUsage)?;
        if let Some(existing) = self.state.settled_auxiliary_attempts.get(&settlement.key) {
            if existing.matches(&settlement) {
                return Ok(false);
            }
            return Err(crate::commands::TimelineWriteError::AttemptUsageConflict);
        }
        let Some((start, segment_index)) =
            self.state.auxiliary_attempts.get(&settlement.key).cloned()
        else {
            return Err(crate::commands::TimelineWriteError::InvalidAuxiliaryUsage(
                "attempt has no durable start".into(),
            ));
        };
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: "sampling_usage".into(),
                    name: "aux_attempt_settled".into(),
                    turn: None,
                    step: None,
                    data: Some(
                        serde_json::to_value(&settlement)
                            .expect("auxiliary settlement serializable"),
                    ),
                },
            ))?;
        self.commit_timeline_event(event).await?;
        let current_prompt = start.captured_prompt_index.is_some()
            && self.state.timeline.current_prompt_index() == start.captured_prompt_index;
        if let Some(usage) = settlement.usage.as_ref() {
            self.state.session_usage.record_auxiliary_call_at(
                segment_index,
                &start.model_id,
                usage,
                settlement.api_duration_ms,
                settlement.cost_usd_ticks,
            );
            if current_prompt {
                self.state
                    .prompt_usage
                    .get_or_insert_default()
                    .record_auxiliary_call(
                        &start.model_id,
                        usage,
                        settlement.api_duration_ms,
                        settlement.cost_usd_ticks,
                    );
            }
        } else {
            self.state
                .session_usage
                .mark_segment_incomplete(segment_index);
            if current_prompt {
                self.state
                    .prompt_usage
                    .get_or_insert_default()
                    .mark_incomplete();
            }
        }
        self.state
            .settled_auxiliary_attempts
            .insert(settlement.key.clone(), settlement);
        self.publish_session_usage();
        Ok(true)
    }

    pub(super) async fn record_subagent_usage(
        &mut self,
        subagent_id: &str,
        by_model: &[(String, crate::usage::UsageTotals)],
        attribute_to_prompt: bool,
        incomplete: bool,
    ) -> Result<bool, crate::commands::TimelineWriteError> {
        if by_model.is_empty() && !incomplete {
            return Ok(false);
        }
        let settlement = SubagentUsageSettlement {
            subagent_id: subagent_id.to_owned(),
            by_model: by_model.to_vec(),
            incomplete,
        };
        if settlement.check_duplicate(self.state.settled_subagent_usage.get(subagent_id))? {
            return Ok(false);
        }

        let data = serde_json::to_value(&settlement).expect("subagent usage is serializable");
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: "session_usage".into(),
                    name: "subagent_settled".into(),
                    turn: None,
                    step: None,
                    data: Some(data),
                },
            ))?;
        self.commit_timeline_event(event).await?;

        if attribute_to_prompt {
            self.state
                .prompt_usage
                .get_or_insert_default()
                .record_subagent(subagent_id, by_model, incomplete);
        }
        // The session ledger always folds, even when the usage is not
        // attributable to the open prompt (its pin may belong to an earlier
        // prompt). Reporting that gap is the coordinator's sticky flag's job —
        // never mark a different live prompt's ledger.
        self.state
            .session_usage
            .record_subagent(subagent_id, by_model, incomplete);
        self.state
            .settled_subagent_usage
            .insert(subagent_id.to_owned(), settlement);
        self.publish_session_usage();
        Ok(true)
    }

    pub(super) async fn mark_usage_incomplete(
        &mut self,
        prompt: bool,
        session: bool,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        if session && !self.state.session_usage.incomplete {
            let event = self
                .state
                .timeline
                .prepare(crate::TimelineEventKind::Observation(
                    crate::ObservationEvent {
                        scope: "session_usage".into(),
                        name: "incomplete".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))?;
            self.commit_timeline_event(event).await?;
        }
        if prompt {
            self.state
                .prompt_usage
                .get_or_insert_default()
                .mark_incomplete();
        }
        if session && !self.state.session_usage.incomplete {
            self.state.session_usage.mark_incomplete();
            self.publish_session_usage();
        }
        Ok(())
    }

    pub(super) async fn begin_usage_resume_segment(
        &mut self,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let event = self
            .state
            .timeline
            .prepare(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: "session_usage".into(),
                    name: "resume_started".into(),
                    turn: None,
                    step: None,
                    data: None,
                },
            ))?;
        let committed = self.commit_timeline_event(event).await?;
        self.state
            .session_usage
            .begin_resume_segment(committed.seq, committed.at_ms);
        self.publish_session_usage();
        Ok(())
    }

    fn publish_session_usage(&self) {
        self.send_event(ChatStateEvent::SessionUsageUpdated {
            usage: self.state.session_usage.clone(),
        });
    }

    /// Atomically select an earlier prompt boundary from Timeline.
    ///
    /// The replacement event is prepared and committed before the actor changes
    /// any projection or prompt bookkeeping. This is the only rewind mutation;
    /// callers cannot install a separately computed Chat snapshot.
    pub(super) async fn rewind_durably(
        &mut self,
        target_prompt_index: usize,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let current_prompt_index = self.state.timeline.next_prompt_index();
        if target_prompt_index >= current_prompt_index {
            return Err(crate::commands::TimelineWriteError::InvalidRewindTarget {
                target: target_prompt_index,
                current: current_prompt_index,
            });
        }
        let items = self.state.timeline.rewind_surface(target_prompt_index)?;
        let mut candidate = self.state.timeline.clone();
        let event = candidate.replace_all(items, MessageCause::Rewind)?;
        let surface_tokens_before =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.commit_timeline_event(event).await?;
        self.state.turn_capture = None;
        self.state.prompt_usage = None;
        self.finish_surface_replacement(surface_tokens_before);
        Ok(())
    }

    /// Commit a non-rewind Surface transformation before exposing it. This is
    /// intentionally actor-private; public branch selection goes through
    /// [`Self::rewind_durably`] so callers cannot split rewind bookkeeping from
    /// its causal event.
    pub(super) async fn replace_conversation_durably(
        &mut self,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let unchanged = serde_json::to_value(self.state.timeline.surface())
            .expect("conversation surface must serialize")
            == serde_json::to_value(&items).expect("replacement surface must serialize");
        if unchanged {
            return Ok(());
        }
        let mut candidate = self.state.timeline.clone();
        let event = candidate.replace_all(items, cause)?;
        let surface_tokens_before =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.commit_timeline_event(event).await?;
        self.finish_surface_replacement(surface_tokens_before);
        Ok(())
    }

    /// Commit the one range declared by the active compaction transaction.
    pub(super) async fn replace_compaction_range_durably(
        &mut self,
        target: crate::SurfaceRange,
        items: Vec<ConversationItem>,
    ) -> Result<(), crate::commands::TimelineWriteError> {
        let mut candidate = self.state.timeline.clone();
        let event = candidate.replace_compaction_range(target, items)?;
        let surface_tokens_before =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.commit_timeline_event(event).await?;
        if let Some(cap) = &mut self.state.turn_capture {
            cap.compaction_occurred = true;
        }
        self.finish_surface_replacement(surface_tokens_before);
        Ok(())
    }

    /// Install one complete Surface through the buffered Timeline append path.
    /// This is reserved for buffered turn writes; acknowledged boundaries use
    /// [`Self::replace_conversation_durably`].
    async fn install_conversation_buffered(
        &mut self,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) {
        let surface_changed = serde_json::to_value(self.state.timeline.surface())
            .expect("conversation surface must serialize")
            != serde_json::to_value(&items).expect("replacement surface must serialize");
        if !surface_changed {
            return;
        }
        let surface_tokens_before =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        let mut candidate = self.state.timeline.clone();
        let event = candidate
            .replace_all(items, cause)
            .expect("a current surface must accept a complete replacement");
        if !self.commit_buffered_timeline_event(event).await {
            return;
        }
        self.finish_surface_replacement(surface_tokens_before);
    }

    /// Apply the signed static-estimate delta of a committed Surface mutation
    /// to the latest provider anchor. Provider overhead is preserved exactly;
    /// no mutation type gets a separate ratio or reset policy.
    pub(super) fn apply_surface_token_delta(&mut self, surface_tokens_before: u64) -> u64 {
        let surface_tokens_after =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.apply_projected_token_delta(surface_tokens_before, surface_tokens_after)
    }

    /// Apply one signed estimate delta to the current provider anchor.
    pub(super) fn apply_projected_token_delta(
        &mut self,
        tokens_before: u64,
        tokens_after: u64,
    ) -> u64 {
        self.state.projected_tokens = if tokens_after >= tokens_before {
            self.state
                .projected_tokens
                .saturating_add(tokens_after - tokens_before)
        } else {
            self.state
                .projected_tokens
                .saturating_sub(tokens_before - tokens_after)
        };
        self.state.projected_tokens
    }

    /// Replace the prior request-envelope adjustment with the final estimate
    /// for this request. Surface mutations remain one signed stream; this only
    /// accounts for provider-visible projections outside canonical Surface
    /// (tool schemas, Goal shadows, ImageShadows, and native output schemas).
    pub(super) fn apply_request_projection(&mut self, request_input_tokens: u64) {
        let projected_before = self.state.projected_tokens;
        self.apply_projected_token_delta(
            self.state.projected_request_input_tokens,
            self.state.projected_request_surface_tokens,
        );
        let surface_tokens =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.apply_projected_token_delta(surface_tokens, request_input_tokens);
        self.state.projected_request_surface_tokens = surface_tokens;
        self.state.projected_request_input_tokens = request_input_tokens;
        if self.state.projected_tokens != projected_before {
            self.send_event(ChatStateEvent::ContextPressureUpdated {
                projected_tokens: self.state.projected_tokens,
            });
        }
    }

    fn finish_surface_replacement(&mut self, surface_tokens_before: u64) {
        self.state.continuation.reset(
            self.state.sampling_config.api_backend.clone(),
            self.state.timeline.surface_len(),
            "surface_replaced",
        );
        let projected_tokens = self.apply_surface_token_delta(surface_tokens_before);
        self.send_event(ChatStateEvent::ConversationReset {
            new_len: self.state.timeline.surface_len(),
        });
        self.send_event(ChatStateEvent::ContextPressureUpdated { projected_tokens });
    }

    /// Commit typed per-group image projections as one acknowledged Surface
    /// mutation. Description/OCR shadows retain the image and add reusable
    /// text; unsupported-model shadows remove the image from the Surface.
    pub(super) async fn record_image_projection(
        &mut self,
        projection: crate::ImageProjectionEvent,
    ) -> Result<crate::commands::ImageProjectionReport, crate::commands::TimelineWriteError> {
        let actual = self.state.timeline.surface_revision();
        if projection.source_revision != actual {
            return Err(crate::commands::TimelineWriteError::SurfaceChanged {
                expected: projection.source_revision,
                actual,
            });
        }
        let surface_tokens_before =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        let surface_before = serde_json::to_value(self.state.timeline.surface())
            .expect("conversation surface must serialize");
        let mut candidate = self.state.timeline.clone();
        let event = candidate.record(crate::TimelineEventKind::ImageProjection(
            projection.clone(),
        ))?;
        self.commit_timeline_event(event).await?;
        if surface_before
            != serde_json::to_value(self.state.timeline.surface())
                .expect("conversation surface must serialize")
        {
            self.finish_surface_replacement(surface_tokens_before);
        }
        let mut report = crate::commands::ImageProjectionReport::default();
        for shadow in projection.shadows {
            match shadow.provenance {
                crate::ImageShadowSource::UnsupportedModel => {
                    report.removed_images += shadow.image_count;
                }
                crate::ImageShadowSource::Description { .. }
                | crate::ImageShadowSource::LocalOcr { .. } => {
                    report.described_images += shadow.image_count;
                }
            }
        }
        Ok(report)
    }
}
