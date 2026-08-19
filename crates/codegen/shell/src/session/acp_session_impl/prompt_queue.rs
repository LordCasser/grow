use super::*;

/// Running-turn display fields for `grow/queue/changed` (clients paint turn-start UI).
pub(super) struct RunningPromptDisplay {
    pub id: String,
    pub text: String,
    pub kind: String,
    pub origin: String,
    pub turn_kind: String,
    pub combined_texts: Option<Vec<String>>,
}

impl SessionActor {
    /// Append one regular turn to the explicit FIFO.
    pub(super) async fn queue_input(
        &self,
        prompt_blocks: Vec<acp::ContentBlock>,
        prompt_id: String,
        origin: crate::session::PromptOrigin,
        turn_kind: crate::session::TurnKind,
        client_identifier: Option<String>,
        screen_mode: Option<String>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        task_wake_fallback: Option<TaskWakeFallback>,
        respond_to: oneshot::Sender<PromptTurnResult>,
        persist_ack: Option<oneshot::Sender<()>>,
    ) {
        tracing::info!("queueing prompt: {prompt_id}");
        let queue_depth = { self.state.lock().await.pending_inputs.len() };
        ::diagnostics::unified_log::info(
            "shell.prompt.queued",
            Some(self.session_info.id.0.as_ref()),
            Some(serde_json::json!({
                "prompt_id": prompt_id,
                "queue_depth": queue_depth,
            })),
        );

        // Bump before any await so a LocalSet recap cannot commit/emit after
        // this Prompt was accepted but before handle_prompt runs.
        if !origin.is_synthetic() {
            self.cancel_pending_recap_for_new_prompt();
        }

        if let crate::session::PromptOrigin::SubagentCompleted { subagent_id } = &origin {
            self.mark_completions_reported(&[subagent_id]).await;
        }

        // Follow-up admission policy, resolved outside the state lock (same
        // pattern as `maybe_start_running_task`'s combine_queued_prompts
        // read). Synthetic auto-wake prompts are never user follow-ups, so
        // they skip the disk read entirely.
        let follow_up_behavior = if origin.is_synthetic() {
            crate::agent::config::FollowUpBehavior::Queue
        } else {
            crate::util::config::load_config()
                .await
                .ui
                .follow_up_behavior
        };

        let mut state = self.state.lock().await;

        // `follow_up_behavior = "steer"`: a plain-Enter follow-up while a
        // regular turn owns foreground is auto-promoted into that turn through
        // the same interjection entry point as Ctrl+Enter / "Send now".
        // Idle and Compaction foreground are never steerable, so the prompt
        // falls through to the FIFO below. The decision is made under the same
        // lock as the FIFO append — foreground is the authority.
        if Self::follow_up_promotion_eligible(
            follow_up_behavior,
            state.foreground.regular().is_some(),
            origin.is_synthetic(),
            task_wake_fallback.is_some(),
            &prompt_blocks,
        ) {
            self.auto_promote_follow_up(
                prompt_blocks,
                &prompt_id,
                origin,
                turn_kind,
                client_identifier,
                screen_mode,
                verbatim,
                json_schema,
                respond_to,
            );
            self.broadcast_queue_changed(&state);
            return;
        }

        // User prompts have priority over queued synthetic auto-wake prompts;
        // the guarded sweep exempts the running turn's own slot (see
        // `State::sweep_pending_inputs`). Gate deliberately keyed on
        // completion-id-bearing synthetics only (pre-existing shape): a queue
        // holding only notification-drain synthetics is never preempted.
        if !origin.is_synthetic() {
            let preempt_armed = state.pending_inputs.iter().any(|i| {
                i.origin.completion_id().is_some()
                    && state.running_prompt_id() != Some(i.prompt_id.as_str())
            });
            if preempt_armed {
                let dropped = state.sweep_pending_inputs(|i| i.origin.is_preemptible_wake());
                if let Some(reservations) = &self.tool_context.task_completion_reservations {
                    for task_id in dropped
                        .iter()
                        .filter_map(|item| item.origin.completion_id())
                    {
                        reservations.release(task_id);
                    }
                }
                tracing::info!(
                    dropped_count = dropped.len(),
                    "auto-wake: dropping pending synthetic prompts (user prompt has priority)"
                );
            }
        }

        // Build the shared-queue metadata for user-originated prompts only.
        // Synthetic inputs (auto-wake, nudges, drains) are not user-visible
        // queue items.
        let queue_meta = if origin.is_synthetic() {
            None
        } else {
            // Derive the wire `kind` from the prompt content so the shared
            // queue / `running_prompt_id` adoption picks the right display
            // shim. Bash commands carry a bash
            // `PromptBlockMeta`; everything user-submitted here is otherwise a
            // plain prompt. (Cron prompts are server-injected via their own
            // path and render client-side.)
            let kind = if Self::extract_bash_command(&prompt_blocks).is_some() {
                "bash"
            } else {
                "prompt"
            };
            Some(crate::session::prompt_queue::QueueEntryMeta {
                id: prompt_id.clone(),
                version: 0,
                owner: client_identifier.clone(),
                last_editor: None,
                kind: kind.to_string(),
                text: Self::queue_text_from_blocks(&prompt_blocks),
                combined_texts: None,
            })
        };
        let log_prompt_id = prompt_id.clone();
        let log_kind = queue_meta
            .as_ref()
            .map(|m| m.kind.clone())
            .unwrap_or_else(|| "synthetic".to_string());
        let log_owner = client_identifier.clone().unwrap_or_default();
        let item = InputItem {
            prompt_id,
            turn_kind,
            prompt_blocks,
            client_identifier,
            screen_mode,
            verbatim,
            json_schema,
            origin,
            task_wake_fallback,
            respond_to,
            persist_ack,
            queue_meta,
        };
        state.pending_inputs.push_back(item);
        // qtrace: server appended a prompt to the authoritative FIFO. The index
        // it lands at vs whether a turn is already running tells us if it will
        // run next or queue behind others (the leader-mode source of truth
        // that clients must mirror).
        tracing::debug!(
            target: "qtrace",
            pid = std::process::id(),
            event = "server_queue_input",
            prompt_id = %log_prompt_id,
            kind = %log_kind,
            owner = %log_owner,
            new_depth = state.pending_inputs.len(),
            running_task_present = state.foreground.regular().is_some(),
            session = self.session_info.id.0.as_ref(),
            "server appended prompt to pending_inputs",
        );
        // Broadcast the new authoritative queue to all subscribers
        // (fire-and-forget, never persisted).
        self.broadcast_queue_changed(&state);
    }

    /// Admission gate for `follow_up_behavior = "steer"` at [`queue_input`]
    /// time: only a plain user prompt while a regular turn owns foreground
    /// may be auto-promoted. Idle and Compaction foreground are never
    /// steerable, explicit chords (Ctrl+Enter / double-Enter / "Send now")
    /// arrive on other commands, synthetic auto-wake prompts always stay on
    /// the FIFO, and bash/structured prompts never hijack the turn.
    fn follow_up_promotion_eligible(
        behavior: crate::agent::config::FollowUpBehavior,
        foreground_is_regular: bool,
        origin_is_synthetic: bool,
        has_task_wake_fallback: bool,
        prompt_blocks: &[acp::ContentBlock],
    ) -> bool {
        behavior == crate::agent::config::FollowUpBehavior::Steer
            && foreground_is_regular
            && !origin_is_synthetic
            && !has_task_wake_fallback
            && Self::extract_bash_command(prompt_blocks).is_none()
            && prompt_blocks.iter().all(|block| {
                matches!(
                    block,
                    acp::ContentBlock::Text(_) | acp::ContentBlock::Image(_)
                )
            })
    }

    /// Promote one plain-Enter follow-up into the running regular turn.
    /// Mirrors [`SessionActor::handle_steer_queued_prompt`]'s queue-steer
    /// exactly: same interjection buffer, same `RemovedFromQueue` resolution
    /// for the submitting client, same interjection broadcast + queue
    /// rebroadcast. The requeue payload rides the entry so a terminal fence
    /// can turn a residual back into the user FIFO as a fresh turn. Runs
    /// under the caller's state lock (foreground was just verified).
    pub(super) fn auto_promote_follow_up(
        &self,
        prompt_blocks: Vec<acp::ContentBlock>,
        prompt_id: &str,
        origin: crate::session::PromptOrigin,
        turn_kind: crate::session::TurnKind,
        client_identifier: Option<String>,
        screen_mode: Option<String>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        respond_to: oneshot::Sender<PromptTurnResult>,
    ) {
        let text = Self::queue_text_from_blocks(&prompt_blocks);
        let attachments = prompt_blocks
            .into_iter()
            .filter_map(|block| match block {
                acp::ContentBlock::Image(image) => Some(image),
                _ => None,
            })
            .collect::<Vec<_>>();
        let image_count = attachments.len() as u32;
        Self::respond_removed_prompt(respond_to);
        self.queue_auto_promoted_follow_up(
            text.clone(),
            attachments,
            AutoPromotedRequeue {
                prompt_id: prompt_id.to_string(),
                origin,
                turn_kind,
                client_identifier,
                screen_mode,
                verbatim,
                json_schema,
            },
        );
        self.broadcast_interjection(&text, Some(prompt_id));
        self.events
            .emit(crate::session::events::Event::Interjected {
                source: crate::session::events::InterjectionSource::Queue,
                image_count,
                redirect_kind: crate::session::events::RedirectKind::Interjection,
            });
        tracing::info!(
            prompt_id,
            "follow_up_behavior=steer: auto-promoted plain Enter into the running turn"
        );
    }

    /// Extract a plain-text summary of a prompt's content blocks for the
    /// shared queue display.
    ///
    /// Prefers a block's `displayText` meta (the compact user-facing form, e.g.
    /// `/loop 5s echo "x"`) over the raw wire text. A client that expands a
    /// slash skill locally sends the full expanded instruction as the wire text
    /// with the compact invocation stamped in `displayText`; the shared queue —
    /// and the turn-start shim that renders other clients' user block from this
    /// text — must show the compact form, not the raw expansion. Falls back to
    /// the joined block text when no `displayText` is present.
    pub(super) fn queue_text_from_blocks(blocks: &[acp::ContentBlock]) -> String {
        if let Some(display) = blocks.iter().find_map(|block| {
            let acp::ContentBlock::Text(t) = block else {
                return None;
            };
            t.meta
                .as_ref()
                .and_then(|m| m.get("displayText"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        }) {
            return display;
        }
        blocks
            .iter()
            .filter_map(|block| {
                if let acp::ContentBlock::Text(t) = block {
                    Some(t.text.trim())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Project the user-visible (queued, not-yet-running) prompts into the
    /// wire shape, in queue order. The currently-running prompt is excluded
    /// (it is shown via the normal turn stream, not the queue).
    pub(super) fn build_queue_wire(
        &self,
        state: &State,
    ) -> Vec<crate::session::prompt_queue::QueueEntryWire> {
        // Race-free running identity (`running_task` lives under the same lock
        // as `pending_inputs`); the `current_prompt_id` pin is cleared early by
        // `handle_completion`, which would briefly re-list the finished-but-
        // unpopped front as a queued row.
        let running_id = state.running_prompt_id();
        let mut out = Vec::new();
        for item in &state.pending_inputs {
            let Some(meta) = &item.queue_meta else {
                continue;
            };
            if running_id == Some(meta.id.as_str()) {
                // This item is the in-flight turn, not a queued prompt.
                continue;
            }
            out.push(crate::session::prompt_queue::QueueEntryWire {
                id: meta.id.clone(),
                version: meta.version,
                owner: meta.owner.clone(),
                last_editor: meta.last_editor.clone(),
                kind: meta.kind.clone(),
                text: meta.text.clone(),
                combined_texts: meta.combined_texts.clone(),
                position: out.len(),
            });
        }
        out
    }

    /// Broadcast the current authoritative prompt queue to all subscribers
    /// Fire-and-forget via the gateway, carrying `sessionId`
    /// so session routing fans it to every attached client. Never persisted.
    pub(super) fn broadcast_queue_changed(&self, state: &State) {
        let running = state.running_prompt_id().and_then(|pid| {
            state
                .pending_inputs
                .iter()
                .find(|i| i.prompt_id == pid)
                .map(Self::running_display_from_item)
        });
        self.broadcast_queue_changed_inner(state, running);
    }

    /// Broadcast with explicit running-turn display (promote before `running_task`
    /// so clients paint before the user-echo races in).
    pub(super) fn broadcast_queue_changed_promoting(
        &self,
        state: &State,
        running: RunningPromptDisplay,
    ) {
        self.broadcast_queue_changed_inner(state, Some(running));
    }

    pub(super) fn running_display_from_item(item: &InputItem) -> RunningPromptDisplay {
        let meta = item.queue_meta.as_ref();
        RunningPromptDisplay {
            id: item.prompt_id.clone(),
            text: meta
                .map(|m| m.text.clone())
                .unwrap_or_else(|| Self::queue_text_from_blocks(&item.prompt_blocks)),
            kind: meta
                .map(|m| m.kind.clone())
                .unwrap_or_else(|| "prompt".to_string()),
            origin: item.origin.wire_name().to_string(),
            turn_kind: item.turn_kind.wire_name().to_string(),
            combined_texts: meta
                .and_then(|m| m.combined_texts.clone())
                .filter(|v| v.len() >= 2),
        }
    }

    fn broadcast_queue_changed_inner(&self, state: &State, running: Option<RunningPromptDisplay>) {
        let running_id = running.as_ref().map(|r| r.id.clone());
        // Exclude the running/promoting row from `entries` (same as when
        // `running_task` is set).
        let mut entries = self.build_queue_wire(state);
        if let Some(rid) = running_id.as_deref() {
            entries.retain(|e| e.id != rid);
            for (i, e) in entries.iter_mut().enumerate() {
                e.position = i;
            }
        }
        let (running_text, running_kind, running_origin, running_turn_kind, running_combined_texts) =
            match running {
                Some(r) => (
                    Some(r.text),
                    Some(r.kind),
                    Some(r.origin),
                    Some(r.turn_kind),
                    r.combined_texts,
                ),
                None => (None, None, None, None, None),
            };
        let payload = crate::session::prompt_queue::QueueChanged {
            session_id: self.session_info.id.0.to_string(),
            entries,
            running_prompt_id: running_id,
            running_text,
            running_kind,
            running_origin,
            running_turn_kind,
            running_combined_texts,
        };
        tracing::debug!(
            target: "qtrace",
            pid = std::process::id(),
            event = "server_broadcast_queue",
            running_prompt_id = payload.running_prompt_id.as_deref().unwrap_or(""),
            combined_segs = payload
                .running_combined_texts
                .as_ref()
                .map(|v| v.len())
                .unwrap_or(0),
            entry_count = payload.entries.len(),
            entries = ?payload.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            session = self.session_info.id.0.as_ref(),
            "broadcasting grow/queue/changed to subscribers",
        );
        if let Ok(params) = serde_json::value::to_raw_value(&payload) {
            self.notifications
                .gateway
                .forward_fire_and_forget(acp::ExtNotification::new(
                    crate::session::prompt_queue::QUEUE_CHANGED_METHOD,
                    params.into(),
                ));
        }
    }

    /// Whether `prompt_id` is the currently in-flight turn (and so must never
    /// be removed/reordered by a queue edit). Keyed on `state.running_task`
    /// (race-free under the caller's state lock), not the `current_prompt_id`
    /// pin, which `handle_completion` clears while the finished front is still
    /// unpopped — a queue edit in that window must still refuse the front.
    fn is_running_prompt(state: &State, prompt_id: &str) -> bool {
        state.running_prompt_id() == Some(prompt_id)
    }

    /// Resolve a removed prompt's pending RPC with `Ok(RemovedFromQueue)` before dropping it. A
    /// dropped sender would look like the running turn failing; the `Ok` lets the client discard it.
    /// It never ran, so token count is `0`.
    pub(super) fn respond_removed_prompt(respond_to: oneshot::Sender<PromptTurnResult>) {
        let _ = respond_to.send(Ok(PromptTurnOk {
            stop_reason: acp::StopReason::Cancelled,
            total_tokens: 0,
            turn_snapshot: None,
            completion_kind: PromptCompletionKind::RemovedFromQueue,
            structured_output: None,
            usage: None,
        }));
    }

    pub(super) async fn handle_remove_queued_prompt(
        &self,
        id: &str,
        expected_version: u64,
        owner: Option<&str>,
    ) {
        let mut state = self.state.lock().await;
        let mut removed = false;
        if !Self::is_running_prompt(&state, id)
            && let Some(pos) = state.pending_inputs.iter().position(|item| {
                item.queue_meta.as_ref().is_some_and(|m| {
                    m.id == id
                        && m.version == expected_version
                        && owner.is_none_or(|o| m.owner.as_deref() == Some(o))
                })
            })
        {
            if let Some(item) = state.pending_inputs.remove(pos) {
                Self::respond_removed_prompt(item.respond_to);
            }
            removed = true;
        }
        if !removed {
            tracing::debug!(
                queued_id = %id,
                expected_version,
                "queue remove was a no-op (drained / stale / not owner); rebroadcasting"
            );
        }
        // Always re-broadcast the authoritative queue so the client reconciles.
        self.broadcast_queue_changed(&state);
    }

    /// Atomically move one queued user row into the identified running turn.
    pub(super) async fn handle_steer_queued_prompt(
        &self,
        expected_turn_id: &str,
        id: &str,
        expected_version: u64,
        owner: Option<&str>,
        new_text: Option<&str>,
    ) {
        let mut state = self.state.lock().await;
        let running_front_id = state.running_prompt_id().map(str::to_string);
        if running_front_id.as_deref() != Some(expected_turn_id) {
            self.broadcast_queue_changed(&state);
            return;
        }
        let row_matches = |item: &InputItem| {
            item.queue_meta.as_ref().is_some_and(|m| {
                m.id == id
                    && m.version == expected_version
                    && owner.is_none_or(|o| m.owner.as_deref() == Some(o))
            })
        };
        let running_is_row = running_front_id.as_deref() == Some(id);
        let pos = if running_is_row {
            None
        } else {
            state.pending_inputs.iter().position(row_matches)
        };
        if let Some(pos) = pos
            && let Some(mut item) = state.pending_inputs.remove(pos)
        {
            // Client-edited text wins (LWW).
            if let Some(new_text) = new_text.filter(|t| !t.trim().is_empty()) {
                Self::apply_queued_prompt_edit(&mut item, new_text.to_string(), owner);
            }
            let plain_prompt = item
                .queue_meta
                .as_ref()
                .is_some_and(|meta| meta.kind == "prompt")
                && Self::extract_bash_command(&item.prompt_blocks).is_none()
                && item.prompt_blocks.iter().all(|block| {
                    matches!(
                        block,
                        acp::ContentBlock::Text(_) | acp::ContentBlock::Image(_)
                    )
                });
            if plain_prompt {
                let text = Self::queue_text_from_blocks(&item.prompt_blocks);
                let attachments = item
                    .prompt_blocks
                    .drain(..)
                    .filter_map(|block| match block {
                        acp::ContentBlock::Image(image) => Some(image),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let image_count = attachments.len() as u32;
                Self::respond_removed_prompt(item.respond_to);
                self.queue_mid_turn_interjection(text.clone(), attachments);
                self.broadcast_interjection(&text, Some(id));
                self.events
                    .emit(crate::session::events::Event::Interjected {
                        source: crate::session::events::InterjectionSource::Queue,
                        image_count,
                        redirect_kind: crate::session::events::RedirectKind::Interjection,
                    });
                tracing::info!(queued_id = %id, expected_turn_id, "steered queued prompt into running turn");
            } else {
                state.pending_inputs.insert(pos, item);
            }
        } else if let Some(new_text) = new_text
            && !new_text.trim().is_empty()
            && !running_is_row
            && let Some(item) = state
                .pending_inputs
                .iter_mut()
                .find(|item| row_matches(item))
        {
            // The send-now no-opped but the row is still queued: keep the
            // edit as an LWW write so it isn't silently lost. Stale versions
            // get no fallback (LWW); the running turn is never edited.
            Self::apply_queued_prompt_edit(item, new_text.to_string(), owner);
            tracing::info!(
                queued_id = %id,
                "steer no-opped; saved the edit to the queued row"
            );
        } else {
            tracing::debug!(
                queued_id = %id,
                expected_version,
                "queue steer no-op (running id / stale / drained / not owner); rebroadcasting"
            );
        }
        // Always re-broadcast the authoritative queue so the client reconciles.
        self.broadcast_queue_changed(&state);
    }

    /// Reorder queued prompts to match `ordered_ids`. The
    /// running turn (front, if active) stays pinned at the front; queued items
    /// not named in `ordered_ids` keep their relative order behind the named
    /// ones. Idempotent; re-broadcasts the result.
    pub(super) async fn handle_reorder_queue(&self, ordered_ids: &[String]) {
        let mut state = self.state.lock().await;

        // Partition: items we never reorder (running turn front + synthetic /
        // non-queue items) vs reorderable queued user prompts.
        let running_id = state.running_prompt_id().map(str::to_string);
        let mut pinned: std::collections::VecDeque<InputItem> = std::collections::VecDeque::new();
        let mut queued: Vec<InputItem> = Vec::new();
        for item in std::mem::take(&mut state.pending_inputs) {
            let is_queueable = item
                .queue_meta
                .as_ref()
                .is_some_and(|m| running_id.as_deref() != Some(m.id.as_str()));
            if is_queueable {
                queued.push(item);
            } else {
                pinned.push_back(item);
            }
        }

        // Stable reorder: named ids first (in requested order), then the rest.
        let rank = |item: &InputItem| -> usize {
            item.queue_meta
                .as_ref()
                .and_then(|m| ordered_ids.iter().position(|x| x == &m.id))
                .unwrap_or(usize::MAX)
        };
        queued.sort_by_key(rank);

        let mut rebuilt = pinned;
        rebuilt.extend(queued);
        state.pending_inputs = rebuilt;

        self.broadcast_queue_changed(&state);
    }

    /// Clear queued prompts. When `owner` is `Some`, only that
    /// client's queued items are removed. The running turn is never touched.
    pub(super) async fn handle_clear_queue(&self, owner: Option<&str>) {
        let mut state = self.state.lock().await;
        // Partition rather than `retain`: each cleared user prompt still has a
        // client awaiting its `respond_to`, so it must be resolved with
        // `Cancelled` (see [`respond_removed_prompt`]) instead of being
        // dropped — a bare drop surfaces as "session failed to respond" and a
        // spurious "Turn failed" on the running turn.
        let running_id = state.running_prompt_id().map(str::to_string);
        let mut kept = VecDeque::with_capacity(state.pending_inputs.len());
        for item in std::mem::take(&mut state.pending_inputs) {
            let keep = match &item.queue_meta {
                // Non-queue (synthetic) items always stay.
                None => true,
                // Never drop the in-flight turn; keep items NOT owned by the
                // requester (owner-scoped clear).
                Some(meta) => {
                    running_id.as_deref() == Some(meta.id.as_str())
                        || owner.is_some_and(|o| meta.owner.as_deref() != Some(o))
                }
            };
            if keep {
                kept.push_back(item);
            } else {
                Self::respond_removed_prompt(item.respond_to);
            }
        }
        state.pending_inputs = kept;
        self.broadcast_queue_changed(&state);
    }

    /// Replace the text of a queued (not-yet-running) prompt in place
    /// (LWW).
    ///
    /// Semantics — last write wins via the actor's serialized mailbox.
    /// Concretely, for an entry whose `queue_meta.id == id`:
    /// 1. Rebuild the underlying `prompt_blocks` as a single
    ///    [`acp::TextContent`] block carrying `new_text` (any non-text blocks
    ///    such as pasted images on the original prompt are not preserved — the
    ///    user has explicitly typed replacement text).
    /// 2. Update `queue_meta.text`, bump `queue_meta.version`, and record
    ///    `last_editor` (the original `owner` attribution is preserved).
    /// 3. Re-broadcast `grow/queue/changed` so every subscriber renders the
    ///    new text and version.
    ///
    /// **No-op cases** (each is a benign discard with no rebroadcast — nothing
    /// changed):
    /// - The id is not in `pending_inputs` (already drained / removed).
    /// - The id names the currently-running turn — editing the live turn is
    ///   out of scope.
    /// - `new_text` is blank (a queued prompt is never blanked).
    pub(super) async fn handle_edit_queued_prompt(
        &self,
        id: &str,
        new_text: String,
        editor: Option<&str>,
    ) {
        if new_text.trim().is_empty() {
            tracing::debug!(queued_id = %id, "queue edit no-op: empty newText");
            return;
        }
        let mut state = self.state.lock().await;
        // Locked first: the promoter arms `running_task` under this lock.
        if Self::is_running_prompt(&state, id) {
            tracing::debug!(
                queued_id = %id,
                "queue edit no-op: id names the running turn"
            );
            return;
        }
        let Some(item) = state
            .pending_inputs
            .iter_mut()
            .find(|item| item.queue_meta.as_ref().is_some_and(|m| m.id == id))
        else {
            tracing::debug!(
                queued_id = %id,
                "queue edit no-op: id not found (already drained / removed)"
            );
            return;
        };
        Self::apply_queued_prompt_edit(item, new_text, editor);
        // Clear the hold under the same lock as the text update — see
        // pager `exit_editing_mode_keeping_hold` for the race this closes.
        state.combine_edit_holds.remove(id);
        self.broadcast_queue_changed(&state);
    }

    /// Merge consecutive plain prompts into `pending[0]` via
    /// [`prompt_queue::combine_prefix_len`]. `skip_ids` holds rows under
    /// composer edit. Merged-away items complete as
    /// [`PromptCompletionKind::RemovedFromQueue`].
    pub(super) fn combine_front_pending_inputs(
        pending: &mut std::collections::VecDeque<InputItem>,
        skip_ids: &[&str],
    ) {
        use ::prompt_queue::{CombineGate, combine_prefix_len};

        if pending.len() < 2 {
            return;
        }
        let gates: Vec<CombineGate<'_>> = pending.iter().map(Self::combine_gate).collect();
        let n = combine_prefix_len(gates, skip_ids);
        if n < 2 {
            return;
        }
        for _ in 1..n {
            let Some(next) = pending.remove(1) else {
                break;
            };
            // The follower's text is folded into the front's turn below, so it
            // still runs — but its own queue row is gone, so it resolves as
            // RemovedFromQueue (the same completion a client sees for an
            // explicit dequeue). The multi-client UI repaints its bubble from
            // the promote broadcast's `running_combined_texts`.
            Self::respond_removed_prompt(next.respond_to);
            let extra = Self::joined_text_blocks(&next.prompt_blocks);
            if let Some(front) = pending.front_mut() {
                Self::append_text_to_prompt(front, &extra);
            }
        }
    }

    fn combine_gate(item: &InputItem) -> ::prompt_queue::CombineGate<'_> {
        let is_bash = Self::extract_bash_command(&item.prompt_blocks).is_some();
        let is_plain_prompt =
            item.queue_meta.as_ref().map(|m| m.kind.as_str()) == Some("prompt") && !is_bash;
        let mut has_text = false;
        let mut has_images = false;
        let mut is_expanded_skill = false;
        let mut non_text_non_image = false;
        for block in &item.prompt_blocks {
            match block {
                acp::ContentBlock::Text(t) => {
                    if Self::has_display_text(t) {
                        is_expanded_skill = true;
                    }
                    if !t.text.is_empty() {
                        has_text = true;
                    }
                }
                acp::ContentBlock::Image(_) => has_images = true,
                _ => non_text_non_image = true,
            }
        }
        // Follower eligibility also requires single plain text; encode via
        // is_expanded_skill / has_images / non_text_non_image.
        let text = item
            .queue_meta
            .as_ref()
            .map(|m| m.text.as_str())
            .unwrap_or("");
        ::prompt_queue::CombineGate {
            id: item.prompt_id.as_str(),
            is_plain_prompt: is_plain_prompt && has_text && !non_text_non_image,
            is_synthetic: item.origin.is_synthetic(),
            is_expanded_skill,
            is_bash,
            has_images,
            text: if text.is_empty() {
                // Fall back so empty meta still participates when blocks have text.
                item.prompt_blocks
                    .iter()
                    .find_map(|b| match b {
                        acp::ContentBlock::Text(t) if !t.text.is_empty() => Some(t.text.as_str()),
                        _ => None,
                    })
                    .unwrap_or("")
            } else {
                text
            },
        }
    }

    fn has_display_text(t: &acp::TextContent) -> bool {
        t.meta
            .as_ref()
            .and_then(|m| m.get("displayText"))
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    }

    fn joined_text_blocks(blocks: &[acp::ContentBlock]) -> String {
        use ::prompt_queue::join_texts;
        join_texts(blocks.iter().filter_map(|block| match block {
            acp::ContentBlock::Text(t) if !t.text.is_empty() => Some(t.text.as_str()),
            _ => None,
        }))
    }

    fn append_text_to_prompt(item: &mut InputItem, extra: &str) {
        use ::prompt_queue::TEXT_SEPARATOR;

        if extra.is_empty() {
            return;
        }
        if let Some(meta) = item.queue_meta.as_mut() {
            match meta.combined_texts.as_mut() {
                Some(segs) => segs.push(extra.to_string()),
                None => {
                    meta.combined_texts = Some(vec![meta.text.clone(), extra.to_string()]);
                }
            }
        }
        // Append to the LAST text block so a multi-text front stays ordered
        // (front text first, then the follower); `combined_texts` mirrors that.
        if let Some(acp::ContentBlock::Text(t)) = item
            .prompt_blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, acp::ContentBlock::Text(_)))
        {
            if !t.text.is_empty() {
                t.text.push_str(TEXT_SEPARATOR);
            }
            t.text.push_str(extra);
        }
        if let Some(meta) = item.queue_meta.as_mut() {
            meta.text = Self::queue_text_from_blocks(&item.prompt_blocks);
        }
        Self::stamp_combined_display_texts_meta(item);
    }

    fn stamp_combined_display_texts_meta(item: &mut InputItem) {
        use ::prompt_queue::stamp_combined_display_texts;

        let Some(segs) = item
            .queue_meta
            .as_ref()
            .and_then(|m| m.combined_texts.as_ref())
            .cloned()
        else {
            return;
        };
        // Stamp the first text block (matches append_text_to_prompt); an
        // image-first front would otherwise lose the replay multi-bubble meta.
        let Some(acp::ContentBlock::Text(t)) = item
            .prompt_blocks
            .iter_mut()
            .find(|b| matches!(b, acp::ContentBlock::Text(_)))
        else {
            return;
        };
        let map = t.meta.get_or_insert_with(acp::Meta::new);
        stamp_combined_display_texts(map, &segs);
    }

    /// Replace a queued item's prompt body with `new_text` and bump its LWW
    /// version metadata. Shared by `handle_edit_queued_prompt` and the
    /// turn-ended fallback in `handle_interject_queued_prompt`.
    ///
    /// Replaces the text blocks with a single text block carrying the new
    /// text; Image blocks are RETAINED — the queue-edit wire is text-only,
    /// so a text edit must never silently detach the row's pasted images
    /// (mirrors the pager's local-row edit semantics). Other non-text
    /// blocks are still dropped — an explicit retype is a fresh prompt
    /// body. The `displayText` meta is left unset so the queue text shown
    /// to other clients is exactly what the editor typed (no stale skill
    /// expansion).
    fn apply_queued_prompt_edit(item: &mut InputItem, new_text: String, editor: Option<&str>) {
        // A bash row executes `extract_bash_command`'s meta value, not the
        // block text — rebuild the meta with the edited text or the edit
        // demotes the row to a plain model prompt.
        let meta = Self::extract_bash_command(&item.prompt_blocks)
            .is_some()
            .then(|| {
                let value = serde_json::to_value(
                    crate::extensions::prompt_meta::PromptBlockMeta::bash(new_text.clone()),
                )
                .expect("PromptBlockMeta serializes");
                value
                    .as_object()
                    .cloned()
                    .expect("PromptBlockMeta serializes to object")
            });
        let mut blocks = vec![acp::ContentBlock::Text(
            acp::TextContent::new(new_text.clone()).meta(meta),
        )];
        blocks.extend(
            std::mem::take(&mut item.prompt_blocks)
                .into_iter()
                .filter(|b| matches!(b, acp::ContentBlock::Image(_))),
        );
        item.prompt_blocks = blocks;
        if let Some(meta) = item.queue_meta.as_mut() {
            meta.text = new_text;
            meta.combined_texts = None;
            meta.version = meta.version.saturating_add(1);
            meta.last_editor = editor.map(str::to_string);
        }
    }
}

#[cfg(test)]
mod follow_up_admission_tests {
    use super::*;
    use crate::agent::config::FollowUpBehavior;

    fn text_blocks(text: &str) -> Vec<acp::ContentBlock> {
        vec![acp::ContentBlock::Text(acp::TextContent::new(
            text.to_string(),
        ))]
    }

    fn bash_blocks() -> Vec<acp::ContentBlock> {
        let value =
            serde_json::to_value(crate::extensions::prompt_meta::PromptBlockMeta::bash("ls"))
                .expect("PromptBlockMeta serializes");
        let meta = value.as_object().cloned();
        vec![acp::ContentBlock::Text(
            acp::TextContent::new("ls".to_string()).meta(meta),
        )]
    }

    #[test]
    fn steer_promotes_plain_enter_only_during_regular_turn() {
        assert!(SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Steer,
            true,
            false,
            false,
            &text_blocks("hi")
        ));
        // Idle foreground is not steerable.
        assert!(!SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Steer,
            false,
            false,
            false,
            &text_blocks("hi")
        ));
        // Default behavior never promotes.
        assert!(!SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Queue,
            true,
            false,
            false,
            &text_blocks("hi")
        ));
    }

    #[test]
    fn synthetic_bash_and_wake_fallback_never_promote() {
        assert!(!SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Steer,
            true,
            true,
            false,
            &text_blocks("auto-wake")
        ));
        assert!(!SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Steer,
            true,
            false,
            false,
            &bash_blocks()
        ));
        assert!(!SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Steer,
            true,
            false,
            true,
            &text_blocks("hi")
        ));
    }

    #[test]
    fn image_carrying_plain_prompt_is_promotable() {
        let blocks = vec![
            acp::ContentBlock::Text(acp::TextContent::new("look".to_string())),
            acp::ContentBlock::Image(acp::ImageContent::new(
                String::new(),
                "image/png".to_string(),
            )),
        ];
        assert!(SessionActor::follow_up_promotion_eligible(
            FollowUpBehavior::Steer,
            true,
            false,
            false,
            &blocks
        ));
    }
}
