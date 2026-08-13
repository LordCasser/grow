//! Mutation handlers for the ChatStateActor.

use sampling_types::{
    ContentPart, ConversationItem, DanglingToolCallReason, dedup_duplicate_tool_results,
    repair_dangling_tool_calls,
};

use super::ChatStateActor;
use super::request_builder::HARD_CLEAR_PLACEHOLDER;
use crate::events::ChatStateEvent;
use crate::types::ChatStateSnapshot;

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
    /// over (`ChatState::new()`, `push_user_message()`, `BuildConversationRequest`).
    /// Do NOT call from read handlers — background tasks run concurrently with
    /// tool execution and would misidentify in-flight calls as dangling.
    pub(super) fn ensure_conversation_integrity(&mut self) {
        self.ensure_conversation_integrity_with_reason(DanglingToolCallReason::UserCancelled);
    }

    /// Like [`Self::ensure_conversation_integrity`] but takes an explicit reason.
    pub(super) fn ensure_conversation_integrity_with_reason(
        &mut self,
        reason: DanglingToolCallReason,
    ) {
        // In-place integrity repair can add/remove items ahead of an active capture's
        // boundary, so snapshot + rebase the offset like the replace/restore paths.
        self.snapshot_turn_slice();
        let deduped = dedup_duplicate_tool_results(&mut self.state.conversation);
        if deduped > 0 {
            tracing::info!(
                deduped_count = deduped,
                "Removed duplicate tool results in conversation"
            );
        }
        let repaired = repair_dangling_tool_calls(&mut self.state.conversation, reason);
        if repaired > 0 || deduped > 0 {
            tracing::info!(
                repaired_count = repaired,
                "Repaired dangling tool calls in conversation"
            );
            self.persistence.replace_history(&self.state.conversation);
        }
        self.rebase_turn_capture_offset();
    }

    /// Repair dangling tool calls after a harness-initiated halt.
    pub(super) fn repair_dangling_after_harness_halt(&mut self, class: &'static str) {
        self.ensure_conversation_integrity_with_reason(DanglingToolCallReason::HarnessHalted {
            class,
        });
    }

    /// Out-of-band history repair (`grow/session/repair`): run
    /// [`crate::compaction_utils::repair_history`] and persist changes via
    /// [`Self::replace_conversation`]. Unlike
    /// [`Self::ensure_conversation_integrity`], this also removes orphaned
    /// `ToolResult`s — the shape that bricks a session with provider 400s.
    /// `dry_run` only reports.
    pub(super) fn repair_history(
        &mut self,
        dry_run: bool,
    ) -> crate::compaction_utils::HistoryRepairReport {
        if dry_run {
            let mut copy = self.state.conversation.clone();
            return crate::compaction_utils::repair_history(&mut copy);
        }
        let mut items = std::mem::take(&mut self.state.conversation);
        let report = crate::compaction_utils::repair_history(&mut items);
        if report.changed() {
            tracing::warn!(
                duplicates_removed = report.duplicates_removed,
                stripped_tool_result_ids = ?report.stripped_tool_result_ids,
                synthetic_results_inserted = report.synthetic_results_inserted,
                "History repair modified the conversation"
            );
            // Full replace: persists atomically and re-bases token estimates.
            self.replace_conversation(items, false);
        } else {
            // Nothing changed — put the conversation back untouched.
            self.state.conversation = items;
        }
        report
    }

    /// Make memory match the disk-authoritative switch for one generation.
    pub(super) fn converge_working_directory_switch(
        &mut self,
        generation: u64,
        authoritative: ConversationItem,
    ) {
        let existing = self
            .state
            .conversation
            .iter_mut()
            .find(|item| item.working_directory_switch_generation() == Some(generation));
        if let Some(existing) = existing {
            let old_tokens = super::state::estimate_item_tokens(existing);
            let new_tokens = super::state::estimate_item_tokens(&authoritative);
            self.state.estimated_tokens_since_model = if new_tokens >= old_tokens {
                self.state
                    .estimated_tokens_since_model
                    .saturating_add(new_tokens - old_tokens)
            } else {
                self.state
                    .estimated_tokens_since_model
                    .saturating_sub(old_tokens - new_tokens)
            };
            *existing = authoritative;
        } else {
            self.state.estimated_tokens_since_model +=
                super::state::estimate_item_tokens(&authoritative);
            self.state.conversation.push(authoritative);
        }
    }

    /// Push any conversation item (user, assistant, or tool result) and persist it.
    pub(super) fn push_message(&mut self, item: ConversationItem) {
        let count_in_delta = !matches!(item, ConversationItem::Assistant(_));
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
        self.persistence.persist_message(&item);
        self.state.conversation.push(item);
    }

    /// Push a user message, ensuring conversation integrity first.
    ///
    /// When the user cancels a turn while the model was executing parallel
    /// tool calls, the conversation may have dangling tool call IDs. This
    /// method repairs them before appending the new message so the on-disk
    /// and in-memory state stay consistent.
    ///
    /// Also runs [`prune_retained_conversation`] to eagerly hard-clear very
    /// old tool results from the in-memory state, bounding long-session
    /// retained memory without waiting for the context-window threshold.
    pub(super) fn push_user_message(&mut self, item: ConversationItem) {
        self.push_user_message_with_repair_reason(item, DanglingToolCallReason::UserCancelled);
    }

    /// Like [`Self::push_user_message`] but takes an explicit repair reason.
    pub(super) fn push_user_message_with_repair_reason(
        &mut self,
        item: ConversationItem,
        reason: DanglingToolCallReason,
    ) {
        self.ensure_conversation_integrity_with_reason(reason);
        let estimated_tokens = super::state::estimate_item_tokens(&item);
        self.state.estimated_tokens_since_model += estimated_tokens;
        tracing::debug!(
            item_kind = item_kind_str(&item),
            estimated_tokens_delta = estimated_tokens,
            estimated_total = self.state.total_tokens + self.state.estimated_tokens_since_model,
            model_reported_total = self.state.total_tokens,
            "ChatState: push_user_message updated estimated_tokens_since_model"
        );
        self.persistence.persist_message(&item);
        self.state.conversation.push(item);
        self.prune_retained_conversation();
    }

    /// Eagerly hard-clear tool results from very old turns in the retained
    /// in-memory conversation, freeing the actual string bytes.
    ///
    /// Unlike the API-copy pruning in `build_conversation_request` (which runs
    /// on a *clone* only when context > 50% full), this operates on
    /// `self.state.conversation` directly and runs after every user turn.
    ///
    /// # What this does
    ///
    /// Only **hard-clears** are applied (no soft-trim).  Soft-trimming is a
    /// context-management operation that changes what the model sees;
    /// hard-clearing is a memory-management operation that replaces content
    /// that is so old the model should not need it again.  The threshold is
    /// controlled by `PruningConfig::hard_clear_age_turns`.
    ///
    /// # Retained-memory measurement
    ///
    /// When any clearing occurs, a `tracing::debug!` event reports:
    /// - `hard_cleared` — number of tool results cleared
    /// - `bytes_freed` — approximate bytes recovered (sum of content lengths)
    /// - `conversation_len` — total item count after the pass
    ///
    /// # Synthetic User items and turn-age accuracy
    ///
    /// The shell can inject synthetic `User` items mid-turn (e.g. system
    /// corrective warnings) without calling `increment_prompt_index`.  These
    /// do not represent real user turns.  The backward scan here counts every
    /// `User` item as a turn boundary, so synthetic items would normally cause
    /// old tool results to appear older than they really are.
    ///
    /// This is compensated by raising the effective clearing threshold by the
    /// number of synthetic User items (`total_user_items - prompt_index`).
    /// The result: a tool result is never cleared before `hard_clear_age_turns`
    /// REAL turns have elapsed, even in sessions with many synthetic messages.
    ///
    /// # Replay / rewind correctness
    ///
    /// `updates.jsonl` is **never touched**, so cross-compaction
    /// `replay_to_prompt` is unaffected.  The pruned `chat_history.jsonl`
    /// on disk mirrors the in-memory state — both lose old bulk content but
    /// `updates.jsonl` retains the original data for replay.
    pub(super) fn prune_retained_conversation(&mut self) -> usize {
        if !self.pruning_config.enabled {
            return 0;
        }
        // Fast exit: not enough turns have elapsed for any hard-clear to apply.
        if self.state.prompt_index < self.pruning_config.hard_clear_age_turns {
            return 0;
        }

        // Compute how many synthetic User items exist (system reminders, etc.).
        // Synthetic User items are NOT real user turns — they are injected by the
        // shell mid-turn and do not increment `prompt_index`.  The naive backward
        // scan counts every User item as a turn boundary, so synthetic items make
        // old tool results appear older than they really are and can cause
        // premature hard-clears.
        //
        // Fix: raise the effective clearing threshold by the number of synthetic
        // User items.  This guarantees a tool result is never cleared before
        // `hard_clear_age_turns` REAL turns have elapsed, regardless of how many
        // synthetic messages the session contains.
        let total_user_items = self
            .state
            .conversation
            .iter()
            .filter(|i| matches!(i, ConversationItem::User(_)))
            .count();
        let synthetic_count = total_user_items.saturating_sub(self.state.prompt_index);
        let effective_threshold = self
            .pruning_config
            .hard_clear_age_turns
            .saturating_add(synthetic_count);

        let before_bytes = self.conversation_content_bytes();
        let mut cleared = 0usize;
        let mut turn_from_end: usize = 0;
        let mut seen_first_user = false;

        for i in (0..self.state.conversation.len()).rev() {
            if matches!(&self.state.conversation[i], ConversationItem::User(_)) {
                if seen_first_user {
                    turn_from_end += 1;
                }
                seen_first_user = true;
                continue;
            }

            let ConversationItem::ToolResult(tr) = &mut self.state.conversation[i] else {
                continue;
            };

            if turn_from_end < effective_threshold {
                continue;
            }

            if tr.content.as_ref() != HARD_CLEAR_PLACEHOLDER {
                tr.content = std::sync::Arc::<str>::from(HARD_CLEAR_PLACEHOLDER);
                cleared += 1;
            }
        }

        if cleared > 0 {
            let after_bytes = self.conversation_content_bytes();
            tracing::debug!(
                hard_cleared = cleared,
                bytes_freed = before_bytes.saturating_sub(after_bytes),
                conversation_len = self.state.conversation.len(),
                "ChatState: in-memory tool-result prune"
            );
            self.persistence.replace_history(&self.state.conversation);
        }

        cleared
    }

    /// Apply a [`compaction::PrunePlan`] to the stored conversation in one
    /// actor transaction: replace the `content` of each planned `ToolResult`
    /// with head + marker + tail and persist the whole history through
    /// `replace_history` (the same `chat_history.jsonl` snapshot path as
    /// compaction). `images`, `tool_call_id`, and every other structural
    /// field are preserved; conversation length and item identity never
    /// change.
    ///
    /// # Serialization
    ///
    /// Runs inside the actor's command loop, so it cannot interleave with
    /// `PushToolResult` / `PushAssistantResponse` mid-turn — unlike a
    /// `GetConversation` + `ReplaceConversation` read-modify-write, which can
    /// lose concurrently appended items (the `ReplaceSystemHead` lesson).
    ///
    /// # UI, logging, and turn capture
    ///
    /// No `ChatStateEvent` is published: the pager renders streamed wire
    /// events, so pruning stored state must not disturb what the user already
    /// saw. `updates.jsonl` is never written — `replace_history` only
    /// rewrites the `chat_history.jsonl` snapshot, and rewind replay across a
    /// prune point restores the unpruned content (correct time-travel
    /// semantics). `snapshot_turn_slice` / `rebase_turn_capture_offset` need
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
    /// `total_tokens` is re-estimated with
    /// [`super::state::estimate_conversation_tokens`] and clamped to the
    /// pre-prune value: pruning must never appear to increase usage (the same
    /// `min(pre_replace_total)` guard as `install_persisted_conversation`).
    /// `estimated_tokens_since_model` and `estimate_at_last_response` are
    /// left untouched, matching [`Self::prune_retained_conversation`]: the
    /// compaction overhead ratio must keep measuring against the
    /// last-response snapshot, and the post-response delta self-heals at the
    /// next `record_token_usage`.
    pub(super) fn prune_tool_results(
        &mut self,
        plan: compaction::PrunePlan,
    ) -> Result<crate::commands::PruneReport, crate::commands::PruneError> {
        use crate::commands::{PruneError, PruneReport};

        if self.state.conversation.is_empty() {
            return Err(PruneError::EmptyConversation);
        }

        let tokens_before = self.state.total_tokens;
        let mut pruned_count = 0usize;

        for item in &plan.items {
            let Some(slot) = self.state.conversation.get_mut(item.index) else {
                tracing::warn!(
                    index = item.index,
                    conversation_len = self.state.conversation.len(),
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
            let reestimated = super::state::estimate_conversation_tokens(&self.state.conversation);
            // Prune must never appear to increase usage.
            tokens_after = tokens_before.min(reestimated);
            self.state.total_tokens = tokens_after;
            self.persistence.replace_history(&self.state.conversation);
            tracing::info!(
                pruned_count,
                tokens_before,
                tokens_after,
                conversation_len = self.state.conversation.len(),
                "PruneToolResults: pruned oversized tool results"
            );
        }

        Ok(PruneReport {
            pruned_count,
            tokens_before,
            tokens_after,
        })
    }

    /// Approximate byte footprint of all string content in the conversation.
    ///
    /// Used for before/after measurement logging when pruning runs.
    /// Sums the byte lengths of all string fields; does not allocate.
    fn conversation_content_bytes(&self) -> usize {
        self.state
            .conversation
            .iter()
            .map(|item| match item {
                ConversationItem::System(s) => s.content.len(),
                ConversationItem::User(u) => u
                    .content
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => text.len(),
                        ContentPart::Image { url } => url.len(),
                    })
                    .sum::<usize>(),
                ConversationItem::Assistant(a) => a.content.len(),
                ConversationItem::ToolResult(tr) => tr.content.len(),
                ConversationItem::BackendToolCall(b) => b.text_summary().len(),
                ConversationItem::Reasoning(r) => {
                    sampling_types::reasoning_item_text(r).len()
                        + r.encrypted_content.as_deref().map(str::len).unwrap_or(0)
                }
            })
            .sum()
    }

    /// Record accumulated token usage and emit an event.
    pub(super) fn record_token_usage(&mut self, total_tokens: u64) {
        self.state.estimated_tokens_since_model = 0;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(&self.state.conversation);
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

    pub(super) fn increment_prompt_index(&mut self) {
        self.state.prompt_usage = None;
        self.state.prompt_index += 1;
        self.send_event(ChatStateEvent::PromptIndexChanged {
            new_index: self.state.prompt_index,
        });
    }

    /// Replace the entire conversation, persist, re-estimate `total_tokens`,
    /// and emit reset + token-update events.
    ///
    /// Compaction replaces carry the provider-side overhead forward as a
    /// *ratio* (`base_estimate × provider_total ÷ estimate_at_last_response`,
    /// capped at the pre-compaction total; `base_estimate` when that estimate is
    /// 0) so the reseed neither springs back nor over-counts (see
    /// `COMPACTION.md`).
    pub(super) fn replace_conversation(
        &mut self,
        items: Vec<ConversationItem>,
        is_compaction: bool,
    ) {
        self.snapshot_turn_slice();
        if is_compaction && let Some(cap) = &mut self.state.turn_capture {
            cap.compaction_occurred = true;
        }
        self.install_conversation(items, is_compaction);
        self.rebase_turn_capture_offset();
    }

    /// Install and persist a complete conversation without changing turn
    /// capture offsets. Callers must preserve item count and identity when a
    /// capture is active; full replacements use [`Self::replace_conversation`]
    /// so the old turn tail is snapshotted first.
    fn install_conversation(&mut self, items: Vec<ConversationItem>, is_compaction: bool) {
        self.persistence.replace_history(&items);
        self.install_persisted_conversation(items, is_compaction);
    }

    /// Apply a full conversation replacement after its persistence operation
    /// has already completed successfully.
    fn install_persisted_conversation(
        &mut self,
        items: Vec<ConversationItem>,
        is_compaction: bool,
    ) {
        let pre_replace_total = self.state.total_tokens;
        let base_estimate = super::state::estimate_conversation_tokens(&items);
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
        self.state.conversation = items;
        self.state.estimated_tokens_since_model = 0;
        self.state.total_tokens = estimated_tokens;
        self.state.estimate_at_last_response =
            super::state::estimate_conversation_tokens(&self.state.conversation);
        self.send_event(ChatStateEvent::ConversationReset {
            new_len: self.state.conversation.len(),
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
        let groups = conversation_image_groups(&self.state.conversation);
        if groups.is_empty() {
            report.unmatched_images = expected
                .values()
                .map(|rewrite| rewrite.expected_image_count)
                .sum();
            return Some(report);
        }

        let mut conversation = self.state.conversation.clone();
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
            self.state.conversation.len(),
            "image rewrite must preserve conversation item identity"
        );
        let persisted = self
            .persistence
            .replace_history_and_ack(&conversation)
            .await;
        match persisted {
            Ok(Ok(())) => {
                self.install_persisted_conversation(conversation, false);
                Some(report)
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "image history rewrite was not persisted");
                None
            }
            Err(_) => {
                tracing::warn!("image history rewrite persistence acknowledgement was dropped");
                None
            }
        }
    }

    /// Atomically swap the leading `System` message with `prompt` (or insert one
    /// if absent), persisting when changed. Runs inside the actor's command loop
    /// so it serializes with turn pushes — no lost-update race on a mid-turn
    /// reconnect. Returns whether the conversation changed.
    ///
    /// The conversation is cloned (items are `Arc`-backed, so the clone is
    /// shallow) rather than `mem::take`n: `replace_conversation` snapshots the
    /// in-flight turn-capture tail from `state.conversation` before swapping,
    /// so the state must stay intact until then.
    pub(super) fn replace_system_head(&mut self, prompt: &str) -> bool {
        if let Some(ConversationItem::System(sys)) = self.state.conversation.first()
            && crate::conversation_util::canonical_system_prompt_eq(sys.content.as_ref(), prompt)
        {
            return false;
        }
        let mut conversation = self.state.conversation.clone();
        let changed =
            crate::conversation_util::replace_or_insert_system_head(&mut conversation, prompt);
        debug_assert!(changed, "head mismatch must produce a change");
        self.replace_conversation(conversation, false);
        changed
    }

    /// Restore all state fields from a snapshot.
    pub(super) fn restore_snapshot(&mut self, snap: ChatStateSnapshot) {
        self.snapshot_turn_slice();
        // Harness trace buffers are transient (not part of the snapshot) and
        // intentionally survive a restore — see `replace_conversation`.
        self.state.conversation = snap.conversation;
        self.rebase_turn_capture_offset();
        self.state.sampling_config = snap.sampling_config;
        self.state.prompt_index = snap.prompt_index;
        self.state.total_tokens = snap.total_tokens;
        self.state.estimated_tokens_since_model = 0;
        self.state.estimate_at_last_response = if snap.estimate_at_last_response > 0 {
            snap.estimate_at_last_response
        } else {
            super::state::estimate_conversation_tokens(&self.state.conversation)
        };
        self.state.agent_edited_paths = snap.agent_edited_paths;
        self.state.prompt_texts = snap.prompt_texts;
        self.state.stream_start_ms = snap.stream_start_ms;
        self.state.turn_start_ms = snap.turn_start_ms;
        self.state.last_compaction_prompt_index = snap.last_compaction_prompt_index;
        self.state.credentials = snap.credentials;
        // Drop abandoned prompt usage; the session ledger is lifetime.
        self.state.prompt_usage = None;
    }

    /// If turn capture is active, append the current turn's tail items into
    /// `pre_replacement_messages` before an in-place mutation shifts or drops them.
    pub(super) fn snapshot_turn_slice(&mut self) {
        if let Some(cap) = &mut self.state.turn_capture {
            cap.pre_replacement_messages
                .extend_from_slice(Self::turn_tail(
                    &self.state.conversation,
                    cap.turn_start_offset,
                ));
        }
    }

    /// Re-base an active turn capture's start offset to the current conversation
    /// length after an in-place mutation, keeping the tail slice valid.
    pub(super) fn rebase_turn_capture_offset(&mut self) {
        if let Some(cap) = &mut self.state.turn_capture {
            cap.turn_start_offset = self.state.conversation.len();
        }
    }

    /// Fail-safe `conversation[offset..]` for turn capture: a capture accounting
    /// slip must never abort the user's session (a raw index here SIGABRT-crashed
    /// a live CLI), so an out-of-range offset yields an empty slice — loud in dev
    /// via `debug_assert!`, with a prod breadcrumb via `error!`.
    pub(super) fn turn_tail(
        conversation: &[ConversationItem],
        offset: usize,
    ) -> &[ConversationItem] {
        debug_assert!(
            offset <= conversation.len(),
            "turn_start_offset {offset} > len {}",
            conversation.len()
        );
        conversation.get(offset..).unwrap_or_else(|| {
            tracing::error!(
                offset,
                len = conversation.len(),
                "turn-capture offset past conversation end; trace tail dropped"
            );
            &[]
        })
    }
}
