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
pub(crate) use tools::interjection::format_interjection;

/// Requeue payload for an auto-promoted follow-up: the exact fields needed to
/// rebuild a fresh [`InputItem`] when the turn terminal turns a residual
/// auto-promoted entry back into the user FIFO (as a brand-new turn — the
/// original prompt id, origin, and turn kind are preserved). Explicit steers
/// carry `None`; their residuals are discarded at the same fence (turn
/// identity: an explicit steer belongs to the exact turn it named).
#[derive(Debug, Clone)]
pub(crate) struct AutoPromotedRequeue {
    pub prompt_id: String,
    pub origin: crate::session::PromptOrigin,
    pub turn_kind: crate::session::TurnKind,
    pub client_identifier: Option<String>,
    pub screen_mode: Option<String>,
    pub verbatim: bool,
    pub json_schema: Option<serde_json::Value>,
}

/// Shell instantiation of the shared pending-interjection entry. `auto_promoted`
/// is `Some` only for plain-Enter QueuePrompts admitted under
/// `follow_up_behavior = "steer"`; explicit steers (Ctrl+Enter / queue
/// "Send now") carry `None`.
#[derive(Debug, Clone)]
pub(crate) struct PendingInterjection<Attachment> {
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub auto_promoted: Option<AutoPromotedRequeue>,
}

/// Same-turn steering buffer: one buffer, one safe-point drain, one terminal
/// fence for both explicit steers and auto-promoted follow-ups.
pub(crate) type InterjectionBuffer<Attachment> =
    tools::interjection::EventQueue<PendingInterjection<Attachment>>;

impl SessionActor {
    /// Common same-turn steering buffer shared by direct and queued input.
    /// Goal planner/verifier leases are intentionally independent: ordinary
    /// supplemental user input does not revise the Goal definition or plan.
    pub(super) fn queue_mid_turn_interjection(
        &self,
        text: String,
        attachments: Vec<acp::ImageContent>,
    ) {
        self.pending_interjections.push(PendingInterjection {
            text,
            attachments,
            auto_promoted: None,
        });
    }

    /// Enqueue a plain-Enter follow-up that [`SessionActor::queue_input`]
    /// auto-promoted into the running regular turn (`follow_up_behavior =
    /// "steer"`). Same buffer, drain, and terminal fence as explicit steers;
    /// the requeue payload lets the turn-end fence turn a residual entry back
    /// into the user FIFO as a fresh turn instead of discarding it.
    pub(super) fn queue_auto_promoted_follow_up(
        &self,
        text: String,
        attachments: Vec<acp::ImageContent>,
        requeue: AutoPromotedRequeue,
    ) {
        self.pending_interjections.push(PendingInterjection {
            text,
            attachments,
            auto_promoted: Some(requeue),
        });
    }

    /// Close the current turn's steering scope.
    ///
    /// The buffer is deliberately not a cross-turn queue: every entry was
    /// admitted against an exact running turn id. Explicit-steer residuals
    /// (Ctrl+Enter / "Send now") are discarded — carrying them into the next
    /// foreground owner would violate turn identity. Auto-promoted follow-ups
    /// (`follow_up_behavior = "steer"`) are different: they were admitted as
    /// queue prompts, so a residual is turned back into the user FIFO front as
    /// a fresh turn (original prompt id / origin / turn kind preserved) before
    /// the next promotion — no user input is silently swallowed.
    pub(super) async fn discard_residual_interjections_at_turn_end(&self) {
        let drained = self.pending_interjections.drain_all();
        if drained.is_empty() {
            return;
        }
        let mut discarded = 0usize;
        let mut to_requeue: Vec<PendingInterjection<acp::ImageContent>> = Vec::new();
        for entry in drained {
            if entry.auto_promoted.is_some() {
                to_requeue.push(entry);
            } else {
                discarded += 1;
            }
        }
        if discarded > 0 {
            tracing::debug!(
                discarded,
                "discarded residual same-turn steering at terminal boundary"
            );
        }
        if !to_requeue.is_empty() {
            let requeued = to_requeue.len();
            let mut state = self.state.lock().await;
            // Reverse so the oldest drained entry lands at the FIFO front.
            for entry in to_requeue.into_iter().rev() {
                state
                    .pending_inputs
                    .push_front(Self::requeue_auto_promoted(entry));
            }
            tracing::info!(
                requeued,
                "re-queued auto-promoted follow-ups as fresh turns at the terminal boundary"
            );
            self.broadcast_queue_changed(&state);
        }
    }

    /// Rebuild a fresh FIFO [`InputItem`] from a residual auto-promoted entry.
    /// Runs under the state lock at the terminal fence, before any promotion,
    /// so the FIFO head is settled before clients can observe a new owner.
    fn requeue_auto_promoted(entry: PendingInterjection<acp::ImageContent>) -> InputItem {
        let auto = entry
            .auto_promoted
            .expect("only auto-promoted entries reach the requeue path");
        let mut prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            entry.text.clone(),
        ))];
        prompt_blocks.extend(entry.attachments.into_iter().map(acp::ContentBlock::Image));
        let owner = auto.client_identifier.clone();
        let (respond_to, _completion_rx) = tokio::sync::oneshot::channel();
        InputItem {
            prompt_id: auto.prompt_id.clone(),
            turn_kind: auto.turn_kind,
            prompt_blocks,
            client_identifier: auto.client_identifier,
            screen_mode: auto.screen_mode,
            verbatim: auto.verbatim,
            json_schema: auto.json_schema,
            origin: auto.origin,
            task_wake_fallback: None,
            respond_to,
            persist_ack: None,
            queue_meta: Some(crate::session::prompt_queue::QueueEntryMeta {
                id: auto.prompt_id,
                version: 0,
                owner,
                last_editor: None,
                kind: "prompt".to_string(),
                text: entry.text,
                combined_texts: None,
            }),
        }
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
        self.normalize_images_with_notices(wrapped, images).await
    }

    /// Broadcast a mid-turn interjection to every attached client.
    ///
    /// Fan it out (sessionId-routed, fire-and-forget) so every pane viewing the
    /// session renders the interjection block, not just the originator. The
    /// originating pager deduplicates its optimistic block by `id`; other
    /// panes render it. `None` remains valid for older clients.
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

    /// Inject a synthetic user message into persistence and conversation
    /// context, optionally notifying the pager. Interjection drains skip the
    /// notification because the pager already owns the optimistic user row.
    pub(super) async fn inject_synthetic_user_message(
        &self,
        text: &str,
        item: ConversationItem,
        notify_pager: bool,
        images: &[acp::ImageContent],
    ) {
        let model_id = self.current_model_id().await;
        let permission_evidence = match &item {
            ConversationItem::User(user) => user.permission_evidence,
            _ => None,
        };
        let mut user_chunk_meta = serde_json::json!({ "modelId": model_id })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(evidence) = permission_evidence {
            user_chunk_meta.insert(
                "permissionEvidence".into(),
                serde_json::to_value(evidence).expect("permission evidence serializes"),
            );
        }
        let user_chunk_meta = Some(user_chunk_meta);

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

    /// Expand skill slash references in interjection text.
    ///
    /// Interjections bypass turn-start slash resolution
    /// (`slash_commands::resolve`), so without this a queued `/skill` row
    /// force-sent mid-turn — or a typed `/skill` interjection — reaches the
    /// model as a bare, unexpanded slash command. Returns `None` when the text
    /// references no known skill.
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

    /// Drain all pending interjections, wrap them, and inject each as a
    /// [`ConversationItem::interjection`] tagged
    /// `SyntheticReason::Interjection`) — never appended to tool results, so
    /// compaction, replay, and analytics see the user's steering text as its
    /// own user turn.
    ///
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

        for PendingInterjection {
            text, attachments, ..
        } in entries
        {
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
