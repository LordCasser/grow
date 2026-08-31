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
        respond_to: oneshot::Sender<PromptTurnResult>,
        persist_ack: Option<oneshot::Sender<()>>,
    ) {
        tracing::info!("queueing prompt: {prompt_id}");
        let human = !origin.is_synthetic();
        let follow_up_behavior = if human {
            crate::util::config::load_config()
                .await
                .ui
                .follow_up_behavior
        } else {
            crate::agent::config::FollowUpBehavior::Queue
        };
        let entered_during_regular_turn =
            human && self.state.lock().await.foreground.regular().is_some();
        // Build the recoverable queue projection before admission. No queue,
        // Surface, or foreground side effect is allowed before the immutable
        // payload and Input admission are durable.
        let queue_meta = human.then(|| {
            let kind = if Self::extract_bash_command(&prompt_blocks).is_some() {
                "bash"
            } else {
                "prompt"
            };
            crate::session::prompt_queue::QueueEntryMeta {
                id: prompt_id.clone(),
                version: 0,
                owner: client_identifier.clone(),
                last_editor: None,
                kind: kind.to_string(),
                text: Self::queue_text_from_blocks(&prompt_blocks),
                combined_texts: None,
            }
        });
        let input_ids = if let Some(queue) = queue_meta.as_ref() {
            let payload = crate::session::input_inbox::InputPayload::Prompt {
                prompt_id: prompt_id.clone(),
                prompt_blocks: prompt_blocks.clone(),
                client_identifier: client_identifier.clone(),
                screen_mode: screen_mode.clone(),
                verbatim,
                json_schema: json_schema.clone(),
                origin: origin.clone(),
                turn_kind,
                queue: Some(queue.into()),
            };
            let admitted = match self
                .admit_human_input(
                    if entered_during_regular_turn {
                        chat_state::InputIntent::Followup
                    } else {
                        chat_state::InputIntent::Prompt
                    },
                    payload,
                    Some(prompt_id.clone()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                )
                .await
            {
                Ok(admitted) => admitted,
                Err(reason) => {
                    let _ = respond_to.send(Err(acp::Error::internal_error().data(reason)));
                    drop(persist_ack);
                    return;
                }
            };
            vec![admitted.input_id]
        } else {
            Vec::new()
        };
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

        let mut state = self.state.lock().await;

        // `follow_up_behavior = "steer"`: a plain-Enter follow-up while a
        // regular turn owns foreground is auto-promoted into that turn through
        // the same interjection entry point as Ctrl+Enter / "Send now".
        // Idle and Compaction foreground are never steerable, so the prompt
        // falls through to the FIFO below. The decision is made under the same
        // lock as the FIFO append — foreground is the authority.
        let auto_promote_target = Self::follow_up_promotion_eligible(
            follow_up_behavior,
            state.foreground.regular().is_some(),
            origin.is_synthetic(),
            false,
            &prompt_blocks,
        )
        .then(|| state.running_prompt_id().map(str::to_owned))
        .flatten();

        // User prompts have priority over queued synthetic auto-wake prompts;
        // the guarded sweep exempts the running turn's own slot (see
        // `AdmissionState::sweep_pending_inputs`). Gate deliberately keyed on
        // completion-id-bearing synthetics only (pre-existing shape): a queue
        // holding only notification-drain synthetics is never preempted.
        if !origin.is_synthetic() {
            let preempt_armed = state.pending_inputs.iter().any(|i| {
                i.origin.completion_id().is_some()
                    && state.running_prompt_id() != Some(i.prompt_id.as_str())
            });
            if preempt_armed {
                let dropped = state.sweep_pending_inputs(|i| i.origin.is_preemptible_wake());
                tracing::info!(
                    dropped_count = dropped.len(),
                    "auto-wake: dropping pending synthetic prompts (user prompt has priority)"
                );
            }
        }

        let log_prompt_id = prompt_id.clone();
        let log_kind = queue_meta
            .as_ref()
            .map(|m| m.kind.clone())
            .unwrap_or_else(|| "synthetic".to_string());
        let log_owner = client_identifier.clone().unwrap_or_default();
        let item = InputItem {
            input_ids,
            prompt_id,
            turn_kind,
            prompt_blocks,
            client_identifier,
            screen_mode,
            verbatim,
            json_schema,
            origin,
            host_command: None,
            notification_ids: Vec::new(),
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
        drop(state);
        if let Some(target) = auto_promote_target {
            self.handle_steer_queued_prompt(
                &target,
                &log_prompt_id,
                0,
                Some(log_owner.as_str()).filter(|owner| !owner.is_empty()),
                None,
            )
            .await;
        }
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
        state: &AdmissionState,
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
    pub(super) fn broadcast_queue_changed(&self, state: &AdmissionState) {
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
        state: &AdmissionState,
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

    fn broadcast_queue_changed_inner(
        &self,
        state: &AdmissionState,
        running: Option<RunningPromptDisplay>,
    ) {
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
    fn is_running_prompt(state: &AdmissionState, prompt_id: &str) -> bool {
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
        let _control_gate = self.step_control_gate.lock().await;
        let input_ids = {
            let state = self.state.lock().await;
            (!Self::is_running_prompt(&state, id))
                .then(|| {
                    state.pending_inputs.iter().find(|item| {
                        item.queue_meta.as_ref().is_some_and(|meta| {
                            meta.id == id
                                && meta.version == expected_version
                                && owner.is_none_or(|owner| meta.owner.as_deref() == Some(owner))
                        })
                    })
                })
                .flatten()
                .map(|item| item.input_ids.clone())
        };
        if let Some(input_ids) = input_ids {
            if let Err(error) = self
                .dismiss_input_ids(input_ids, chat_state::InputDismissReason::UserRemoved)
                .await
            {
                tracing::error!(%error, queued_id = id, "queue removal was not durable");
            } else {
                let removed = {
                    let mut state = self.state.lock().await;
                    state
                        .pending_inputs
                        .iter()
                        .position(|item| {
                            item.queue_meta.as_ref().is_some_and(|meta| {
                                meta.id == id
                                    && meta.version == expected_version
                                    && owner
                                        .is_none_or(|owner| meta.owner.as_deref() == Some(owner))
                            })
                        })
                        .and_then(|pos| state.pending_inputs.remove(pos))
                };
                if let Some(item) = removed {
                    Self::respond_removed_prompt(item.respond_to);
                } else {
                    tracing::error!(
                        queued_id = id,
                        "durably dismissed queue row disappeared behind the control fence"
                    );
                }
            }
        } else {
            tracing::debug!(
                queued_id = %id,
                expected_version,
                "queue remove was a no-op (drained / stale / not owner); rebroadcasting"
            );
        }
        // Always re-broadcast the authoritative queue so the client reconciles.
        let state = self.state.lock().await;
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
        let mut effective_version = expected_version;
        if let Some(new_text) = new_text.filter(|text| !text.trim().is_empty()) {
            if !self
                .replace_queued_prompt_with_admitted_input(id, new_text.to_string(), owner)
                .await
            {
                return;
            }
            effective_version = effective_version.saturating_add(1);
        }
        let _control_gate = self.step_control_gate.lock().await;
        let (target_turn, input_ids) = {
            let state = self.state.lock().await;
            let exact_foreground = state.foreground.regular().is_some_and(|task| {
                task.prompt_id == expected_turn_id && !task.is_finished() && task.steering_open
            });
            if !exact_foreground {
                self.broadcast_queue_changed(&state);
                return;
            }
            let Some(target_turn) = self.events.current_turn() else {
                self.broadcast_queue_changed(&state);
                return;
            };
            let Some(item) = state.pending_inputs.iter().find(|item| {
                item.queue_meta.as_ref().is_some_and(|meta| {
                    meta.id == id
                        && meta.version == effective_version
                        && owner.is_none_or(|owner| meta.owner.as_deref() == Some(owner))
                })
            }) else {
                self.broadcast_queue_changed(&state);
                return;
            };
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
            if !plain_prompt {
                self.broadcast_queue_changed(&state);
                return;
            }
            (target_turn, item.input_ids.clone())
        };

        if let Err(error) = self
            .reroute_input_ids(
                input_ids.clone(),
                chat_state::InputRoute::Steer { target_turn },
            )
            .await
        {
            tracing::error!(%error, ?input_ids, "failed to reroute queued input to steer");
            return;
        }

        let mut state = self.state.lock().await;
        let exact_foreground = state.foreground.regular().is_some_and(|task| {
            task.prompt_id == expected_turn_id
                && !task.is_finished()
                && task.steering_open
                && self.events.current_turn() == Some(target_turn)
        });
        let row_matches = |item: &InputItem| {
            item.queue_meta.as_ref().is_some_and(|m| {
                m.id == id
                    && m.version == effective_version
                    && owner.is_none_or(|o| m.owner.as_deref() == Some(o))
            })
        };
        let pos = exact_foreground
            .then(|| state.pending_inputs.iter().position(row_matches))
            .flatten();
        let Some(pos) = pos else {
            self.broadcast_queue_changed(&state);
            tracing::error!(
                queued_id = id,
                expected_turn_id,
                ?input_ids,
                "durably steered queue row changed behind the control fence"
            );
            return;
        };
        let Some(mut item) = state.pending_inputs.remove(pos) else {
            return;
        };
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
        let requeue = ResidualInterjectionRequeue {
            input_ids: item.input_ids.clone(),
            prompt_id: item.prompt_id.clone(),
            origin: item.origin.clone(),
            turn_kind: item.turn_kind,
            client_identifier: item.client_identifier.clone(),
            screen_mode: item.screen_mode.clone(),
            verbatim: item.verbatim,
            json_schema: item.json_schema.clone(),
        };
        Self::respond_removed_prompt(item.respond_to);
        self.queue_auto_promoted_follow_up(text.clone(), attachments, requeue);
        self.broadcast_interjection(&text, Some(id));
        self.events
            .emit(crate::session::events::Event::Interjected {
                source: crate::session::events::InterjectionSource::Queue,
                image_count,
                redirect_kind: crate::session::events::RedirectKind::Interjection,
            });
        tracing::info!(queued_id = %id, expected_turn_id, "steered queued prompt into running turn");
        self.broadcast_queue_changed(&state);
    }

    async fn replace_queued_prompt_with_admitted_input(
        &self,
        id: &str,
        new_text: String,
        editor: Option<&str>,
    ) -> bool {
        let _control_gate = self.step_control_gate.lock().await;
        let snapshot = {
            let state = self.state.lock().await;
            if Self::is_running_prompt(&state, id) {
                return false;
            }
            let Some(item) = state
                .pending_inputs
                .iter()
                .find(|item| item.queue_meta.as_ref().is_some_and(|meta| meta.id == id))
            else {
                return false;
            };
            (
                item.prompt_id.clone(),
                item.prompt_blocks.clone(),
                item.client_identifier.clone(),
                item.screen_mode.clone(),
                item.verbatim,
                item.json_schema.clone(),
                item.origin.clone(),
                item.turn_kind,
                item.queue_meta
                    .clone()
                    .expect("queued user input has metadata"),
                state.foreground.regular().is_some(),
                item.input_ids.clone(),
            )
        };
        let mut replacement = InputItem {
            input_ids: Vec::new(),
            prompt_id: snapshot.0.clone(),
            prompt_blocks: snapshot.1,
            client_identifier: snapshot.2.clone(),
            screen_mode: snapshot.3.clone(),
            verbatim: snapshot.4,
            json_schema: snapshot.5.clone(),
            origin: snapshot.6.clone(),
            turn_kind: snapshot.7,
            host_command: None,
            notification_ids: Vec::new(),
            respond_to: tokio::sync::oneshot::channel().0,
            persist_ack: None,
            queue_meta: Some(snapshot.8),
        };
        Self::apply_queued_prompt_edit(&mut replacement, new_text.clone(), editor);
        let queue = replacement
            .queue_meta
            .as_ref()
            .expect("edited input remains queued");
        let payload = crate::session::input_inbox::InputPayload::Prompt {
            prompt_id: replacement.prompt_id.clone(),
            prompt_blocks: replacement.prompt_blocks.clone(),
            client_identifier: replacement.client_identifier.clone(),
            screen_mode: replacement.screen_mode.clone(),
            verbatim: replacement.verbatim,
            json_schema: replacement.json_schema.clone(),
            origin: replacement.origin.clone(),
            turn_kind: replacement.turn_kind,
            queue: Some(queue.into()),
        };
        let admitted = match self
            .admit_human_input(
                if snapshot.9 {
                    chat_state::InputIntent::Followup
                } else {
                    chat_state::InputIntent::Prompt
                },
                payload,
                Some(replacement.prompt_id.clone()),
                chat_state::InputRoute::Fifo,
                snapshot.10.clone(),
            )
            .await
        {
            Ok(admitted) => admitted,
            Err(error) => {
                tracing::warn!(%error, queued_id = id, "queue edit admission was blocked");
                return false;
            }
        };
        let mut state = self.state.lock().await;
        let Some(item) = state
            .pending_inputs
            .iter_mut()
            .find(|item| item.queue_meta.as_ref().is_some_and(|meta| meta.id == id))
        else {
            return false;
        };
        item.input_ids = vec![admitted.input_id];
        item.prompt_blocks = replacement.prompt_blocks;
        item.queue_meta = replacement.queue_meta;
        state.combine_edit_holds.remove(id);
        self.broadcast_queue_changed(&state);
        true
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
        let _control_gate = self.step_control_gate.lock().await;
        let selected = {
            let state = self.state.lock().await;
            let running_id = state.running_prompt_id();
            state
                .pending_inputs
                .iter()
                .filter_map(|item| {
                    let meta = item.queue_meta.as_ref()?;
                    (running_id != Some(meta.id.as_str())
                        && owner.is_none_or(|owner| meta.owner.as_deref() == Some(owner)))
                    .then(|| (meta.id.clone(), meta.version, item.input_ids.clone()))
                })
                .collect::<Vec<_>>()
        };
        let input_ids = selected
            .iter()
            .flat_map(|(_, _, input_ids)| input_ids.iter().cloned())
            .collect::<Vec<_>>();
        let mut dismissed = std::collections::BTreeSet::new();
        for chunk in input_ids.chunks(chat_state::MAX_TURN_INPUTS) {
            if let Err(error) = self
                .dismiss_input_ids(
                    chunk.iter().cloned(),
                    chat_state::InputDismissReason::UserRemoved,
                )
                .await
            {
                tracing::error!(%error, "queue clear was not durable");
                break;
            }
            dismissed.extend(chunk.iter().cloned());
        }
        let selected = selected
            .into_iter()
            .filter(|(_, _, input_ids)| input_ids.iter().all(|id| dismissed.contains(id)))
            .collect::<Vec<_>>();
        if !selected.is_empty() {
            let selected = selected
                .into_iter()
                .map(|(id, version, _)| (id, version))
                .collect::<std::collections::BTreeSet<_>>();
            let removed = {
                let mut state = self.state.lock().await;
                let mut kept = VecDeque::with_capacity(state.pending_inputs.len());
                let mut removed = Vec::new();
                for item in std::mem::take(&mut state.pending_inputs) {
                    let selected = item
                        .queue_meta
                        .as_ref()
                        .is_some_and(|meta| selected.contains(&(meta.id.clone(), meta.version)));
                    if selected {
                        removed.push(item);
                    } else {
                        kept.push_back(item);
                    }
                }
                state.pending_inputs = kept;
                removed
            };
            for item in removed {
                Self::respond_removed_prompt(item.respond_to);
            }
        }
        let state = self.state.lock().await;
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
        if !self
            .replace_queued_prompt_with_admitted_input(id, new_text, editor)
            .await
        {
            tracing::debug!(queued_id = %id, "queue edit did not replace the admitted input");
        }
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
        let mut input_count = pending.front().map_or(0, |front| front.input_ids.len());
        for _ in 1..n {
            let next_input_count = pending.get(1).map_or(0, |next| next.input_ids.len());
            if input_count.saturating_add(next_input_count) > chat_state::MAX_TURN_INPUTS {
                break;
            }
            let Some(next) = pending.remove(1) else {
                break;
            };
            input_count += next.input_ids.len();
            // The follower's text is folded into the front's turn below, so it
            // still runs — but its own queue row is gone, so it resolves as
            // RemovedFromQueue (the same completion a client sees for an
            // explicit dequeue). The multi-client UI repaints its bubble from
            // the promote broadcast's `running_combined_texts`.
            Self::respond_removed_prompt(next.respond_to);
            let extra = Self::joined_text_blocks(&next.prompt_blocks);
            if let Some(front) = pending.front_mut() {
                front.input_ids.extend(next.input_ids);
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

    fn install_user_prompt_deny_hook(actor: &SessionActor, callback_id: &str) {
        let mut client_hooks = crate::extensions::hooks::ClientHooks::new();
        client_hooks.insert(
            ::hooks::event::HookEventName::UserPromptSubmit,
            vec![crate::extensions::hooks::ClientHookGroup {
                matcher: None,
                callback_ids: vec![callback_id.to_string()],
                timeout: None,
            }],
        );
        *actor.hooks.client_hooks.borrow_mut() = client_hooks;
    }

    fn spawn_user_prompt_deny_responder(
        mut gateway_rx: tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
        reason: &'static str,
    ) {
        tokio::task::spawn_local(async move {
            while let Some(message) = gateway_rx.recv().await {
                match message {
                    acp_transport::AcpClientMessage::ExtMethod(args) => {
                        let response: Arc<serde_json::value::RawValue> =
                            serde_json::value::to_raw_value(&serde_json::json!({
                                "decision": "deny",
                                "systemMessage": reason,
                            }))
                            .unwrap()
                            .into();
                        let _ = args.response_tx.send(Ok(acp::ExtResponse::new(response)));
                    }
                    acp_transport::AcpClientMessage::SessionNotification(args) => {
                        let _ = args.response_tx.send(Ok(()));
                    }
                    _ => {}
                }
            }
        });
    }

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

    #[test]
    fn combining_prompts_preserves_every_durable_input_identity() {
        let mut first = crate::session::actor::tests::support::user_item("first", "client");
        first.input_ids = vec!["input-first".into()];
        let mut second = crate::session::actor::tests::support::user_item("second", "client");
        second.input_ids = vec!["input-second".into()];
        let mut pending = std::collections::VecDeque::from([first, second]);
        SessionActor::combine_front_pending_inputs(&mut pending, &[]);
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending.front().unwrap().input_ids,
            vec!["input-first".to_string(), "input-second".to_string()]
        );
    }

    #[test]
    fn combining_prompts_never_exceeds_the_timeline_input_batch_limit() {
        let mut pending = (0..=chat_state::MAX_TURN_INPUTS)
            .map(|index| {
                let mut item = crate::session::actor::tests::support::user_item(
                    &format!("prompt-{index}"),
                    "client",
                );
                item.input_ids = vec![format!("input-{index}")];
                item
            })
            .collect::<std::collections::VecDeque<_>>();

        SessionActor::combine_front_pending_inputs(&mut pending, &[]);

        assert_eq!(pending.len(), 2);
        assert_eq!(
            pending.front().unwrap().input_ids.len(),
            chat_state::MAX_TURN_INPUTS
        );
        assert_eq!(
            pending.back().unwrap().input_ids,
            vec![format!("input-{}", chat_state::MAX_TURN_INPUTS)]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn human_prompt_admission_is_durable_before_fifo_visibility() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let (respond_to, _response) = tokio::sync::oneshot::channel();
                actor
                    .queue_input(
                        text_blocks("hello"),
                        "prompt-order".into(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        respond_to,
                        None,
                    )
                    .await;
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let submitted = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        )
                    })
                    .unwrap();
                let triggered = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                                event: chat_state::HookEventType::UserPromptSubmit,
                                ..
                            })
                        )
                    })
                    .unwrap();
                let admitted = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::AdmissionResolved { .. }
                            )
                        )
                    })
                    .unwrap();
                assert!(matches!(
                    &events[admitted].kind,
                    chat_state::TimelineEventKind::Input(
                        chat_state::InputEvent::AdmissionResolved {
                            decision: chat_state::InputAdmissionDecision::Allow,
                            route: Some(chat_state::InputRoute::Fifo),
                            ..
                        }
                    )
                ));
                assert!(submitted < triggered && triggered < admitted);
                assert_eq!(actor.state.lock().await.pending_inputs.len(), 1);
                assert!(!events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Messages(chat_state::MessageEvent {
                        cause: chat_state::MessageCause::User,
                        ..
                    })
                )));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn denied_user_prompt_records_full_hook_lifecycle_without_fifo_turn_or_surface() {
        tokio::task::LocalSet::new()
            .run_until(async {
                const REASON: &str = "blocked by the admission test";
                let (actor, gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                install_user_prompt_deny_hook(&actor, "deny-user-prompt");
                spawn_user_prompt_deny_responder(gateway_rx, REASON);

                let (respond_to, response) = tokio::sync::oneshot::channel();
                actor
                    .queue_input(
                        text_blocks("must not enter the queue"),
                        "prompt-denied".into(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        respond_to,
                        None,
                    )
                    .await;
                assert!(
                    response
                        .await
                        .expect("prompt RPC must be resolved")
                        .is_err(),
                    "the denied prompt must be rejected at admission"
                );

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let (submitted_index, input_id) = events
                    .iter()
                    .enumerate()
                    .find_map(|(index, event)| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted { input_id, .. },
                        ) => Some((index, input_id.clone())),
                        _ => None,
                    })
                    .expect("the HumanIntent must be durable before its Hook");
                let (triggered_index, occurrence_id, run_id) = events
                    .iter()
                    .enumerate()
                    .find_map(|(index, event)| match &event.kind {
                        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            occurrence_id,
                            event: chat_state::HookEventType::UserPromptSubmit,
                            gate,
                            cause,
                            handlers,
                            ..
                        }) => {
                            assert_eq!(*gate, chat_state::HookGateKind::Prompt);
                            assert_eq!(
                                cause,
                                &chat_state::HookCause::Input {
                                    input_id: input_id.clone(),
                                }
                            );
                            assert_eq!(handlers.len(), 1);
                            assert_eq!(
                                handlers[0].provenance,
                                chat_state::HookHandlerProvenance::Client
                            );
                            assert_eq!(
                                handlers[0].action,
                                chat_state::HookHandlerPlanAction::Execute
                            );
                            Some((index, occurrence_id.clone(), handlers[0].run_id.clone()))
                        }
                        _ => None,
                    })
                    .expect("the real UserPromptSubmit Hook must be triggered");
                let run_started_index = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::RunStarted {
                                    occurrence_id: event_occurrence,
                                    run_id: event_run,
                                    handler_index: 0,
                                }
                            ) if event_occurrence == &occurrence_id && event_run == &run_id
                        )
                    })
                    .expect("the client Hook run must start durably");
                let run_finished_index = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::RunFinished {
                                    occurrence_id: event_occurrence,
                                    run_id: event_run,
                                    handler_index: 0,
                                    elapsed_ms: _,
                                    outcome: chat_state::HookRunOutcome::Blocked,
                                    control: chat_state::HookRunControl::Block { reason },
                                }
                            ) if event_occurrence == &occurrence_id
                                && event_run == &run_id
                                && reason == REASON
                        )
                    })
                    .expect("the deny decision must close the Hook run as blocked");
                let completed_index = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Completed {
                                    occurrence_id: event_occurrence,
                                    decision: chat_state::HookAggregateDecision::Prompt {
                                        decision: chat_state::HookGateDecision::Block { reason },
                                    },
                                }
                            ) if event_occurrence == &occurrence_id && reason == REASON
                        )
                    })
                    .expect("the Hook occurrence must complete with a Prompt block");
                let admission_index = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::AdmissionResolved {
                                    input_id: event_input,
                                    decision: chat_state::InputAdmissionDecision::Block {
                                        reason: chat_state::InputBlockReason::Hook { reason },
                                    },
                                    route: None,
                                    supersedes,
                                }
                            ) if event_input == &input_id
                                && reason == REASON
                                && supersedes.is_empty()
                        )
                    })
                    .expect("the blocked admission must be durable");

                assert!(
                    submitted_index < triggered_index
                        && triggered_index < run_started_index
                        && run_started_index < run_finished_index
                        && run_finished_index < completed_index
                        && completed_index < admission_index
                );
                assert!(actor.state.lock().await.pending_inputs.is_empty());
                assert!(!events[submitted_index..].iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Turn(_)
                        | chat_state::TimelineEventKind::Messages(_)
                        | chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Consumed { .. }
                        )
                )));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn subagent_user_prompt_hook_occurrence_is_confined_to_child_timeline() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (parent, _parent_gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let (mut child, child_gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let child_actor = Arc::get_mut(&mut child)
                    .expect("the child fixture must have one actor owner before startup");
                child_actor.startup_hints.is_subagent = true;
                child_actor.session_info.id = acp::SessionId::new("test-child-actor");
                child_actor
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::SubagentSeed(
                        chat_state::SubagentSeedEvent {
                            parent_timeline_id: parent.session_id_string(),
                            parent_spawn_seq: 1,
                            subagent_id: child_actor.session_id_string(),
                            security_parent_session_id: parent.session_id_string(),
                            context_source: chat_state::SubagentContextSource::New,
                            source_ref: None,
                            normalized: false,
                        },
                    ))
                    .await
                    .expect("the child identity must be durable before internal Hook activity");
                install_user_prompt_deny_hook(&child, "child-only-deny");
                spawn_user_prompt_deny_responder(child_gateway_rx, "child-only block");

                let (respond_to, response) = tokio::sync::oneshot::channel();
                child
                    .queue_input(
                        text_blocks("child prompt"),
                        "child-prompt-denied".into(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        respond_to,
                        None,
                    )
                    .await;
                assert!(response.await.unwrap().is_err());

                let child_events = child.chat_state_handle.timeline_events().await.unwrap();
                let child_occurrence = child_events
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            occurrence_id,
                            event: chat_state::HookEventType::UserPromptSubmit,
                            ..
                        }) => Some(occurrence_id.clone()),
                        _ => None,
                    })
                    .expect("the child Timeline must own its internal Hook occurrence");
                assert!(child_events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                        occurrence_id,
                        ..
                    }) if occurrence_id == &child_occurrence
                )));

                let parent_events = parent.chat_state_handle.timeline_events().await.unwrap();
                assert!(!parent_events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(
                        chat_state::HookEvent::Triggered {
                            occurrence_id,
                            event: chat_state::HookEventType::UserPromptSubmit,
                            ..
                        }
                    ) if occurrence_id == &child_occurrence
                )));
                assert!(
                    !parent_events
                        .iter()
                        .any(|event| matches!(&event.kind, chat_state::TimelineEventKind::Hook(_)))
                );
            })
            .await;
    }
}
