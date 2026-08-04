//! Mid-turn interjection concern for `SessionActor` (buffer type, formatting,
//! broadcast, drain). Also hosts `inject_synthetic_user_message`, the shared
//! synthetic-user-message injector the permission-panel followup path reuses.

use super::*;

// Buffer, entry type, and formatting live with the tool execution layer so
// shell and tool loops share semantics. The shell keeps arrival (ACP ext
// methods), persistence, and pager echo.
//
// Re-exported for `acp_session.rs` which does `pub(crate) use interjection::*;`
// so retained code and co-located tests keep resolving by `acp_session::` path.
#[allow(unused_imports)]
pub(crate) use tools::interjection::{InterjectionBuffer, drain_formatted, format_interjection};

/// Shell instantiation of the shared entry type: images are ACP content.
pub(crate) type PendingInterjection = tools::interjection::PendingInterjection<acp::ImageContent>;

/// Prompt-id prefix for interjections that missed their turn and were
/// converted into standalone prompt turns (arrived while idle, or after the
/// running turn's final drain). The prefix keeps the turn's user echo
/// persist-only: every pane already rendered the text from the
/// `grow/session/interjection` broadcast, so a live echo would duplicate it.
pub(crate) const INTERJECT_FALLBACK_PROMPT_PREFIX: &str = "interject-fallback-";

impl SessionActor {
    /// Common mid-turn steering path shared by the direct `Interject`
    /// command and the queue send-now (`InterjectQueuedPrompt`) path: bump
    /// the Goal autonomy generation while the Goal is Active (invalidating
    /// any in-flight verifier stage or planner result captured under the old
    /// generation) and buffer the text for the turn loop's next safe
    /// boundary. Callers own the broadcast / event / idle-fallback
    /// differences (source `Direct` vs `Queue`, echo id, prompt fallback).
    pub(super) fn queue_mid_turn_interjection(
        &self,
        text: String,
        attachments: Vec<acp::ImageContent>,
    ) {
        if self.goal_tracker.lock().status()
            == Some(crate::session::goal_tracker::GoalStatus::Active)
        {
            self.goal_tracker.lock().bump_autonomy_generation();
        }
        self.pending_interjections
            .push(PendingInterjection { text, attachments });
    }

    /// Convert a stranded interjection into a queued prompt turn.
    ///
    /// An interjection is only merged into a *running* turn
    /// (`drain_pending_interjections`); one that arrives while the session is
    /// idle — or lands after the running turn's final drain — would otherwise
    /// sit in `pending_interjections` forever and the user's message would be
    /// silently lost (the pager already rendered it and said "Interjection
    /// sent"). Queue it as its own prompt turn instead; the caller kicks
    /// `maybe_start_running_task`.
    ///
    /// `front` puts the converted turn ahead of already-queued prompts —
    /// send-now semantics: the user asked for "now", queued rows asked for
    /// "later". Front placement is re-validated under the state lock: the
    /// caller's "no turn running" check is unlocked, so a concurrent
    /// promotion (MCP-init release, plan-approval resume) may have pinned a
    /// running prompt at the front in the meantime — displacing it would
    /// desync `handle_completion`'s front pop. In that case the item lands
    /// right behind the running front.
    pub(super) async fn queue_interjection_fallback_prompt(
        &self,
        text: String,
        images: Vec<acp::ImageContent>,
        front: bool,
    ) {
        let prompt_id = format!("{INTERJECT_FALLBACK_PROMPT_PREFIX}{}", uuid::Uuid::now_v7());
        let mut prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(text))];
        prompt_blocks.extend(images.into_iter().map(acp::ContentBlock::Image));
        // Preserve the selected Behavior when an interjection becomes a new
        // turn; it must not silently fall back to Normal.
        let prompt_mode = *self.current_prompt_mode.lock();
        let (respond_to, _) = tokio::sync::oneshot::channel();
        // User message (skips queue_input); invalidate in-flight recap now.
        self.cancel_pending_recap_for_new_prompt();
        let item = InputItem {
            prompt_id,
            prompt_blocks,
            prompt_mode,
            client_identifier: None,
            screen_mode: None,
            verbatim: false,
            json_schema: None,
            origin: super::super::PromptOrigin::User,
            task_wake_fallback: None,
            respond_to,
            persist_ack: None,
            parsed_prompt_tx: None,
            queue_meta: None,
            // Send-now semantics (see doc): a later real send-now must not
            // leapfrog this fallback in `queue_input`'s FIFO scan.
            send_now: front,
        };
        let mut state = self.state.lock().await;
        if front {
            // Never displace a running front (see doc): insert after it when
            // the front row is the in-flight turn's own item.
            let insert_at = usize::from(matches!(
                (state.pending_inputs.front(), state.running_prompt_id()),
                (Some(front_item), Some(running)) if front_item.prompt_id == running
            ));
            state.pending_inputs.insert(insert_at, item);
        } else {
            state.pending_inputs.push_back(item);
        }
        tracing::info!("Converted stranded interjection into a queued prompt turn");
    }

    /// Flush interjections that missed the completed turn's final drain into
    /// queued prompt turns (front of the queue, original order). Returns
    /// whether anything was flushed; the caller kicks
    /// `maybe_start_running_task`.
    pub(super) async fn flush_stranded_interjections(&self) -> bool {
        let stranded = self.pending_interjections.drain_all();
        if stranded.is_empty() {
            return false;
        }
        // Reversed push_fronts keep entry 0 front-most.
        for entry in stranded.into_iter().rev() {
            self.queue_interjection_fallback_prompt(entry.text, entry.attachments, true)
                .await;
        }
        true
    }
    /// Normalize interjection images for injection (shared pipeline above);
    /// notices append to `wrapped` (TEXT side only). Returns the images to
    /// attach structurally after normalization.
    async fn prepare_interjection_images(
        &self,
        wrapped: &mut String,
        images: Vec<acp::ImageContent>,
    ) -> Vec<acp::ImageContent> {
        if images.is_empty() {
            return images;
        }
        self.normalize_images_with_notices(wrapped, images, false)
            .await
    }

    /// Broadcast a mid-turn interjection to every attached client.
    /// Fan it out (sessionId-routed, fire-and-forget) so every pane viewing the
    /// session renders the interjection block — not just the originator. The
    /// originating pager rendered an optimistic block locally and dedups this
    /// echo by `id`; other panes (which never minted the id) render it. `id` is
    /// to chat state. Shared by `add_followup_message_as_user_turn` (which
    /// `None` only for older clients, in which case every pane renders.
    pub(super) fn broadcast_interjection(&self, text: &str, id: Option<&str>) {
        let mut payload = serde_json::json!({
            "sessionId": self.session_info.id.0.as_ref(),
            "text": text,
        });
        if let Some(id) = id {
            payload["interjectionId"] = serde_json::json!(id);
        }
        if let Ok(params) = serde_json::value::to_raw_value(&payload) {
            self.notifications
                .gateway
                .forward_fire_and_forget(acp::ExtNotification::new(
                    "grow/session/interjection",
                    params.into(),
                ));
        }
    }

    /// Inject a synthetic user message: persist, optionally notify pager, push
    /// notifies) and `drain_pending_interjections` (which skips notification
    /// `<skill_information>` envelope (loaded + substituted SKILL.md bodies).
    /// because the pager already has a local user prompt block).
    pub(super) async fn inject_synthetic_user_message(
        &self,
        text: &str,
        item: ConversationItem,
        notify_pager: bool,
        images: &[acp::ImageContent],
    ) {
        let model_id = self.current_model_id().await;
        let user_chunk_meta = serde_json::json!({ "modelId": model_id })
            .as_object()
            .cloned();

        // Persist to updates.jsonl: one UserMessageChunk per content block
        // (text first, then any images — Image chunks already round-trip).
        let mut content_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            text.to_string(),
        ))];
        content_blocks.extend(images.iter().cloned().map(acp::ContentBlock::Image));
        let notification_meta = self.build_notification_meta();
        for content_block in content_blocks {
            let update = acp::SessionUpdate::UserMessageChunk(
                acp::ContentChunk::new(content_block).meta(user_chunk_meta.clone()),
            );
            let _ = self
                .notifications
                .persistence_tx
                .send(PersistenceMsg::Update(SessionUpdate::Acp(Box::new(
                    acp::SessionNotification::new(self.session_info.id.clone(), update)
                        .meta(notification_meta.clone().as_object().cloned()),
                ))));
        }

        // Notify pager (skipped for interjections — pager has local block).
        if notify_pager {
            self.send_update(
                acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                        text.to_string(),
                    )))
                    .meta(user_chunk_meta),
                ),
                None,
            )
            .await;
        }

        // Add to conversation context
        self.chat_state_handle.push_user_message(item);
    }

    /// Expand skill slash references in interjection text into the
    ///
    /// Interjections bypass turn-start slash resolution
    /// (`slash_commands::resolve`), so without this a queued `/skill` row
    /// force-sent mid-turn — or a typed `/skill` interjection — reaches the
    /// model as a bare, unexpanded slash command. Returns `None` when the
    /// conversation as a standalone synthetic user message
    /// text references no known skill.
    async fn interjection_skill_information(&self, text: &str) -> Option<String> {
        // Mirror turn-start gating (`parse_slash_prefix`): only a leading
        // slash invokes skills — "don't run /commit yet" is steering text,
        // not an invocation.
        if !text.trim_start().starts_with('/') {
            return None;
        }
        let slash_skills = self.slash_skills_for_resolve().await;
        // Availability without `command_availability()`'s goal-reconciliation
        // side effects — this runs mid-turn inside the drain.
        let tool_names = self.registered_tool_names().await;
        let has_workflow_runs = !self.workflow_tracker().await.lock().list().is_empty();
        let availability = self.build_command_availability(&tool_names, has_workflow_runs);
        let parsed = slash_commands::parse_skill_references(text, &slash_skills, availability)?;
        // Deliberately lighter diagnostics than turn start: no `skill.activated`
        // span, `PluginUsed`, or `active_skill` stamp — those attribute the
        // turn, which this skill did not start. `SkillDispatched` still
        // carries `plugin_source`, so dispatch counts stay complete.
        for sk in &parsed {
            ::diagnostics::session_ctx::log_event(::diagnostics::events::SlashCommandUsed {
                command: sk.name.clone(),
                args_provided: !sk.args.is_empty(),
            });
            ::diagnostics::session_ctx::log_event(::diagnostics::events::SkillDispatched {
                skill_name: sk.name.clone(),
                plugin_source: sk.plugin_name.clone(),
            });
        }
        slash_commands::build_skill_information_for_refs(
            &parsed,
            &slash_skills,
            &self.session_id_string(),
        )
        .await
    }

    /// Drain all pending interjections, wrap them, and inject each into the
    /// ([`ConversationItem::interjection`], tagged
    /// `SyntheticReason::Interjection`) — never appended to tool results, so
    /// compaction, replay, and analytics see the user's steering text as its
    /// own user turn.
    ///
    /// Returns `true` if any interjections were drained (caller may want to
    /// Returns `true` if any interjections were drained (caller may want to
    /// `continue` the turn loop so the model sees them on the next iteration).
    pub(super) async fn drain_pending_interjections(&self) -> bool {
        // Manual drain (not `drain_formatted`): skill parsing needs the raw
        // text — parsed post-wrap, the envelope's closing `</user_query>` tag
        // would pollute the trailing skill's args.
        let entries = self.pending_interjections.drain_all();
        if entries.is_empty() {
            return false;
        }

        for PendingInterjection { text, attachments } in entries {
            // Sanitizer drops `[Image #N: <path>]` → `[Image #N]` before the
            // text reaches the model, covering legacy-client raw text AND the
            // queue-interject harvest. Wrapping and truncation stay in the
            // shared crate (`format_interjection`).
            let sanitized =
                crate::session::placeholder_images::strip_paths_from_image_placeholders(text);
            let skill_information = self.interjection_skill_information(&sanitized).await;
            let mut wrapped = format_interjection(sanitized);
            let images = self
                .prepare_interjection_images(&mut wrapped, attachments)
                .await;
            // Model-visible text: <skill_information> follows the wrapped
            // <user_query> — same order as turn-start prompt assembly, and
            // appended after the image pipeline so the template-specific
            // transcription rewrite cannot mangle the envelope. The
            // persisted user chunk stays envelope-free so session replay
            // renders the compact interjection, not the SKILL.md body
            // (mirrors turn-start skills, which replay via `displayText`).
            let model_text = match &skill_information {
                Some(skill_information) => {
                    tracing::info!("expanded skill references in mid-turn interjection");
                    format!("{wrapped}\n{skill_information}")
                }
                None => wrapped.clone(),
            };
            let mut item = ConversationItem::interjection(model_text);
            for img in &images {
                item.add_image(pick_user_image_url(img));
            }
            self.inject_synthetic_user_message(&wrapped, item, false, &images)
                .await;
            tracing::info!("Injected mid-turn interjection as standalone synthetic user message");
        }
        // An interjection never cancels the turn, so it leaves no marker on the
        // next user turn (that field is reserved for fatal aborts). The
        // interjection itself is recorded at enqueue time via
        // `Event::Interjected` (carrying the shared `redirect_kind`).
        true
    }
}
