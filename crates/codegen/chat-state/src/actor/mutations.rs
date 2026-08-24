//! Mutation handlers for the ChatStateActor.

use sampling_types::{
    ConversationItem, DanglingToolCallReason, dedup_duplicate_tool_results,
    repair_dangling_tool_calls,
};

use super::ChatStateActor;
use crate::MessageCause;
use crate::events::ChatStateEvent;

/// Static string label for tracing on `ConversationItem` (avoids pulling
/// the `Role` enum into the format string).
fn item_kind_str(item: &ConversationItem) -> &'static str {
    match item {
        ConversationItem::System(_) => "system",
        ConversationItem::User(_) => "user",
        ConversationItem::Assistant(_) => "assistant",
        ConversationItem::ToolResult(_) => "tool_result",
        ConversationItem::BackendToolCall(_) => "backend_tool_call",
        ConversationItem::Reasoning(_) => "reasoning",
    }
}

fn message_cause(item: &ConversationItem) -> Result<MessageCause, crate::TimelineError> {
    match item {
        ConversationItem::System(_) => Err(crate::TimelineError::InvalidMessageShape),
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
        ConversationItem::User(_) => Ok(MessageCause::User),
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
        let mut candidate = self.state.timeline.clone();
        let event = candidate
            .append(item.clone(), cause)
            .expect("an assembled conversation item must append to the timeline");
        self.commit_buffered_timeline_event(event).await
    }

    /// Repair any dangling tool calls in the conversation and persist the fix.
    ///
    /// A "dangling" tool call is an assistant message with tool call IDs that
    /// lack matching `ToolResult` entries. This can happen when:
    /// - The user cancels (Ctrl+C) mid-tool-execution in a live session
    /// - The process crashes between pushing the assistant and tool results
    /// - The tokio task is aborted at an `.await` point
    ///
    /// This method repairs the state in-place and persists the fix to disk.
    /// It is idempotent — calling it on a clean conversation is a cheap no-op
    /// (single forward scan, no allocations).
    ///
    /// Only call at write boundaries where the previous turn is definitively
    /// over (`push_user_message()` or a harness-declared halt).
    /// Do NOT call from read handlers — background tasks run concurrently with
    /// tool execution and would misidentify in-flight calls as dangling.
    /// Repair dangling/duplicate tool results through the buffered append path.
    pub(super) async fn ensure_conversation_integrity_with_reason(
        &mut self,
        reason: DanglingToolCallReason,
    ) {
        let mut conversation = self.state.timeline.surface().to_vec();
        let deduped = dedup_duplicate_tool_results(&mut conversation);
        if deduped > 0 {
            tracing::info!(
                deduped_count = deduped,
                "Removed duplicate tool results in conversation"
            );
        }
        let repaired = repair_dangling_tool_calls(&mut conversation, reason);
        if repaired > 0 || deduped > 0 {
            tracing::info!(
                repaired_count = repaired,
                "Repaired dangling tool calls in conversation"
            );
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
        let deduped = dedup_duplicate_tool_results(&mut conversation);
        let repaired = repair_dangling_tool_calls(&mut conversation, reason);
        if repaired == 0 && deduped == 0 {
            return Ok(());
        }
        tracing::info!(
            deduped_count = deduped,
            repaired_count = repaired,
            "Repaired conversation before durable boundary"
        );
        self.replace_conversation_durably(conversation, MessageCause::IntegrityRepair)
            .await
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
    /// the Timeline replacement transaction. Unlike
    /// the turn-boundary integrity repair, this also removes orphaned
    /// `ToolResult`s — the shape that bricks a session with provider 400s.
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
                duplicates_removed = report.duplicates_removed,
                stripped_tool_result_ids = ?report.stripped_tool_result_ids,
                synthetic_results_inserted = report.synthetic_results_inserted,
                "History repair modified the conversation"
            );
            debug_assert_eq!(events.len(), 1, "explicit repair is one Surface event");
            for event in events {
                self.commit_timeline_event(event).await?;
            }
            let pre_replace_total = self.state.total_tokens;
            self.refresh_surface_projection(false, pre_replace_total);
        }
        Ok(report)
    }

    /// Push any conversation item (user, assistant, or tool result) and persist it.
    pub(super) async fn push_message(&mut self, item: ConversationItem) {
        let count_in_delta = !matches!(item, ConversationItem::Assistant(_));
        if !self.append_message_fact(item.clone()).await {
            return;
        }
        if count_in_delta {
            let estimated_tokens = super::state::estimate_item_tokens(&item);
            self.state.estimated_tokens_since_model += estimated_tokens;
            tracing::debug!(
                item_kind = item_kind_str(&item),
                estimated_tokens_delta = estimated_tokens,
                estimated_total = self.state.total_tokens + self.state.estimated_tokens_since_model,
                model_reported_total = self.state.total_tokens,
                "ChatState: push_message updated estimated_tokens_since_model"
            );
        }
    }

    pub(super) async fn push_tool_result_conditionally(
        &mut self,
        item: ConversationItem,
        rejection_item: ConversationItem,
        expected_surface_revision: u64,
        max_estimated_total_tokens: u64,
        max_result_tokens: u64,
    ) -> Result<crate::commands::ConditionalToolResultOutcome, crate::commands::TimelineWriteError>
    {
        use crate::commands::ConditionalToolResultOutcome;

        let actual_revision = self.state.timeline.surface_revision();
        let current_tokens = self.state.total_tokens + self.state.estimated_tokens_since_model;
        let item_tokens = super::state::estimate_item_tokens(&item);
        let outcome = if actual_revision != expected_surface_revision {
            ConditionalToolResultOutcome::RejectedSurfaceChanged
        } else if item_tokens > max_result_tokens
            || current_tokens.saturating_add(item_tokens) > max_estimated_total_tokens
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
        self.state.estimated_tokens_since_model += selected_tokens;
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
    ) -> Result<(), crate::commands::TimelineWriteError> {
        self.ensure_conversation_integrity_durably(DanglingToolCallReason::UserCancelled)
            .await?;
        let cause = message_cause(&item)?;
        let mut candidate = self.state.timeline.clone();
        let event = candidate.append(item.clone(), cause)?;
        self.commit_timeline_event(event).await?;

        let estimated_tokens = super::state::estimate_item_tokens(&item);
        self.state.estimated_tokens_since_model += estimated_tokens;
        tracing::debug!(
            item_kind = item_kind_str(&item),
            estimated_tokens_delta = estimated_tokens,
            estimated_total = self.state.total_tokens + self.state.estimated_tokens_since_model,
            model_reported_total = self.state.total_tokens,
            "ChatState: durable user message updated estimated_tokens_since_model"
        );
        Ok(())
    }

    /// Like [`Self::push_user_message`] but takes an explicit repair reason.
    pub(super) async fn push_user_message_with_repair_reason(
        &mut self,
        item: ConversationItem,
        reason: DanglingToolCallReason,
    ) {
        self.ensure_conversation_integrity_with_reason(reason).await;
        if !self.append_message_fact(item.clone()).await {
            return;
        }
        let estimated_tokens = super::state::estimate_item_tokens(&item);
        self.state.estimated_tokens_since_model += estimated_tokens;
        tracing::debug!(
            item_kind = item_kind_str(&item),
            estimated_tokens_delta = estimated_tokens,
            estimated_total = self.state.total_tokens + self.state.estimated_tokens_since_model,
            model_reported_total = self.state.total_tokens,
            "ChatState: push_user_message updated estimated_tokens_since_model"
        );
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
    /// latest provider anchor and clamped at zero. This preserves provider-side
    /// accounting instead of replacing it with a fresh local estimate.
    /// `estimated_tokens_since_model` and `estimate_at_last_response` are
    /// left untouched: the compaction overhead ratio must keep measuring
    /// against the last-response snapshot, and the post-response delta
    /// self-heals at the next `record_token_usage`.
    pub(super) async fn prune_tool_results(
        &mut self,
        plan: compaction::PrunePlan,
    ) -> Result<crate::commands::PruneReport, crate::commands::PruneError> {
        use crate::commands::{PruneError, PruneReport};

        if self.state.timeline.surface().is_empty() {
            return Err(PruneError::EmptyConversation);
        }

        let tokens_before = self.state.total_tokens;
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
            let reestimated = super::state::estimate_conversation_tokens(&conversation);
            let mut candidate = self.state.timeline.clone();
            let event = candidate
                .replace_all(conversation, MessageCause::ToolResultPrune)
                .map_err(crate::commands::TimelineWriteError::Invalid)?;
            self.commit_timeline_event(event).await?;
            // Project the signed Surface delta from the latest provider
            // anchor. This preserves provider-side overhead instead of
            // replacing it with a fresh local estimate. The independent
            // post-response addition estimate stays untouched below.
            let removed_tokens = surface_tokens_before.saturating_sub(reestimated);
            tokens_after = tokens_before.saturating_sub(removed_tokens);
            self.state.total_tokens = tokens_after;
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

    /// Record accumulated token usage and emit an event.
    pub(super) fn record_token_usage(&mut self, total_tokens: u64) {
        self.state.estimated_tokens_since_model = 0;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.state.total_tokens = total_tokens;
        self.send_event(ChatStateEvent::TokensUpdated { total_tokens });
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
    }

    pub(super) fn record_subagent_usage(
        &mut self,
        by_model: &[(String, crate::usage::UsageTotals)],
        attribute_to_prompt: bool,
        incomplete: bool,
    ) {
        if by_model.is_empty() && !incomplete {
            return;
        }
        if attribute_to_prompt {
            self.state
                .prompt_usage
                .get_or_insert_default()
                .record_subagent(by_model, incomplete);
        }
        // The session ledger always folds, even when the usage is not
        // attributable to the open prompt (its pin may belong to an earlier
        // prompt). Reporting that gap is the coordinator's sticky flag's job —
        // never mark a different live prompt's ledger.
        self.state
            .session_usage
            .record_subagent(by_model, incomplete);
    }

    pub(super) fn mark_usage_incomplete(&mut self, prompt: bool, session: bool) {
        if prompt {
            self.state
                .prompt_usage
                .get_or_insert_default()
                .mark_incomplete();
        }
        if session {
            self.state.session_usage.mark_incomplete();
        }
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
        let pre_replace_total = self.state.total_tokens;
        self.commit_timeline_event(event).await?;
        self.state.turn_capture = None;
        self.state.prompt_usage = None;
        self.refresh_surface_projection(false, pre_replace_total);
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
        let pre_replace_total = self.state.total_tokens;
        self.commit_timeline_event(event).await?;
        self.refresh_surface_projection(false, pre_replace_total);
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
        let pre_replace_total = self.state.total_tokens;
        self.commit_timeline_event(event).await?;
        if let Some(cap) = &mut self.state.turn_capture {
            cap.compaction_occurred = true;
        }
        self.refresh_surface_projection(true, pre_replace_total);
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
        let pre_replace_total = self.state.total_tokens;
        let surface_changed = serde_json::to_value(self.state.timeline.surface())
            .expect("conversation surface must serialize")
            != serde_json::to_value(&items).expect("replacement surface must serialize");
        if surface_changed {
            let mut candidate = self.state.timeline.clone();
            let event = candidate
                .replace_all(items, cause)
                .expect("a current surface must accept a complete replacement");
            if !self.commit_buffered_timeline_event(event).await {
                return;
            }
        }
        self.refresh_surface_projection(false, pre_replace_total);
    }

    fn refresh_surface_projection(&mut self, is_compaction: bool, pre_replace_total: u64) {
        let base_estimate =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        let mut estimated_tokens =
            if is_compaction && pre_replace_total > 0 && self.state.estimate_at_last_response > 0 {
                let ratio = pre_replace_total as f64 / self.state.estimate_at_last_response as f64;
                (base_estimate as f64 * ratio).round() as u64
            } else {
                base_estimate
            };
        // Compaction must never appear to increase usage.
        if is_compaction && pre_replace_total > 0 {
            estimated_tokens = estimated_tokens.min(pre_replace_total);
        }
        self.state.estimated_tokens_since_model = 0;
        self.state.total_tokens = estimated_tokens;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.send_event(ChatStateEvent::ConversationReset {
            new_len: self.state.timeline.surface_len(),
        });
        self.send_event(ChatStateEvent::TokensUpdated {
            total_tokens: estimated_tokens,
        });
    }

    /// Rewrite all current image groups against an optimistic snapshot in one
    /// actor transaction. A mismatched or newly appended group is removed
    /// rather than retained: once the active runtime is known to reject image
    /// input, the postcondition is a canonical history with no images.
    pub(super) async fn rewrite_images(
        &mut self,
        rewrites: Vec<crate::commands::ImageRewrite>,
        dropped_placeholder: &str,
    ) -> Option<crate::commands::ImageRewriteReport> {
        use std::collections::BTreeMap;

        use sampling_types::conversation::{
            conversation_image_groups, replace_item_images_with_text,
        };

        let mut report = crate::commands::ImageRewriteReport::default();
        let mut expected = rewrites
            .into_iter()
            .map(|rewrite| ((rewrite.item_index, rewrite.fingerprint.clone()), rewrite))
            .collect::<BTreeMap<_, _>>();
        let groups = conversation_image_groups(self.state.timeline.surface());
        if groups.is_empty() {
            report.unmatched_images = expected
                .values()
                .map(|rewrite| rewrite.expected_image_count)
                .sum();
            return Some(report);
        }

        let mut conversation = self.state.timeline.surface().to_vec();
        for group in groups {
            let key = (group.item_index, group.fingerprint.clone());
            let rewrite = expected.remove(&key);
            let replacement = rewrite
                .as_ref()
                .and_then(|rewrite| rewrite.replacement.as_deref())
                .filter(|text| !text.trim().is_empty());
            let text = replacement.unwrap_or(dropped_placeholder);
            let removed = replace_item_images_with_text(&mut conversation[group.item_index], text);
            if replacement.is_some() {
                report.converted_images += removed;
            } else {
                report.dropped_images += removed;
            }
            if rewrite.is_none() {
                report.unmatched_images += removed;
            }
        }
        report.unmatched_images += expected
            .values()
            .map(|rewrite| rewrite.expected_image_count)
            .sum::<usize>();
        debug_assert_eq!(
            conversation.len(),
            self.state.timeline.surface_len(),
            "image rewrite must preserve conversation item identity"
        );
        if let Err(error) = self
            .replace_conversation_durably(conversation, MessageCause::ImageRewrite)
            .await
        {
            tracing::warn!(%error, "image rewrite Timeline event was not committed");
            return None;
        }
        Some(report)
    }

    /// Seed provider-reported accounting without mutating Timeline-derived
    /// conversation or branch coordinates.
    pub(super) fn seed_token_accounting(&mut self, total_tokens: u64) {
        self.state.total_tokens = total_tokens;
        self.state.estimated_tokens_since_model = 0;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(self.state.timeline.surface());
        self.send_event(ChatStateEvent::TokensUpdated { total_tokens });
    }
}
