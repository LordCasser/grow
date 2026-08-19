use super::*;
use crate::remote::DEFAULT_CONTEXT_WINDOW;
use chat_state::conversation_util::replace_or_insert_system_head;
impl SessionActor {
    /// Adopt a validated live catalog/provider snapshot at a mailbox safe
    /// point. This updates routing, limits, credentials and the auxiliary
    /// image model without rebuilding the agent harness.
    pub(super) async fn handle_reload_model_config(
        &self,
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        image_description_model: Option<String>,
        inference_idle_timeout: std::time::Duration,
        max_retries: u32,
        auto_compact_threshold_percent: u8,
    ) {
        // These actor-local knobs participate in the same mailbox snapshot as
        // the provider route. Setting them before the first await prevents a
        // turn spawned after this command from rebuilding a hybrid old/new
        // sampler config.
        self.inference_idle_timeout.set(inference_idle_timeout);
        self.max_retries.set(max_retries);
        self.compaction
            .threshold_percent
            .set(auto_compact_threshold_percent);
        let context_window = self.compaction.context_window_override.unwrap_or_else(|| {
            std::num::NonZeroU64::new(sampling_config.context_window).unwrap_or_else(|| {
                std::num::NonZeroU64::new(DEFAULT_CONTEXT_WINDOW)
                    .expect("DEFAULT_CONTEXT_WINDOW is non-zero")
            })
        });
        self.compactions_remaining
            .set(sampling_config.compactions_remaining);
        self.compaction_at_tokens
            .set(sampling_config.compaction_at_tokens);
        self.chat_state_handle
            .update_sampling_config(sampling_types::SamplingConfig {
                base_url: sampling_config.base_url.clone(),
                model: sampling_config.model.clone(),
                output_limit: sampling_config.output_limit,
                temperature: sampling_config.temperature,
                top_p: sampling_config.top_p,
                api_backend: sampling_config.api_backend.clone(),
                extra_headers: sampling_config.extra_headers.clone(),
                query_params: sampling_config.query_params.clone(),
                env_http_headers: sampling_config.env_http_headers.clone(),
                context_window,
                reasoning_effort: sampling_config.reasoning_effort,
                stream_tool_calls: Some(sampling_config.stream_tool_calls),
            });
        let existing = self.chat_state_handle.get_credentials().await;
        self.chat_state_handle
            .update_credentials(chat_state::Credentials {
                api_key: sampling_config.api_key.clone(),
                alpha_test_key: existing.alpha_test_key,
            });
        *self.image_description_model.write() = image_description_model;
        self.invalidate_model_auth_memo();
        let agent_name = self.agent.borrow().definition().name.clone();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id,
                agent_name: Some(agent_name),
                reasoning_effort: Some(sampling_config.reasoning_effort),
            });
    }

    pub(super) async fn handle_set_session_model(
        &self,
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        use_concise: bool,
        apply_prompt_override: bool,
        skip_prompt_rewrite: bool,
        auto_compact_threshold_percent: u8,
    ) -> Result<acp::ModelId, acp::Error> {
        let new_context_window = self.compaction.context_window_override.unwrap_or_else(|| {
            std::num::NonZeroU64::new(sampling_config.context_window).unwrap_or_else(|| {
                std::num::NonZeroU64::new(DEFAULT_CONTEXT_WINDOW)
                    .expect("DEFAULT_CONTEXT_WINDOW is non-zero")
            })
        });
        let prev_threshold = self.compaction.threshold_percent.get();
        if prev_threshold != auto_compact_threshold_percent {
            tracing::info!(
                session_id = %self.session_info.id.0,
                new_model = %sampling_config.model,
                old_threshold = prev_threshold,
                new_threshold = auto_compact_threshold_percent,
                "auto_compact_threshold_percent updated for model switch"
            );
        }
        self.compaction
            .threshold_percent
            .set(auto_compact_threshold_percent);
        self.compactions_remaining
            .set(sampling_config.compactions_remaining);
        self.compaction_at_tokens
            .set(sampling_config.compaction_at_tokens);
        self.chat_state_handle
            .update_sampling_config(sampling_types::SamplingConfig {
                base_url: sampling_config.base_url.clone(),
                model: sampling_config.model.clone(),
                output_limit: sampling_config.output_limit,
                temperature: sampling_config.temperature,
                top_p: sampling_config.top_p,
                api_backend: sampling_config.api_backend.clone(),
                extra_headers: sampling_config.extra_headers.clone(),
                query_params: sampling_config.query_params.clone(),
                env_http_headers: sampling_config.env_http_headers.clone(),
                context_window: new_context_window,
                reasoning_effort: sampling_config.reasoning_effort,
                stream_tool_calls: Some(sampling_config.stream_tool_calls),
            });
        let existing = self.chat_state_handle.get_credentials().await;
        self.chat_state_handle
            .update_credentials(chat_state::Credentials {
                api_key: sampling_config.api_key.clone(),
                alpha_test_key: existing.alpha_test_key,
            });
        self.invalidate_model_auth_memo();
        self.signals_handle()
            .record_model_usage(&sampling_config.model);
        if apply_prompt_override && !skip_prompt_rewrite {
            let system_prompt = if use_concise {
                agent::prompt::template::COMPACT_SYSTEM_PROMPT.to_owned()
            } else {
                self.agent.borrow().system_prompt().to_owned()
            };
            self.chat_state_handle
                .replace_system_head(&system_prompt)
                .await
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("model context was not durably recorded: {error}"))
                })?;
        } else if !apply_prompt_override {
            tracing::info!(
                session_id = %self.session_info.id.0,
                model_id = %model_id.0,
                "handle_set_session_model: skipping prompt override (apply_prompt_override=false)"
            );
        } else {
            tracing::info!(
                session_id = %self.session_info.id.0,
                model_id = %model_id.0,
                "handle_set_session_model: skipping prompt rewrite (just rebuilt harness)"
            );
        }
        let agent_name = self.agent.borrow().definition().name.clone();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id: model_id.clone(),
                agent_name: Some(agent_name),
                reasoning_effort: Some(sampling_config.reasoning_effort),
            });
        Ok(model_id)
    }
    /// Handle [`SessionCommand::RebuildAgentForDefinition`].
    ///
    /// Builds a fresh [`agent::Agent`] from the cached
    /// [`crate::session::agent_rebuild::AgentRebuildSpec`] + the supplied
    /// [`agent::AgentDefinition`], replaces `self.agent`,
    /// rewrites the system message in the conversation, persists the
    /// new prompt artifacts, and updates the active Agent name.
    /// Agent selection is independent from model selection; this command
    /// preserves the current sampling configuration. Defense-in-depth:
    /// rejects if a turn is in flight.
    pub(super) async fn handle_rebuild_agent_for_definition(
        &self,
        definition: agent::AgentDefinition,
    ) -> Result<(), acp::Error> {
        {
            let state = self.state.lock().await;
            if state.foreground.regular().is_some() {
                tracing::warn!(
                    session_id = %self.session_info.id.0,
                    new_agent_type = %definition.name,
                    "handle_rebuild_agent_for_definition: turn in flight, rejecting rebuild"
                );
                return Err(acp::Error::internal_error()
                    .data("rebuild_agent: turn in flight, refusing to rebuild harness"));
            }
        }
        let new_agent_name = definition.name.clone();
        let current_sampling = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .ok_or_else(|| {
                acp::Error::internal_error()
                    .data("rebuild_agent: active session has no sampling config")
            })?;
        let current_model = acp::ModelId::new(current_sampling.model.clone());
        tracing::info!(
            session_id = %self.session_info.id.0,
            new_agent_type = %new_agent_name,
            "handle_rebuild_agent_for_definition: rebuilding harness"
        );
        let new_agent = self
            .rebuild_spec
            .build_agent(definition)
            .await
            .map_err(|e| {
                tracing::error!(
                    session_id = %self.session_info.id.0,
                    new_agent_type = %new_agent_name,
                    error = %e,
                    "handle_rebuild_agent_for_definition: AgentBuilder::build failed"
                );
                acp::Error::internal_error().data(format!(
                    "rebuild_agent: build failed for agent_type={new_agent_name}: {e}"
                ))
            })?;
        let new_system_prompt = new_agent.system_prompt().to_string();
        *self.agent.borrow_mut() = new_agent;
        *self.active_agent_type.lock() = Some(new_agent_name.clone());
        if let Err(e) = self.workspace_ops.bind_local_session(
            &self.session_id_string(),
            self.tool_context.cwd.as_path().to_path_buf(),
            self.tool_context.hunk_tracker_handle.clone(),
            self.agent.borrow().tool_bridge().toolset(),
            None,
        ) {
            tracing::warn!(error = %e, "failed to rebind local session toolset after agent rebuild");
        }
        {
            let bridge = self.agent.borrow().tool_bridge().clone();
            let snapshot = self.tool_metadata_snapshot.clone();
            let tool_index = crate::session::tool_index::Bm25ToolSearchIndex::new(snapshot);
            bridge
                .update_resource(tools::types::tool_index::ToolIndex(std::sync::Arc::new(
                    tool_index,
                )))
                .await;
            if let Some(display_cwd) = self.display_cwd.get() {
                bridge
                    .set_display_cwd(std::path::PathBuf::from(display_cwd))
                    .await;
            }
            bridge
                .update_resource(
                    tools::implementations::grow_build::workflow::WorkflowHandle {
                        sender: self.workflow_tx.clone(),
                        admitted_behavior: self.turn_behavior.clone(),
                    },
                )
                .await;
            bridge
                .update_resource(
                    tools::implementations::grow_build::update_goal::GoalRuntimeHandle(
                        self.goal_command_tx.clone(),
                    ),
                )
                .await;
            if let Some(reservations) = self.tool_context.task_completion_reservations.clone() {
                bridge.update_resource(reservations).await;
            }
            if let Some(gate) = self.tool_context.task_wake_suppressed.clone() {
                bridge.update_resource(gate).await;
            }
            self.inject_deny_read_globs().await;
        }
        {
            let notified = self.mcp_handshakes_done.notified();
            tokio::pin!(notified);
            let needs_wait = {
                let s = self.mcp_state.lock().await;
                !s.configs.is_empty() && !s.is_initialized()
            };
            if needs_wait {
                const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
                tokio::select! {
                    () = &mut notified => {}
                    () = tokio::time::sleep(TIMEOUT) => {
                        tracing::warn!(
                            session_id = %self.session_info.id.0,
                            "handle_rebuild_agent_for_definition: timed out waiting for MCP handshakes"
                        );
                    }
                }
            }
        }
        self.re_register_mcp_tools_on_rebuilt_bridge().await;
        if let Some(old_handle) = self.deferred_prefix.take() {
            old_handle.abort();
        }
        let new_user_prefix = self.build_user_message_prefix().await;
        {
            let Some((mut conversation, source_surface_revision)) = self
                .chat_state_handle
                .get_conversation_with_revision()
                .await
            else {
                return Err(acp::Error::internal_error()
                    .data("rebuilt agent context unavailable: chat-state actor stopped"));
            };
            let _ = replace_or_insert_system_head(&mut conversation, &new_system_prompt);
            Self::rewrite_zero_turn_prefix(&mut conversation, new_user_prefix);
            if !conversation_has_project_instructions(&conversation)
                && let Some(agents_md_reminder) = self.agent.borrow().agents_md_user_reminder()
            {
                let agents_md_at = conversation.len().min(2);
                conversation.insert(
                    agents_md_at,
                    ConversationItem::project_instructions(agents_md_reminder),
                );
            }
            self.inject_baseline_skill_reminder(&mut conversation).await;
            self.chat_state_handle
                .replace_context_durably(conversation, source_surface_revision)
                .await
                .map_err(|error| {
                    acp::Error::internal_error().data(format!(
                        "rebuilt agent context was not durably recorded: {error}"
                    ))
                })?;
        }
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id: current_model,
                agent_name: Some(new_agent_name.clone()),
                reasoning_effort: Some(current_sampling.reasoning_effort),
            });
        self.mcp_reminder_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.refresh_goal_harness_enabled().await;
        self.send_available_commands_update().await;
        tracing::info!(
            session_id = %self.session_info.id.0,
            new_agent_type = %new_agent_name,
            "handle_rebuild_agent_for_definition: harness rebuild complete"
        );
        Ok(())
    }
    /// Apply a client-supplied `systemPromptOverride` on session attach without
    /// wiping user/assistant history: swap only the leading `System` message,
    /// atomically inside the `ChatStateActor` (see
    /// `ChatStateCommand::ReplaceSystemHead` for the serialization guarantees).
    /// Skipped entirely on a verbatim mirror-fork (`preserve_inherited_system`).
    pub(super) async fn handle_replace_system_prompt(&self, system_prompt: String) {
        if self.startup_hints.preserve_inherited_system {
            tracing::debug!(
                session_id = %self.session_info.id.0,
                "handle_replace_system_prompt: skipped (preserve_inherited_system)"
            );
            return;
        }
        let changed = match self
            .chat_state_handle
            .replace_system_head(&system_prompt)
            .await
        {
            Ok(changed) => changed,
            Err(error) => {
                tracing::error!(
                session_id = %self.session_info.id.0,
                %error,
                "handle_replace_system_prompt: durable replacement failed; override not applied"
                );
                return;
            }
        };
        if changed {
            tracing::info!(
                session_id = %self.session_info.id.0,
                prompt_len = system_prompt.len(),
                "handle_replace_system_prompt: client override applied"
            );
        } else {
            tracing::debug!(
                session_id = %self.session_info.id.0,
                "handle_replace_system_prompt: head already matches, no-op"
            );
        }
    }
}
