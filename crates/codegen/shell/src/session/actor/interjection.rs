//! Mid-turn interjection concern for `SessionActor` (buffer type, formatting,
//! broadcast, drain). Also hosts `inject_synthetic_user_message`, the shared
//! synthetic-user-message injector the permission-panel followup path reuses.

use super::*;

// Buffer, entry type, and formatting live with the tool execution layer so
// shell and tool loops share semantics. The shell keeps arrival (ACP ext
// methods), persistence, and pager echo.
//
// Re-exported by the actor module so retained code and co-located tests resolve
// through the actor's private namespace.
pub(crate) use tools::interjection::format_interjection;

/// Requeue payload for accepted user input: the exact fields needed to rebuild
/// a fresh [`InputItem`] when an authority/budget/cancel terminal prevents the
/// named turn from consuming it. Queue-promoted input preserves its original
/// identity; direct steering receives a new fallback prompt identity.
#[derive(Debug, Clone)]
pub(crate) struct ResidualInterjectionRequeue {
    pub input_ids: Vec<String>,
    pub prompt_id: String,
    pub origin: crate::session::PromptOrigin,
    pub turn_kind: crate::session::TurnKind,
    pub client_identifier: Option<String>,
    pub screen_mode: Option<String>,
    pub verbatim: bool,
    pub json_schema: Option<serde_json::Value>,
}

/// Shell instantiation of the shared pending-interjection entry. `requeue`
/// is `Some` for every production user steer. `None` is reserved for internal
/// synthetic/test entries that may be discarded at a terminal fence.
#[derive(Debug, Clone)]
pub(crate) struct PendingInterjection<Attachment> {
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub requeue: Option<ResidualInterjectionRequeue>,
}

/// Same-turn steering buffer: one buffer, one safe-point drain, one terminal
/// fence for both explicit steers and auto-promoted follow-ups.
pub(crate) type InterjectionBuffer<Attachment> =
    tools::interjection::EventQueue<PendingInterjection<Attachment>>;

impl SessionActor {
    pub(super) async fn admit_human_steer(
        &self,
        expected_turn_id: &str,
        text: String,
        images: Vec<acp::ImageContent>,
        interjection_id: Option<String>,
    ) -> Result<(), String> {
        crate::session::input_inbox::validate_image_sizes(&images)
            .map_err(|error| error.to_string())?;
        let prompt_id = interjection_id
            .clone()
            .unwrap_or_else(|| format!("steer-{}", uuid::Uuid::now_v7()));
        let _admission_guard = self.input_admission_gate.lock().await;
        // Capture the server-minted durable identity before the potentially
        // slow Hook. The client prompt id is presentation identity and may be
        // reused; it cannot by itself prevent an ABA target replacement.
        let captured_turn = {
            let _control_gate = self.step_control_gate.lock().await;
            let state = self.state.lock().await;
            state
                .foreground
                .regular()
                .filter(|task| {
                    task.prompt_id == expected_turn_id && !task.is_finished() && task.steering_open
                })
                .and_then(|_| self.events.current_turn())
        };
        let mut blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(text.clone()))];
        blocks.extend(images.iter().cloned().map(acp::ContentBlock::Image));
        let payload = crate::session::input_inbox::InputPayload::Prompt {
            prompt_id: prompt_id.clone(),
            requested_route: crate::session::input_inbox::InputRequestRoute::Steer {
                expected_turn_id: expected_turn_id.to_owned(),
            },
            prompt_blocks: blocks,
            skill_invocations: Vec::new(),
            client_identifier: None,
            screen_mode: None,
            verbatim: false,
            json_schema: None,
            origin: crate::session::PromptOrigin::User,
            turn_kind: crate::session::TurnKind::User,
            queue: None,
        };
        let attempt = self
            .run_human_input_admission_hook(
                chat_state::InputIntent::Steer,
                payload,
                Some(prompt_id.clone()),
            )
            .await?;
        let (input_id, hook_decision) = match attempt {
            super::input_admission::InputAdmissionAttempt::New { input_id, decision } => {
                (input_id, decision)
            }
            super::input_admission::InputAdmissionAttempt::Existing { input_id, state } => {
                return match state {
                    super::input_admission::ExistingInputState::Allowed => Ok(()),
                    super::input_admission::ExistingInputState::Blocked(reason) => {
                        Err(super::input_admission::input_block_reason_message(reason))
                    }
                    super::input_admission::ExistingInputState::Submitted => Err(format!(
                        "input admission remains unresolved for prompt {prompt_id} (input {input_id})"
                    )),
                    super::input_admission::ExistingInputState::Consumed
                    | super::input_admission::ExistingInputState::Handled
                    | super::input_admission::ExistingInputState::Dismissed => Err(format!(
                        "input is already terminal for prompt {prompt_id} (input {input_id})"
                    )),
                };
            }
        };
        if !matches!(&hook_decision, chat_state::InputAdmissionDecision::Allow) {
            return self
                .resolve_human_input_admission(input_id, hook_decision, None, Vec::new())
                .await
                .map(|_| ());
        }

        let _control_gate = self.step_control_gate.lock().await;
        let target_turn = {
            let state = self.state.lock().await;
            captured_turn.filter(|captured_turn| {
                state.foreground.regular().is_some_and(|task| {
                    task.prompt_id == expected_turn_id && !task.is_finished() && task.steering_open
                }) && self.events.current_turn() == Some(*captured_turn)
            })
        };
        let Some(target_turn) = target_turn else {
            return self
                .resolve_human_input_admission(
                    input_id,
                    chat_state::InputAdmissionDecision::Block {
                        reason: chat_state::InputBlockReason::StaleSteerTarget,
                    },
                    None,
                    Vec::new(),
                )
                .await
                .map(|_| ());
        };
        let admitted = self
            .resolve_human_input_admission(
                input_id,
                hook_decision,
                Some(chat_state::InputRoute::Steer { target_turn }),
                Vec::new(),
            )
            .await?;
        let requeue = ResidualInterjectionRequeue {
            input_ids: vec![admitted.input_id.clone()],
            prompt_id,
            origin: crate::session::PromptOrigin::User,
            turn_kind: crate::session::TurnKind::User,
            client_identifier: None,
            screen_mode: None,
            verbatim: false,
            json_schema: None,
        };
        {
            let state = self.state.lock().await;
            let exact = state.foreground.regular().is_some_and(|task| {
                task.prompt_id == expected_turn_id
                    && !task.is_finished()
                    && task.steering_open
                    && self.events.current_turn() == Some(target_turn)
            });
            if !exact {
                tracing::error!(
                    expected_turn_id,
                    ?target_turn,
                    "steer target changed behind the control fence"
                );
                return Err("the target turn changed behind its control fence".into());
            }
            self.pending_interjections.push(PendingInterjection {
                text,
                attachments: images,
                requeue: Some(requeue),
            });
        }
        Ok(())
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
        requeue: ResidualInterjectionRequeue,
    ) {
        self.pending_interjections.push(PendingInterjection {
            text,
            attachments,
            requeue: Some(requeue),
        });
    }

    /// Close the current turn's steering scope.
    ///
    /// The buffer itself is not cross-turn context. Accepted user entries that
    /// could not be consumed are upgraded to fresh FIFO turns; only explicitly
    /// non-requeueable internal entries are discarded. This preserves exact
    /// turn conditioning without ever acknowledging and losing user input.
    pub(super) async fn discard_residual_interjections_at_turn_end(&self) {
        let drained = self.pending_interjections.drain_all();
        if drained.is_empty() {
            return;
        }
        let mut discarded = 0usize;
        let mut to_requeue: Vec<PendingInterjection<acp::ImageContent>> = Vec::new();
        for entry in drained {
            if entry.requeue.is_some() {
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
            let mut durable = Vec::with_capacity(to_requeue.len());
            for entry in to_requeue {
                let mut rerouted = true;
                if let Some(requeue) = &entry.requeue {
                    if let Err(error) = self
                        .reroute_input_ids(requeue.input_ids.clone(), chat_state::InputRoute::Fifo)
                        .await
                    {
                        tracing::error!(%error, input_ids = ?requeue.input_ids, "residual steer reroute was not durable");
                        rerouted = false;
                    }
                }
                if rerouted {
                    durable.push(entry);
                }
            }
            let mut state = self.state.lock().await;
            // Reverse so the oldest drained entry lands at the FIFO front.
            for entry in durable.into_iter().rev() {
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
        let requeue = entry
            .requeue
            .expect("only requeueable entries reach the requeue path");
        let mut prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            entry.text.clone(),
        ))];
        prompt_blocks.extend(entry.attachments.into_iter().map(acp::ContentBlock::Image));
        let owner = requeue.client_identifier.clone();
        let (respond_to, _completion_rx) = tokio::sync::oneshot::channel();
        InputItem {
            input_ids: requeue.input_ids,
            prompt_id: requeue.prompt_id.clone(),
            turn_kind: requeue.turn_kind,
            prompt_blocks,
            client_identifier: requeue.client_identifier,
            screen_mode: requeue.screen_mode,
            verbatim: requeue.verbatim,
            json_schema: requeue.json_schema,
            origin: requeue.origin,
            host_command: None,
            notification_ids: Vec::new(),
            respond_to,
            persist_ack: None,
            queue_meta: Some(crate::session::prompt_queue::QueueEntryMeta {
                id: requeue.prompt_id,
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
        self.publish_synthetic_user_message(text, item, notify_pager, images, true, None)
            .await;
    }

    async fn publish_synthetic_user_message(
        &self,
        text: &str,
        item: ConversationItem,
        notify_pager: bool,
        images: &[acp::ImageContent],
        append_to_chat: bool,
        display_echo: Option<&chat_state::UserDisplayEcho>,
    ) {
        let model_id = self.current_catalog_model_id();
        let permission_evidence = match &item {
            ConversationItem::User(user) => user.permission_evidence.clone(),
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
        if let Some(display_echo) = display_echo {
            user_chunk_meta.insert(
                "messageId".into(),
                serde_json::json!(display_echo.message_id),
            );
            user_chunk_meta.insert(
                "promptIndex".into(),
                serde_json::json!(display_echo.prompt_index),
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
        if append_to_chat {
            self.chat_state_handle.push_user_message(item);
        }
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
        if self.events.has_active_tool() {
            return false;
        }
        // Manual drain keeps each entry paired with its admitted input
        // identity while the shared Runtime loader reads immutable snapshots.
        let entries = self.pending_interjections.drain_all();
        if entries.is_empty() {
            return false;
        }

        self.inject_pending_interjections(entries).await;
        true
    }

    async fn inject_pending_interjections(
        &self,
        entries: Vec<PendingInterjection<acp::ImageContent>>,
    ) {
        if self.events.has_active_tool() {
            for entry in entries.into_iter().rev() {
                self.pending_interjections.push(entry);
            }
            return;
        }
        for PendingInterjection {
            text,
            attachments,
            requeue,
        } in entries
        {
            let retry = PendingInterjection {
                text: text.clone(),
                attachments: attachments.clone(),
                requeue: requeue.clone(),
            };
            // Sanitizer drops `[Image #N: <path>]` → `[Image #N]` before the
            // text reaches the model, covering legacy-client raw text AND the
            // queue-interject harvest. Wrapping and truncation stay in the
            // shared crate (`format_interjection`).
            let sanitized =
                crate::session::placeholder_images::strip_paths_from_image_placeholders(text);
            let display_text = sanitized.clone();
            let mut wrapped = format_interjection(sanitized);
            let images = self
                .prepare_interjection_images(&mut wrapped, attachments)
                .await;
            let mut item = ConversationItem::interjection(wrapped.clone(), display_text.clone());
            for img in &images {
                item.add_image(pick_user_image_url(img));
            }
            let input_ids = requeue
                .as_ref()
                .map(|requeue| requeue.input_ids.as_slice())
                .unwrap_or_default();
            let mut injected = false;
            if input_ids.is_empty() {
                self.inject_synthetic_user_message(&display_text, item, false, &images)
                    .await;
                injected = true;
            } else {
                let Some(turn) = self.events.current_turn() else {
                    tracing::error!("accepted steer lost its durable Turn before consumption");
                    continue;
                };

                if self.events.has_active_tool() {
                    self.pending_interjections.push(retry);
                    continue;
                }

                let prompt_index = self.chat_state_handle.current_prompt_index().await;
                let Some(prompt_index) = prompt_index else {
                    self.fail_steer_skill_load(
                        input_ids,
                        turn,
                        "The current prompt identity was unavailable for skill preparation.",
                    )
                    .await;
                    continue;
                };
                item.set_prompt_index(prompt_index);
                let group = match self.prepare_user_skill_loads(input_ids, prompt_index).await {
                    Ok(group) => group,
                    Err(error) => {
                        self.fail_steer_skill_load(input_ids, turn, &error.to_string())
                            .await;
                        continue;
                    }
                };
                let mut display_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
                    display_text.clone(),
                ))];
                display_blocks.extend(images.iter().cloned().map(acp::ContentBlock::Image));
                let input_identity = serde_json::to_vec(input_ids)
                    .expect("admitted input identities serialize to JSON");
                let message_id = format!("steer-{}", blake3::hash(&input_identity).to_hex());
                let display_echo =
                    super::user_display_echo(message_id, prompt_index, &display_blocks);

                let publication = if group.items.is_empty() {
                    self.consume_steer_inputs(
                        input_ids.to_vec(),
                        turn,
                        item.clone(),
                        Some(display_echo.clone()),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|error| (false, error.to_string()))
                } else {
                    let revision = self.chat_state_handle.get_surface_revision().await;
                    let Some(revision) = revision else {
                        let _ = self.finish_user_skill_loads(&group, false).await;
                        self.fail_steer_skill_load(
                            input_ids,
                            turn,
                            "The conversation surface revision was unavailable.",
                        )
                        .await;
                        continue;
                    };
                    let window = self
                        .chat_state_handle
                        .get_sampling_config()
                        .await
                        .map(|config| config.context_window.get())
                        .unwrap_or(0);
                    match self
                        .chat_state_handle
                        .consume_input_group_conditionally(
                            input_ids.to_vec(),
                            turn,
                            item.clone(),
                            Some(display_echo.clone()),
                            group.items.clone(),
                            revision,
                            super::context_recall::context_recall_max_context_tokens(window),
                        )
                        .await
                    {
                        Ok(chat_state::ConditionalToolResultOutcome::Accepted) => Ok(()),
                        Ok(outcome) => Err((
                            true,
                            format!("skill group publication was rejected: {outcome:?}"),
                        )),
                        Err(error) => {
                            Err((false, format!("skill group publication failed: {error}")))
                        }
                    }
                };

                match publication {
                    Ok(()) => {
                        if !group.items.is_empty() {
                            if let Err(error) = self.finish_user_skill_loads(&group, true).await {
                                tracing::error!(%error, ?input_ids, "published steer skill loads could not be finalized");
                                continue;
                            }
                        }
                        self.publish_synthetic_user_message(
                            &display_text,
                            item,
                            false,
                            &images,
                            false,
                            Some(&display_echo),
                        )
                        .await;
                        injected = true;
                    }
                    Err((rejected, error)) => {
                        if rejected {
                            let _ = self.finish_user_skill_loads(&group, false).await;
                            self.fail_steer_skill_load(input_ids, turn, &error).await;
                        } else {
                            tracing::error!(%error, ?input_ids, "steer consumption was not durable");
                        }
                    }
                }
            }
            if injected {
                tracing::info!(
                    "Injected mid-turn interjection as standalone synthetic user message"
                );
            }
        }
        // An interjection never cancels the turn, so it leaves no marker on the
        // next user turn (that field is reserved for fatal aborts). The
        // interjection itself is recorded at enqueue time via
        // `Event::Interjected` (carrying the shared `redirect_kind`).
    }

    async fn fail_steer_skill_load(
        &self,
        input_ids: &[String],
        turn: chat_state::TurnId,
        error: &str,
    ) {
        let settlement = self
            .complete_unmodeled_fifo_inputs(input_ids.to_vec(), turn)
            .await
            .map_err(|settle_error| {
                tracing::error!(%settle_error, ?input_ids, "failed steer could not be settled");
                settle_error
            });
        let details = settlement
            .err()
            .map(|settle_error| format!("{error}\nInput settlement also failed: {settle_error}"))
            .unwrap_or_else(|| error.to_string());
        if let Err(notice_error) = self.persist_ui_notice(crate::extensions::notification::UiNotice {
            correlation_id: input_ids.join(","),
            category: crate::extensions::notification::UiNoticeCategory::Lifecycle,
            subject: Some("steering skill".into()),
            description: None,
            message: "Your steering message was not applied because its requested skill could not be loaded. The current work continues.".into(),
            tone: crate::extensions::notification::UiNoticeTone::Error,
            details: Some(details),
        }).await {
            tracing::warn!(%notice_error, ?input_ids, "failed to publish steer skill failure notice");
        }
    }

    /// Linearize final-response settlement against `SteerTurn` admission.
    /// Both sides hold `state`: an accepted steer is queued before this fence
    /// and injected into a successor step, or observes the closed gate and is
    /// rejected. It cannot be acknowledged behind the final drain.
    pub(super) async fn close_steering_and_drain(&self, prompt_id: &str) -> bool {
        let control_gate = self.step_control_gate.lock().await;
        let entries = {
            let mut state = self.state.lock().await;
            let Some(task) = state.foreground.regular_mut() else {
                return false;
            };
            if task.prompt_id != prompt_id {
                return false;
            }
            task.steering_open = false;
            if self.events.has_active_tool() {
                Vec::new()
            } else {
                self.pending_interjections.drain_all()
            }
        };
        drop(control_gate);
        let had_interjections = !entries.is_empty();
        if had_interjections {
            self.inject_pending_interjections(entries).await;
        }
        let has_parent_message = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default()
            .iter()
            .any(|item| {
                matches!(
                    &item.source,
                    chat_state::NotificationSource::ParentMessage { .. }
                        | chat_state::NotificationSource::AgentReply { .. }
                )
            });
        let delivered_parent_message =
            has_parent_message && self.drain_active_notifications().await;
        had_interjections || delivered_parent_message
    }

    pub(super) async fn reopen_steering(&self, prompt_id: &str) {
        let mut state = self.state.lock().await;
        if let Some(task) = state.foreground.regular_mut()
            && task.prompt_id == prompt_id
        {
            task.steering_open = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::actor::tests::support::{
        begin_test_causal_turn, build_actor, install_test_foreground,
    };

    #[tokio::test(flavor = "current_thread")]
    async fn interjection_replay_preserves_user_text_and_model_envelope() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for accepted in [false, true] {
                    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                    let (persistence_tx, mut persistence_rx) =
                        tokio::sync::mpsc::unbounded_channel();
                    let (actor, _events) = super::super::tests::support::create_test_actor_ex(
                        0,
                        100_000,
                        80,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await;
                    begin_test_causal_turn(&actor).await;
                    install_test_foreground(&actor, "turn-1").await;
                    let text = "用户原文\n<user_query>literal user markup</user_query>";
                    if accepted {
                        actor
                            .admit_human_steer(
                                "turn-1",
                                text.into(),
                                Vec::new(),
                                Some("steer-display".into()),
                            )
                            .await
                            .unwrap();
                        assert!(actor.close_steering_and_drain("turn-1").await);
                    } else {
                        actor
                            .inject_pending_interjections(vec![PendingInterjection {
                                text: text.into(),
                                attachments: Vec::new(),
                                requeue: None,
                            }])
                            .await;
                    }
                    let notification =
                        tokio::time::timeout(std::time::Duration::from_secs(2), async {
                            loop {
                                if let Some(PersistenceMsg::Update(SessionUpdate::Acp(
                                    notification,
                                ))) = persistence_rx.recv().await
                                    && matches!(
                                        notification.update,
                                        acp::SessionUpdate::UserMessageChunk(_)
                                    )
                                {
                                    break notification;
                                }
                            }
                        })
                        .await
                        .expect("persisted user display update");
                    let encoded = serde_json::to_string(&notification).unwrap();
                    let replayed: acp::SessionNotification =
                        serde_json::from_str(&encoded).unwrap();
                    let acp::SessionUpdate::UserMessageChunk(chunk) = replayed.update else {
                        panic!("user chunk")
                    };
                    let acp::ContentBlock::Text(display) = chunk.content else {
                        panic!("text block")
                    };
                    assert_eq!(
                        display.text, text,
                        "resume must preserve user-authored text"
                    );
                    let conversation = actor.chat_state_handle.get_conversation().await;
                    assert!(
                        conversation
                            .iter()
                            .any(|item| item.text_content() == format_interjection(text.into())),
                        "model context must retain its runtime envelope"
                    );
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn final_steering_fence_injects_accepted_input_and_rejects_late_input() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = build_actor().await;
                begin_test_causal_turn(&actor).await;
                install_test_foreground(&actor, "turn-1").await;

                actor
                    .admit_human_steer(
                        "turn-1",
                        "accepted".into(),
                        Vec::new(),
                        Some("steer-accepted".into()),
                    )
                    .await
                    .unwrap();
                assert!(actor.close_steering_and_drain("turn-1").await);
                assert!(
                    actor
                        .admit_human_steer(
                            "turn-1",
                            "too late".into(),
                            Vec::new(),
                            Some("steer-late".into()),
                        )
                        .await
                        .is_err()
                );
                assert!(actor.pending_interjections.is_empty());
                let conversation = actor.chat_state_handle.get_conversation().await;
                assert!(
                    serde_json::to_string(&conversation)
                        .unwrap()
                        .contains("accepted")
                );
            })
            .await;
    }
}
