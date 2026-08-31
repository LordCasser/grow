//! Turn admission concern for `SessionActor`: prompt intake, routing, and user echoes.
use super::*;
use crate::session::TurnKind;
use crate::session::behavior::BehaviorChangeOutcome;

/// Successful execution behind one durably admitted turn.
///
/// Host routes already produce the public prompt response, while the model
/// route retains its richer internal outcome until post-turn bookkeeping is
/// complete.  Both variants deliberately share the same outer Timeline
/// terminal owner in `handle_prompt`.
enum AdmittedTurnSuccess {
    Host(crate::session::commands::PromptTurnOk),
    Model(TurnOutcome),
}

pub(in crate::session::actor) fn should_capture_implicit_goal_objective(
    origin: &crate::session::PromptOrigin,
    goal_behavior_selected: bool,
    goal_status: Option<crate::session::goal_tracker::GoalStatus>,
    text: &str,
) -> bool {
    matches!(origin, crate::session::PromptOrigin::User)
        && goal_behavior_selected
        && goal_status.is_none()
        && !text.trim_start().starts_with('/')
        && !text.trim().is_empty()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserEchoMode {
    /// Live + persist (real user / cron / skill turns).
    Broadcast,
    /// Persist without live broadcast. Interject-fallback: panes already
    /// rendered the text, so a live echo would duplicate it. Notification
    /// drain: model-only content (the UI surfaces it via side channels:
    /// monitor gutter, task pane) that no pane should render live.
    PersistOnly,
}
pub(super) fn user_echo_mode(origin: &super::super::PromptOrigin) -> UserEchoMode {
    match origin {
        super::super::PromptOrigin::NotificationDrain => UserEchoMode::PersistOnly,
        _ => UserEchoMode::Broadcast,
    }
}
impl SessionActor {
    /// Run the image-normalization pipeline (re-encode caps, min-side and
    /// integrity checks) and surface its outcomes: compression / re-encode
    /// fallback / dropped notices are appended to `text_out` (TEXT only —
    /// image data never enters a string) and mirrored as
    /// `ImageCompressed`/`ImageDropped` notifications. Returns the surviving
    /// images. Single owner of the notice/notify wiring, shared by the
    /// prompt path and the interjection drain.
    pub(crate) async fn normalize_images_with_notices(
        &self,
        text_out: &mut String,
        images: Vec<acp::ImageContent>,
    ) -> Vec<acp::ImageContent> {
        let mut norm_result = crate::session::image_normalize::normalize_images(images).await;
        let user_images = std::mem::take(&mut norm_result.images);
        use crate::extensions::notification::ImageCompressedEntry;
        if !norm_result.compressed.is_empty() {
            text_out.push_str(&crate::session::image_normalize::render_compression_notice(
                &norm_result.compressed,
            ));
            let message = norm_result
                .compressed
                .iter()
                .map(|c| c.display())
                .collect::<Vec<_>>()
                .join("; ");
            let images = norm_result
                .compressed
                .iter()
                .map(ImageCompressedEntry::from)
                .collect();
            self.send_grow_notification(GrowSessionUpdate::ImageCompressed { images, message })
                .await;
        }
        if !norm_result.re_encode_fallbacks.is_empty() {
            text_out.push_str(
                &crate::session::image_normalize::render_re_encode_fallback_notice(
                    &norm_result.re_encode_fallbacks,
                ),
            );
            self.send_grow_notification(GrowSessionUpdate::ImageCompressed {
                images: vec![],
                message: norm_result.re_encode_fallbacks.join(" "),
            })
            .await;
        }
        if let Some((notice, notes)) = crate::session::image_normalize::dropped_to_envelope(
            std::mem::take(&mut norm_result.dropped),
        ) {
            text_out.push_str(&notice);
            self.send_grow_notification(GrowSessionUpdate::ImageDropped { notes })
                .await;
        }
        user_images
    }
    pub(super) fn persist_host_turn_user_echo(&self, text: &str, _prompt_id: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let mut chunk_meta = serde_json::Map::new();
        chunk_meta.insert(
            crate::session::storage::HOST_TURN_META_KEY.into(),
            serde_json::json!(true),
        );
        chunk_meta.insert("hideFromScrollback".into(), serde_json::json!(true));
        let update = acp::SessionUpdate::UserMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                text.to_string(),
            )))
            .meta(Some(chunk_meta)),
        );
        let notification_meta = self.build_notification_meta();
        let notification = acp::SessionNotification::new(self.session_info.id.clone(), update)
            .meta(notification_meta.as_object().cloned());
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Acp(Box::new(notification)),
            ));
    }
    #[tracing::instrument(
        name = "session.handle_prompt",
        skip_all,
        fields(
            session_id = %self.session_info.id.0,
            prompt_id = %prompt_id,
            prompt_length = tracing::field::Empty,
            command_name = tracing::field::Empty,
            command_source = tracing::field::Empty,
        )
    )]
    pub(in crate::session::actor) async fn handle_prompt(
        self: &Arc<Self>,
        prompt_id: &str,
        origin: super::super::PromptOrigin,
        notification_ids: Vec<String>,
        turn_kind: TurnKind,
        mut prompt_blocks: Vec<acp::ContentBlock>,
        admitted_behavior: tool_types::BehaviorId,
        prompt_client_identifier: Option<String>,
        prompt_screen_mode: Option<String>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        persist_ack: Option<oneshot::Sender<()>>,
    ) -> PromptTurnResult {
        let handle_prompt_start = std::time::Instant::now();
        let prompt_length: usize = prompt_blocks
            .iter()
            .map(|b| match b {
                acp::ContentBlock::Text(t) => t.text.len(),
                _ => 0,
            })
            .sum();
        tracing::Span::current().record("prompt_length", prompt_length as i64);
        *self.active_skill.lock() = None;
        ::diagnostics::unified_log::info(
            "shell.handle_prompt.start",
            Some(self.session_info.id.0.as_ref()),
            Some(serde_json::json!({
                "prompt_id": prompt_id,
                "block_count": prompt_blocks.len(),
            })),
        );
        // `QueuePrompt` performs the same check before external input is
        // admitted, but autonomous producers (most notably the first Goal
        // continuation in a fresh session) construct `AgentTask` directly.
        // Keep this final barrier at the common runner boundary: no prompt
        // may make `ContextRebuild` historical before the deferred stable
        // prefix has been committed.
        // Box the bootstrap future so this already-large turn runner does not
        // copy its context-building state into every `handle_prompt` future.
        if let Err(error) = Box::pin(self.ensure_prefix_ready()).await {
            super::super::tasks_cancel::signal_durable_turn_start(false);
            if let Some(extension) = &self.idle_prompt_extension {
                extension.on_turn_failed();
            }
            return Err(crate::session::commands::fatal_turn_boundary_error(
                "bootstrap",
                format!("session context was not durably published: {error}"),
            ));
        }
        let admitted_notification_task_ids = if notification_ids.is_empty() {
            Vec::new()
        } else {
            self.chat_state_handle
                .pending_notifications()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|notification| notification_ids.contains(&notification.id))
                .filter_map(|notification| match notification.source {
                    chat_state::NotificationSource::MonitorProgress { .. }
                    | chat_state::NotificationSource::TaskStillRunning { .. }
                    | chat_state::NotificationSource::WorkflowCompleted { .. } => None,
                    chat_state::NotificationSource::TaskCompleted { task_id, .. } => Some(task_id),
                    chat_state::NotificationSource::SubagentCompleted { subagent_id, .. } => {
                        Some(subagent_id)
                    }
                })
                .collect::<Vec<_>>()
        };
        if !origin.is_synthetic() {
            self.cancel_pending_recap_for_new_prompt();
        }
        // `admitted_behavior` is captured atomically by the idle admission path.
        // Prompt text and queued metadata never drive Behavior transitions.
        *self.turn_behavior.lock() = admitted_behavior;
        self.signals_handle().increment_turn();
        let _turn_active_guard =
            TurnActiveGuard::activate(self.tool_context.is_turn_active.as_ref());
        let _session_turn_active_guard = TurnActiveGuard::activate(Some(&self.session_turn_active));
        if let Some(extension) = &self.idle_prompt_extension {
            extension.on_turn_start();
        }
        if let Ok(mut pending) = self.rewind_pending_prompt.lock()
            && let Some(prev_text) = pending.take()
        {
            let new_text = prompt_blocks.iter().fold(String::new(), |mut acc, b| {
                if let acp::ContentBlock::Text(t) = b {
                    acc.push_str(&t.text);
                }
                acc
            });
            if new_text.trim() == prev_text.trim() {
                self.signals_handle().record_regeneration();
            } else {
                self.signals_handle().record_edit_and_retry();
            }
        }
        self.open_subagent_spawn_admission();

        let original_prompt_text = prompt_blocks.iter().fold(String::new(), |mut acc, b| {
            if let acp::ContentBlock::Text(t) = b {
                acc.push_str(&t.text);
            }
            acc
        });
        // The turn intent is the admission boundary for every route below,
        // including slash commands, Goal/Workflow launches, and direct Bash.
        // No route may perform an external effect before this fact is durable.
        self.events.begin_turn();
        let model_id = self.current_catalog_model_id();
        let turn_number = self.chat_state_handle.get_prompt_index().await as u64;
        self.current_turn_number.set(turn_number);
        let permission_mode = self.permissions.mode();
        let msg_count = self.chat_state_handle.get_conversation_len().await;
        let redirect_kind = if matches!(origin, super::super::PromptOrigin::User) {
            self.events.take_prior_redirect_kind()
        } else {
            None
        };
        let input_kind = if Self::extract_bash_command(&prompt_blocks).is_some() {
            chat_state::TurnInputKind::Bash
        } else {
            chat_state::TurnInputKind::Prompt
        };
        let mut turn_identity = origin.turn_identity(turn_kind);
        if matches!(origin, super::super::PromptOrigin::User)
            && admitted_behavior == tool_types::BehaviorId::Goal
            && let Some(goal) = self.goal_tracker.lock().snapshot()
            && goal.status == crate::session::goal_tracker::GoalStatus::Active
        {
            turn_identity.goal_id = Some(goal.goal_id.clone());
            turn_identity.goal_definition_revision = Some(goal.definition_revision);
        }
        if turn_identity.goal_id.is_none() {
            turn_identity.goal_id = self.goal_usage_window.active_goal_id();
            if let Some(goal_id) = turn_identity.goal_id.as_deref()
                && let Some(goal) = self
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .filter(|goal| goal.goal_id == goal_id)
            {
                turn_identity.goal_definition_revision = Some(goal.definition_revision);
            }
        }
        if let Err(error) = self
            .events
            .start_turn(crate::session::events::Event::TurnStarted {
                session_id: self.session_id_string(),
                turn_number,
                identity: turn_identity,
                model_id: model_id.clone(),
                permission_mode,
                conversation_message_count: msg_count,
                prompt_index: Some(turn_number as usize),
                prompt_text: Some(original_prompt_text.trim().to_owned()),
                input_kind,
                session_relationship: crate::session::events::SessionRelationship::Primary,
                schema_version: crate::session::events::EVENT_SCHEMA_VERSION.into(),
                redirect_kind,
            })
            .await
        {
            super::super::tasks_cancel::signal_durable_turn_start(false);
            if let Some(extension) = &self.idle_prompt_extension {
                extension.on_turn_failed();
            }
            return Err(crate::session::commands::fatal_turn_boundary_error(
                "start",
                error.to_string(),
            ));
        }
        super::super::tasks_cancel::signal_durable_turn_start(true);
        // SUPPRESS_TURN is scoped to the durable Timeline turn, not to one
        // invocation of the model/recovery wrapper. Stop-hook continuations
        // and completion recovery may sample several times under this same
        // TurnStarted fact and must not resurrect a known-impossible compact
        // plan between those samples.
        let _ = self.compaction.auto_compact_suppressed.compare_exchange(
            crate::session::compaction_config::SUPPRESS_TURN,
            crate::session::compaction_config::SUPPRESS_NONE,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        );
        let mut model_path_started = false;
        // `turn_number` is the coordinate admitted for this turn. Recording the
        // durable Turn::Started advances Timeline's *next* prompt index, so it
        // must not be queried again for facts that belong to this turn.
        let current_prompt_index = turn_number as usize;
        let mut turn_scope_guard = None;
        let mut turn_model_id = None;
        let mut turn_timer = None;
        let mut timeline_error_override = None;
        let execution: Result<AdmittedTurnSuccess, acp::Error> = async {
            let implicit_goal_set = should_capture_implicit_goal_objective(
                &origin,
                admitted_behavior == tool_types::BehaviorId::Goal,
                self.goal_tracker.lock().status(),
                &original_prompt_text,
            );
            if implicit_goal_set {
                self.persist_host_turn_user_echo(&original_prompt_text, prompt_id);
                if let Err(message) = self
                    .initialize_goal_runtime(original_prompt_text.trim(), None)
                    .await
                {
                    self.send_host_turn_slash_command_output(&message).await;
                }
                return ok_end_turn(0, None).map(AdmittedTurnSuccess::Host);
            }
            if let Some(bash_command) = Self::extract_bash_command(&prompt_blocks) {
                return self
                    .handle_direct_bash_command(prompt_id, bash_command, &prompt_blocks)
                    .await
                    .map(AdmittedTurnSuccess::Host);
            }
            let slash_skills = self.slash_skills_for_resolve().await;
            let skill_rewrite = slash_commands::SkillSlashRewrite::RewriteToRun;
            let availability = self.command_availability().await;
            let mut pending_skill_information: Option<String> = None;
            let (_, named_workflows, _) = self.named_workflow_snapshot();
            let prompt_blocks = match slash_commands::resolve(
                prompt_blocks,
                &slash_skills,
                availability,
                skill_rewrite,
                &named_workflows,
            ) {
                Ok(blocks) => blocks,
                Err(SlashCommandOutcome::Builtin(action)) => {
                    let slash_used = ::diagnostics::events::SlashCommandUsed {
                        command: action.command_name().to_string(),
                        args_provided: action.args_provided(),
                    };
                    {
                        let span = tracing::Span::current();
                        span.record("command_name", action.command_name());
                        span.record("command_source", "builtin");
                    }
                    match action {
                        action @ (BuiltinAction::GoalSet { .. }
                        | BuiltinAction::GoalEdit { .. }
                        | BuiltinAction::GoalEnter
                        | BuiltinAction::GoalUsage
                        | BuiltinAction::GoalStatus
                        | BuiltinAction::GoalPause
                        | BuiltinAction::GoalRestart
                        | BuiltinAction::GoalClear
                        | BuiltinAction::GoalBudget { .. }) => {
                            ::diagnostics::session_ctx::log_event(slash_used);
                            self.persist_host_turn_user_echo(&original_prompt_text, prompt_id);
                            return self
                                .execute_builtin_slash_command(action)
                                .await
                                .map(AdmittedTurnSuccess::Host);
                        }
                        BuiltinAction::WorkflowLaunch { name, input } => {
                            self.persist_host_turn_user_echo(&original_prompt_text, prompt_id);
                            let msg = self.launch_named_workflow(&name, &input).await;
                            self.send_host_turn_slash_command_output(&msg).await;
                            return ok_end_turn(0, None).map(AdmittedTurnSuccess::Host);
                        }
                        _ => {
                            self.persist_host_turn_user_echo(&original_prompt_text, prompt_id);
                            return self
                                .execute_builtin_slash_command(action)
                                .await
                                .map(AdmittedTurnSuccess::Host);
                        }
                    }
                }
                Err(SlashCommandOutcome::InvokeSkill {
                    blocks: original_blocks,
                    skills: parsed_skills,
                }) => {
                    if let Some(first) = parsed_skills.first() {
                        *self.active_skill.lock() = Some(first.name.clone());
                        let span = tracing::Span::current();
                        span.record("command_name", first.name.as_str());
                        span.record(
                            "command_source",
                            if first.plugin_name.is_some() {
                                "plugin"
                            } else {
                                "skill"
                            },
                        );
                    }
                    for sk in &parsed_skills {
                        ::diagnostics::session_ctx::log_event(
                            ::diagnostics::events::SlashCommandUsed {
                                command: sk.name.clone(),
                                args_provided: !sk.args.is_empty(),
                            },
                        );
                        ::diagnostics::session_ctx::log_event(
                            ::diagnostics::events::SkillDispatched {
                                skill_name: sk.name.clone(),
                                plugin_source: sk.plugin_name.clone(),
                            },
                        );
                        let skill_source = if sk.plugin_name.is_some() {
                            "plugin"
                        } else {
                            crate::session::diagnostics::skill_source_label(
                                &sk.skill_path,
                                self.session_info.cwd.as_str(),
                            )
                        };
                        tracing::info_span!(
                            "skill.activated",
                            skill_name = %sk.name,
                            invocation_trigger = "slash_command",
                            skill_source = skill_source,
                        )
                        .in_scope(|| {});
                        if let Some(ref pname) = sk.plugin_name {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::PluginUsed {
                                    plugin_id: pname.clone(),
                                    plugin_name: pname.clone(),
                                    skill_name: Some(sk.name.clone()),
                                    hook_event: None,
                                    success: true,
                                },
                            );
                            tracing::info_span!(
                                "plugin.used",
                                plugin_name = %pname,
                                skill_name = %sk.name,
                            )
                            .in_scope(|| {});
                        }
                    }
                    pending_skill_information = slash_commands::build_skill_information_for_refs(
                        &parsed_skills,
                        &slash_skills,
                        &self.session_id_string(),
                    )
                    .await;
                    original_blocks
                }
            };
            model_path_started = true;
            self.publish_goal_mutation_authority(prompt_id, current_prompt_index as u64)
                .await;
            self.send_before_turn_event(tool_protocol::turn_hook::BeforeTurnPayload {
                turn_number: current_prompt_index as u64,
                model_id: model_id.clone(),
                conversation_message_count: msg_count,
                session_relationship: tool_protocol::turn_hook::DEFAULT_SESSION_RELATIONSHIP
                    .to_string(),
                schema_version: crate::session::events::EVENT_SCHEMA_VERSION.to_string(),
            })
            .await;
            ::diagnostics::session_ctx::log_session_event(crate::agent::session_metrics::Turn {
                session_id: self.session_info.id.0.to_string(),
                turn_number: current_prompt_index as u64,
            });
            ::diagnostics::session_ctx::begin_prompt_id();
            let mut chunk_meta = serde_json::Map::new();
            chunk_meta.insert("modelId".into(), serde_json::json!(model_id));
            chunk_meta.insert(
                "promptIndex".into(),
                serde_json::json!(current_prompt_index),
            );
            chunk_meta.insert("messageId".into(), serde_json::json!(prompt_id));
            if matches!(origin, super::super::PromptOrigin::User) {
                chunk_meta.insert(
                    "permissionEvidence".into(),
                    serde_json::json!("direct_user"),
                );
            }
            if origin.hide_user_echo_from_scrollback() {
                chunk_meta.insert("hideFromScrollback".into(), serde_json::json!(true));
            }
            let user_chunk_meta = Some(chunk_meta);
            *self.tool_context.prompt_index.lock().await = current_prompt_index;
            self.file_state_tracker
                .begin_prompt(current_prompt_index)
                .await;
            let echo_mode = user_echo_mode(&origin);
            for block in prompt_blocks.iter() {
                let update = acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(block.clone()).meta(user_chunk_meta.clone()),
                );
                let notification_meta = self.build_notification_meta();
                let notification =
                    acp::SessionNotification::new(self.session_info.id.clone(), update)
                        .meta(notification_meta.as_object().cloned());
                if echo_mode == UserEchoMode::PersistOnly {
                    let _ = self
                        .notifications
                        .persistence_tx
                        .send(PersistenceMsg::Update(
                            crate::session::storage::SessionUpdate::Acp(Box::new(notification)),
                        ));
                } else {
                    self.emit_notification_direct(notification).await;
                }
            }
            let crate::session::prompt_parser::ParsedPrompt {
                mut context,
                query,
                skill_information: skill_info,
                images: mut raw_images,
            } = match parse_prompt_with_skills(
                &prompt_blocks,
                self.tool_context.cwd.to_path_buf(),
                &self.session_info,
                verbatim,
                pending_skill_information.take().unwrap_or_default(),
            )
            .await
            {
                Ok(v) => v,
                Err(err) => {
                    tracing::warn!("Invalid prompt: {}", err.message);
                    return Err(err);
                }
            };
            let recovered = crate::session::placeholder_images::recover_orphan_placeholders(
                &query,
                &mut raw_images,
                std::path::Path::new(&self.session_info.cwd),
            );
            if recovered > 0 {
                tracing::info!(
                    session_id = %self.session_info.id,
                    recovered,
                    "server-side placeholder fallback: loaded orphan image(s) from disk",
                );
            }
            let query =
                crate::session::placeholder_images::strip_paths_from_image_placeholders(query);
            let user_images = self
                .normalize_images_with_notices(&mut context, raw_images)
                .await;
            let extraction = tools::util::base64_images::extract_base64_images(query);
            let (query, extra_images) = if extraction.images.is_empty() {
                (extraction.text, Vec::new())
            } else {
                let cleaned_text = extraction.text;
                let count = extraction.images.len();
                tracing::info!(
                    session_id = %self.session_info.id,
                    count,
                    "base64 images extracted from user query",
                );
                let acp_imgs: Vec<agent_client_protocol::schema::v1::ImageContent> = extraction
                    .images
                    .into_iter()
                    .map(|img| agent_client_protocol::schema::v1::ImageContent::new(img.data, img.mime_type))
                    .collect();
                let nr = crate::session::image_normalize::normalize_images(acp_imgs).await;
                if !nr.re_encode_fallbacks.is_empty() {
                    tracing::warn!(
                        session_id = %self.session_info.id,
                        notes = %nr.re_encode_fallbacks.join(" "),
                        "Extracted user query image kept original after re-encode failure",
                    );
                }
                (cleaned_text, nr.images)
            };
            let permission_text = query.trim().to_owned();
            let assembled = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
                &context,
                &query,
                &skill_info,
            );
            let user_message = if verbatim {
                assembled
            } else {
                self.maybe_truncate_large_prompt_with_skills(context, query, skill_info)
                    .await
            };
            let model_id = self.current_catalog_model_id();
            {
                let effective_client_identifier =
                    prompt_client_identifier.or_else(|| self.client_identifier.clone());
                let ev = ::diagnostics::events::PromptSubmitted {
                    prompt_length: user_message.len(),
                    model_id,
                    client_identifier: effective_client_identifier,
                    screen_mode: prompt_screen_mode,
                };
                ::diagnostics::session_ctx::log_event(ev);
            }
            self.maybe_inject_mcp_reminder().await;
            self.maybe_inject_mcp_connecting_reminder().await;
            self.maybe_inject_date_rollover_reminder().await;
            self.inject_behavior_reminders().await?;
            // A real user turn may absorb notifications that were already pending
            // at admission. An autonomous notification turn must first commit its
            // queued primary receipt; later arrivals belong after it in Surface.
            if notification_ids.is_empty() {
                self.drain_active_notifications().await;
            }
            if matches!(&origin, super::super::PromptOrigin::User) {
                ::diagnostics::unified_log::info(
                    "shell.task_wake.gate_cleared",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({ "reason": "handle_prompt_user_start" })),
                );
            }
            self.inject_workflow_status_reminder().await;
            let user_message = if user_images.is_empty() {
                user_message
            } else {
                crate::session::image_describe::persist_and_prepend_image_files(
                    &self.session_directory,
                    &user_images,
                    &user_message,
                )
                .map_err(|e| {
                    acp::Error::internal_error()
                        .data(format!("failed to save user images to assets dir: {e}"))
                })?
            };
            let prompt_text_for_hook = user_message.clone();
            {
                if matches!(origin, super::super::PromptOrigin::User) {
                    self.maybe_inject_interrupt_reminder().await;
                }
                let mut user_chat = match &origin {
                    super::super::PromptOrigin::TaskCompleted { .. } => {
                        ConversationItem::task_completed(user_message)
                    }
                    super::super::PromptOrigin::SubagentCompleted { .. } => {
                        ConversationItem::subagent_completed(user_message)
                    }
                    super::super::PromptOrigin::WorkflowCompleted { .. } => {
                        ConversationItem::notification_drain(user_message)
                    }
                    super::super::PromptOrigin::NotificationDrain => {
                        ConversationItem::notification_drain(user_message)
                    }
                    super::super::PromptOrigin::HostCommand => {
                        ConversationItem::system_reminder(user_message)
                    }
                    super::super::PromptOrigin::GoalContinuation { .. } => self
                        .goal_directive_item(
                            user_message,
                            sampling_types::SyntheticReason::SystemReminder,
                        ),
                    super::super::PromptOrigin::PlanResume => {
                        ConversationItem::system_reminder(user_message)
                    }
                    super::super::PromptOrigin::User => {
                        let mut item = ConversationItem::user(user_message);
                        item.set_permission_evidence(
                            sampling_types::PermissionEvidence::direct_user(permission_text),
                        );
                        if let Some(interrupt) =
                            self.events.take_prior_interrupt_category().and_then(
                                crate::session::events::prior_turn_interrupt_from_cancellation,
                            )
                        {
                            item.set_prior_turn_interrupt(interrupt);
                        }
                        item
                    }
                };
                user_chat.set_prompt_index(current_prompt_index);
                for image in &user_images {
                    user_chat.add_image(pick_user_image_url(image));
                }
                for image in &extra_images {
                    user_chat.add_image(format!("data:{};base64,{}", image.mime_type, image.data));
                }
                let input_commit = if notification_ids.is_empty() {
                    self.chat_state_handle
                        .push_user_message_durably(user_chat)
                        .await
                } else {
                    let turn = self.events.current_turn().ok_or_else(|| {
                        chat_state::TimelineWriteError::Invalid(
                            chat_state::TimelineError::InvalidNotification,
                        )
                    });
                    match turn {
                        Ok(turn) => self
                            .consume_notifications_durably(
                                notification_ids.clone(),
                                turn,
                                Some(user_chat),
                            )
                            .await
                            .map(|_| ()),
                        Err(error) => Err(error),
                    }
                };
                if let Err(error) = input_commit {
                    tracing::error!(
                        session_id = %self.session_info.id.0,
                        prompt_id = %prompt_id,
                        %error,
                        "aborting turn: user-message Timeline commit failed"
                    );
                    timeline_error_override = Some((
                        "user_message_persistence_failed",
                        serde_json::json!({
                            "reason": "user_message_persistence_failed",
                            "error": error.to_string(),
                        }),
                    ));
                    return Err(acp::Error::internal_error()
                        .data(format!("user message was not durably recorded: {error}")));
                }
                if !notification_ids.is_empty() {
                    self.drain_active_notifications().await;
                }
                if !admitted_notification_task_ids.is_empty() {
                    let ids = admitted_notification_task_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>();
                    self.completion_delivery.consume(&ids);
                }
                if let Some(ack) = persist_ack {
                    let _ = ack.send(());
                }
            }
            if matches!(origin, super::super::PromptOrigin::User) {
                self.schedule_session_title(original_prompt_text.clone())
                    .await;
            }
            if !matches!(origin, super::super::PromptOrigin::HostCommand) {
                self.dispatch_hook(
                    ::hooks::event::HookEventName::UserPromptSubmit,
                    ::hooks::event::HookPayload::UserPromptSubmit {
                        prompt: Some(prompt_text_for_hook),
                    },
                    Some(prompt_id),
                    None,
                )
                .await;
            }
            turn_scope_guard = Some(TurnSubagentScopeGuard::new(
                self.current_prompt_id.clone(),
                prompt_id.to_string(),
            ));
            self.open_subagent_spawn_admission();
            turn_model_id = Some(self.current_catalog_model_id());
            turn_timer = Some(std::time::Instant::now());
            let result = {
                let mut stop_continuations_this_turn: u32 = 0;
                let mut step_already_started = false;
                loop {
                    if self.goal_runtime_available() {
                        let goal_loop_active = self.goal_tracker.lock().status()
                            == Some(crate::session::goal_tracker::GoalStatus::Active);
                        self.set_goal_loop_active(goal_loop_active);
                    }
                    let round = self
                        .process_conversation_turn_with_recovery(
                            prompt_id,
                            origin.clone(),
                            json_schema.clone(),
                            std::mem::take(&mut step_already_started),
                        )
                        .await;
                    if !matches!(round, Ok(TurnOutcome::Completed { .. })) {
                        break round;
                    }
                    if matches!(
                        round,
                        Ok(TurnOutcome::Completed {
                            refusal: Some(_),
                            ..
                        })
                    ) {
                        break round;
                    }
                    if matches!(origin, super::super::PromptOrigin::GoalContinuation { .. }) {
                        break round;
                    }
                    match self
                        .run_stop_gate(prompt_id, stop_continuations_this_turn)
                        .await
                    {
                        StopGateDecision::AllowStop => break round,
                        StopGateDecision::KeepWorking { feedback } => {
                            if self
                                .enforce_goal_spending_limit_for_prompt(Some(prompt_id))
                                .await
                            {
                                let snapshot = match round {
                                    Ok(TurnOutcome::Completed { snapshot, .. }) => snapshot,
                                    _ => Box::new(None),
                                };
                                break Ok(TurnOutcome::GoalSpendingStopped { snapshot });
                            }
                            if !self
                                .start_step_after_control_boundary(
                                    prompt_id,
                                    Some(ConversationItem::stop_hook_feedback(feedback)),
                                )
                                .await
                            {
                                break Ok(TurnOutcome::Cancelled {
                                    category: None,
                                    context: Some(serde_json::json!({
                                        "reason": "stop-hook continuation lost foreground admission",
                                    })),
                                });
                            }
                            stop_continuations_this_turn += 1;
                            step_already_started = true;
                        }
                    }
                }
            };
            result.map(AdmittedTurnSuccess::Model)
        }
        .await;
        let turn_duration_ms = turn_timer
            .map(|timer| timer.elapsed().as_millis() as u64)
            .unwrap_or_default();
        let handle_prompt_elapsed_ms = handle_prompt_start.elapsed().as_millis() as u64;
        ::diagnostics::unified_log::info(
            "shell.handle_prompt.done",
            Some(self.session_info.id.0.as_ref()),
            Some(serde_json::json!({
                "prompt_id": prompt_id,
                "total_elapsed_ms": handle_prompt_elapsed_ms,
                "turn_elapsed_ms": turn_duration_ms,
                "pre_turn_ms": handle_prompt_elapsed_ms.saturating_sub(turn_duration_ms),
                "ok": execution.is_ok(),
            })),
        );
        let turn_tool_count = self.events.tool_count_this_turn();
        let (timeline_outcome, timeline_terminal, timeline_category, timeline_context) =
            match &execution {
                Ok(AdmittedTurnSuccess::Host(ok)) => match &ok.completion_kind {
                    PromptCompletionKind::Completed => (
                        crate::session::events::TurnOutcomeLabel::Completed,
                        chat_state::TurnTerminal {
                            stop_reason: "end_turn".into(),
                            completion_kind: "completed".into(),
                        },
                        None,
                        None,
                    ),
                    PromptCompletionKind::StationarityEnded => (
                        crate::session::events::TurnOutcomeLabel::Completed,
                        chat_state::TurnTerminal {
                            stop_reason: "end_turn".into(),
                            completion_kind: "stationarity_ended".into(),
                        },
                        None,
                        None,
                    ),
                    PromptCompletionKind::Cancelled { category, context } => (
                        crate::session::events::TurnOutcomeLabel::Cancelled,
                        chat_state::TurnTerminal {
                            stop_reason: "cancelled".into(),
                            completion_kind: "cancelled".into(),
                        },
                        *category,
                        context
                            .as_ref()
                            .and_then(|context| serde_json::to_value(context).ok()),
                    ),
                    PromptCompletionKind::MaxTurnsReached { limit } => (
                        crate::session::events::TurnOutcomeLabel::Cancelled,
                        chat_state::TurnTerminal {
                            stop_reason: "cancelled".into(),
                            completion_kind: "max_turns_reached".into(),
                        },
                        None,
                        Some(serde_json::json!({
                            "reason": "max_turns_reached",
                            "limit": limit,
                        })),
                    ),
                    PromptCompletionKind::Rewound | PromptCompletionKind::RemovedFromQueue => (
                        crate::session::events::TurnOutcomeLabel::Error,
                        chat_state::TurnTerminal {
                            stop_reason: "error".into(),
                            completion_kind: "invalid_admitted_completion".into(),
                        },
                        None,
                        None,
                    ),
                },
                Ok(AdmittedTurnSuccess::Model(TurnOutcome::Completed { refusal, .. })) => (
                    crate::session::events::TurnOutcomeLabel::Completed,
                    chat_state::TurnTerminal {
                        stop_reason: if refusal.is_some() {
                            "refusal"
                        } else {
                            "end_turn"
                        }
                        .into(),
                        completion_kind: "completed".into(),
                    },
                    None,
                    None,
                ),
                Ok(AdmittedTurnSuccess::Model(TurnOutcome::ControlBoundary { .. })) => (
                    crate::session::events::TurnOutcomeLabel::Completed,
                    chat_state::TurnTerminal {
                        stop_reason: "end_turn".into(),
                        completion_kind: "control_boundary".into(),
                    },
                    None,
                    None,
                ),
                Ok(AdmittedTurnSuccess::Model(TurnOutcome::GoalSpendingStopped { .. })) => (
                    crate::session::events::TurnOutcomeLabel::Completed,
                    chat_state::TurnTerminal {
                        stop_reason: "end_turn".into(),
                        completion_kind: "goal_spending_stopped".into(),
                    },
                    None,
                    None,
                ),
                Ok(AdmittedTurnSuccess::Model(TurnOutcome::StationarityEnded { .. })) => (
                    crate::session::events::TurnOutcomeLabel::Completed,
                    chat_state::TurnTerminal {
                        stop_reason: "end_turn".into(),
                        completion_kind: "stationarity_ended".into(),
                    },
                    None,
                    None,
                ),
                Ok(AdmittedTurnSuccess::Model(TurnOutcome::Cancelled { category, context })) => (
                    crate::session::events::TurnOutcomeLabel::Cancelled,
                    chat_state::TurnTerminal {
                        stop_reason: "cancelled".into(),
                        completion_kind: "cancelled".into(),
                    },
                    *category,
                    context.clone(),
                ),
                Ok(AdmittedTurnSuccess::Model(TurnOutcome::MaxTurnsReached { limit })) => (
                    crate::session::events::TurnOutcomeLabel::Cancelled,
                    chat_state::TurnTerminal {
                        stop_reason: "cancelled".into(),
                        completion_kind: "max_turns_reached".into(),
                    },
                    None,
                    Some(serde_json::json!({
                        "reason": "max_turns_reached",
                        "limit": limit,
                    })),
                ),
                Err(_) => (
                    crate::session::events::TurnOutcomeLabel::Error,
                    chat_state::TurnTerminal {
                        stop_reason: "error".into(),
                        completion_kind: timeline_error_override
                            .as_ref()
                            .map(|(kind, _)| *kind)
                            .unwrap_or("error")
                            .into(),
                    },
                    None,
                    timeline_error_override
                        .as_ref()
                        .map(|(_, context)| context.clone()),
                ),
            };
        // Transfer foreground ownership before the durable terminal command is
        // enqueued. Stop may still arrive while the writer acknowledgement or
        // post-turn hooks are pending, but it must then observe Settling and
        // leave this single terminal transaction in charge.
        let terminalization_owned = {
            let _boundary = self.step_control_gate.lock().await;
            self.state
                .lock()
                .await
                .foreground
                .begin_terminalization(prompt_id)
        };
        if !terminalization_owned {
            return Err(crate::session::commands::fatal_turn_boundary_error(
                "terminal ownership",
                format!("turn {prompt_id} lost foreground ownership before terminalization"),
            ));
        }
        if let Err(error) = self
            .emit_turn_ended(
                timeline_outcome,
                timeline_terminal,
                timeline_category,
                timeline_context,
            )
            .await
        {
            if let Some(extension) = &self.idle_prompt_extension {
                extension.on_turn_failed();
            }
            self.cancel_running_turn_subagents(prompt_id);
            self.flush_to_disk().await;
            if model_path_started {
                self.file_state_tracker
                    .end_prompt(&self.tool_context.fs, current_prompt_index)
                    .await;
                if let Some(rewind_point) = self
                    .file_state_tracker
                    .get_rewind_point(current_prompt_index)
                    .await
                {
                    let _ = self
                        .notifications
                        .persistence_tx
                        .send(PersistenceMsg::RewindPoint(rewind_point));
                }
            }
            let usage = if model_path_started {
                self.freeze_prompt_usage_bounded(prompt_id, std::time::Duration::ZERO)
                    .await
            } else {
                None
            };
            drop(turn_scope_guard);
            let boundary_error =
                crate::session::commands::fatal_turn_boundary_error("terminal", error.to_string());
            return Err(crate::sampling::error::attach_prompt_usage(
                boundary_error,
                usage,
            ));
        }
        let result = match execution {
            Ok(AdmittedTurnSuccess::Host(ok)) => {
                if let Some(extension) = &self.idle_prompt_extension {
                    extension.on_turn_failed();
                }
                return Ok(ok);
            }
            Ok(AdmittedTurnSuccess::Model(outcome)) => Ok(outcome),
            Err(error) if !model_path_started => return Err(error),
            Err(error) => Err(error),
        };
        let turn_model_id = turn_model_id.unwrap_or(model_id);
        let doom_event_model = turn_model_id.clone();
        match &result {
            Ok(TurnOutcome::Completed { refusal, .. }) => {
                if let Some(explanation) = refusal {
                    let details = (!explanation.is_empty()).then(|| explanation.clone());
                    self.dispatch_hook(
                        ::hooks::event::HookEventName::StopFailure,
                        ::hooks::event::HookPayload::StopFailure {
                            error: ::hooks::event::StopFailureKind::InvalidRequest,
                            error_details: details.clone(),
                            last_assistant_message: details,
                        },
                        Some(prompt_id),
                        None,
                    )
                    .await;
                }
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: None,
                    cancellation_context: None,
                })
                .await;
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: None,
                    error_category: None,
                });
            }
            Ok(TurnOutcome::ControlBoundary { .. }) => {
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: Some("control_boundary".to_string()),
                    cancellation_context: None,
                })
                .await;
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: Some("control_boundary".to_string()),
                    error_category: None,
                });
            }
            Ok(TurnOutcome::GoalSpendingStopped { .. }) => {
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: Some("goal_spending_stopped".to_string()),
                    cancellation_context: None,
                })
                .await;
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: Some("goal_spending_stopped".to_string()),
                    error_category: None,
                });
            }
            Ok(TurnOutcome::StationarityEnded { .. }) => {
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: Some("action_stationarity".to_string()),
                    cancellation_context: None,
                })
                .await;
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Completed,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: Some("action_stationarity".to_string()),
                    error_category: None,
                });
            }
            Ok(TurnOutcome::Cancelled { category, context }) => {
                if let Some(cause) = category {
                    self.events.set_prior_interrupt_category(*cause);
                }
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Cancelled,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: cancellation_category_to_wire_string(*category),
                    cancellation_context: context.clone(),
                })
                .await;
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Cancelled,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: category.map(|c| format!("{c:?}")),
                    error_category: None,
                });
            }
            Ok(TurnOutcome::MaxTurnsReached { limit }) => {
                tracing::info!(limit, "turn ended: max_turns reached");
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Cancelled,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: None,
                    cancellation_context: Some(serde_json::json!({
                        "reason": "max_turns_reached",
                        "limit": limit,
                    })),
                })
                .await;
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Cancelled,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: Some("max_turns_reached".to_string()),
                    error_category: None,
                });
            }
            Err(err) => {
                self.send_after_turn_event(tool_protocol::turn_hook::AfterTurnPayload {
                    turn_number: current_prompt_index as u64,
                    outcome: tool_protocol::turn_hook::TurnHookOutcome::Error,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id.clone(),
                    written_repo_paths: Vec::new(),
                    cancellation_category: None,
                    cancellation_context: None,
                })
                .await;
                let error_category = Self::classify_turn_error(err);
                ::diagnostics::session_ctx::log_session_event(::diagnostics::events::ApiError {
                    error_category: error_category.clone(),
                    model_id: turn_model_id.clone(),
                    status_code: None,
                    duration_ms: Some(turn_duration_ms),
                });
                ::diagnostics::session_ctx::log_event(::diagnostics::events::TurnCompleted {
                    outcome: ::diagnostics::events::Outcome::Error,
                    duration_ms: turn_duration_ms,
                    tool_call_count: turn_tool_count,
                    model_id: turn_model_id,
                    cancellation_category: None,
                    error_category: Some(error_category),
                });
                self.dispatch_hook(
                    ::hooks::event::HookEventName::StopFailure,
                    ::hooks::event::HookPayload::StopFailure {
                        error: Self::stop_failure_error_type(err),
                        error_details: Self::turn_error_detail(err),
                        last_assistant_message: Some(Self::format_turn_error_message(err)),
                    },
                    Some(prompt_id),
                    None,
                )
                .await;
            }
        }
        ::diagnostics::session_ctx::log_session_event(
            crate::agent::session_metrics::TurnCompletedLifecycle {
                session_id: self.session_info.id.0.to_string(),
                turn_number: current_prompt_index as u64,
            },
        );
        let doom_tally = std::mem::take(&mut *self.doom_loop_turn_tally.lock());
        if doom_tally.fired() {
            ::diagnostics::session_ctx::log_session_event(
                crate::agent::session_metrics::DoomLoopRecovery {
                    session_id: self.session_info.id.0.to_string(),
                    turn_number: current_prompt_index as u64,
                    attempts: doom_tally.attempts,
                    accepted_after_budget: doom_tally.accepted_after_budget,
                    top_trigger: doom_tally.top_trigger,
                    model: doom_event_model,
                },
            );
        }
        match &result {
            Ok(TurnOutcome::Completed { .. })
            | Ok(TurnOutcome::ControlBoundary { .. })
            | Ok(TurnOutcome::GoalSpendingStopped { .. })
            | Ok(TurnOutcome::StationarityEnded { .. }) => {
                if let Some(extension) = &self.idle_prompt_extension {
                    extension.on_turn_done();
                }
            }
            Ok(TurnOutcome::Cancelled { .. }) | Ok(TurnOutcome::MaxTurnsReached { .. }) => {
                if let Some(extension) = &self.idle_prompt_extension {
                    extension.on_turn_failed();
                }
            }
            Err(_) => {
                if let Some(extension) = &self.idle_prompt_extension {
                    extension.on_turn_failed();
                }
            }
        }
        if matches!(
            result,
            Ok(TurnOutcome::Cancelled { .. }) | Ok(TurnOutcome::MaxTurnsReached { .. })
        ) {
            self.cancel_running_turn_subagents(prompt_id);
        }
        self.flush_to_disk().await;
        self.file_state_tracker
            .end_prompt(&self.tool_context.fs, current_prompt_index)
            .await;
        if let Some(rewind_point) = self
            .file_state_tracker
            .get_rewind_point(current_prompt_index)
            .await
        {
            let _ = self
                .notifications
                .persistence_tx
                .send(PersistenceMsg::RewindPoint(rewind_point));
        }
        match result {
            Ok(outcome) => {
                let usage = self.freeze_prompt_usage(prompt_id).await;
                drop(turn_scope_guard);
                self.chat_state_handle.flush();
                let total_tokens = self.chat_state_handle.get_projected_tokens().await;
                let (stop_reason, mut snapshot, completion_kind, structured_output) = match outcome
                {
                    TurnOutcome::Completed {
                        snapshot,
                        structured_output,
                        refusal,
                        ..
                    } => (
                        if refusal.is_some() {
                            acp::StopReason::Refusal
                        } else {
                            acp::StopReason::EndTurn
                        },
                        *snapshot,
                        PromptCompletionKind::Completed,
                        structured_output,
                    ),
                    TurnOutcome::ControlBoundary { snapshot } => (
                        acp::StopReason::EndTurn,
                        *snapshot,
                        PromptCompletionKind::Completed,
                        None,
                    ),
                    TurnOutcome::GoalSpendingStopped { snapshot } => (
                        acp::StopReason::EndTurn,
                        *snapshot,
                        PromptCompletionKind::Completed,
                        None,
                    ),
                    TurnOutcome::StationarityEnded { snapshot, .. } => (
                        acp::StopReason::EndTurn,
                        *snapshot,
                        PromptCompletionKind::StationarityEnded,
                        None,
                    ),
                    TurnOutcome::Cancelled { category, context } => {
                        let cancellation_ctx = context.and_then(|v| serde_json::from_value(v).ok());
                        (
                            acp::StopReason::Cancelled,
                            None,
                            PromptCompletionKind::Cancelled {
                                category,
                                context: cancellation_ctx,
                            },
                            None,
                        )
                    }
                    TurnOutcome::MaxTurnsReached { limit } => (
                        acp::StopReason::Cancelled,
                        None,
                        PromptCompletionKind::MaxTurnsReached { limit },
                        None,
                    ),
                };
                if let Some(snapshot) = snapshot.as_mut() {
                    self.apply_behavior_to_snapshot(snapshot);
                }
                Ok(crate::session::commands::PromptTurnOk {
                    stop_reason,
                    total_tokens,
                    turn_snapshot: snapshot,
                    completion_kind,
                    structured_output,
                    usage,
                })
            }
            Err(e) => {
                let usage = self.freeze_prompt_usage(prompt_id).await;
                drop(turn_scope_guard);
                Err(crate::sampling::error::attach_prompt_usage(e, usage))
            }
        }
    }
}
