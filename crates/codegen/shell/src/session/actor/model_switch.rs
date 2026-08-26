use super::*;
use crate::remote::DEFAULT_CONTEXT_WINDOW;
impl SessionActor {
    async fn commit_model_change(
        &self,
        model_id: &acp::ModelId,
        sampling_config: &sampler::SamplerConfig,
        reason: &str,
    ) -> std::io::Result<()> {
        let previous_route = self.model_route.snapshot();
        let previous_model_id = previous_route.model_id;
        let previous_sampling = previous_route.sampling_config;
        let previous_provider_model = previous_sampling.model.as_str();
        let previous_reasoning_effort = previous_sampling.reasoning_effort;
        let previous_transport_key = sampling_types::model_image_input_key_from_parts(
            &previous_sampling.model,
            &previous_sampling.api_backend,
            &previous_sampling.base_url,
            &previous_sampling.query_params,
        );
        let transport_key = sampling_types::model_image_input_key_from_parts(
            &sampling_config.model,
            &sampling_config.api_backend,
            &sampling_config.base_url,
            &sampling_config.query_params,
        );
        if previous_model_id != *model_id
            || previous_reasoning_effort != sampling_config.reasoning_effort
            || previous_provider_model != sampling_config.model
            || previous_transport_key != transport_key
        {
            self.chat_state_handle
                .record_timeline_event_durably(crate::session::persistence::model_change_event(
                    &previous_model_id,
                    model_id,
                    previous_reasoning_effort,
                    sampling_config.reasoning_effort,
                    previous_provider_model,
                    &sampling_config.model,
                    &previous_transport_key,
                    &transport_key,
                    reason,
                ))
                .await
                .map_err(std::io::Error::other)?;
        }
        Ok(())
    }

    /// Apply a validated catalog/provider snapshot while the caller owns the
    /// idle foreground-admission lock. This updates routing, limits,
    /// credentials and the auxiliary image model without rebuilding the agent
    /// harness.
    async fn apply_model_config_reload(
        &self,
        workflow_admission: &mut crate::session::workflow::manager::WorkflowManager,
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        image_description_model: Option<String>,
        inference_idle_timeout: std::time::Duration,
        max_retries: u32,
        auto_compact_threshold_percent: u8,
    ) -> Result<(), acp::Error> {
        let mut workflow_default_sampler = sampling_config.clone();
        workflow_default_sampler.idle_timeout_secs = Some(inference_idle_timeout.as_secs());
        workflow_default_sampler.max_retries = Some(max_retries);
        workflow_default_sampler.doom_loop_recovery = self.doom_loop_recovery;
        let alpha_test_key = self
            .chat_state_handle
            .get_credentials()
            .await
            .alpha_test_key;
        let next_run_route = crate::session::workflow::tracker::WorkflowRuntimeRoute::capture(
            model_id.0.to_string(),
            workflow_default_sampler,
            &self.models_manager,
            alpha_test_key,
        )
        .map_err(|error| acp::Error::invalid_request().data(error))?;
        self.commit_model_change(&model_id, &sampling_config, "catalog_reload")
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "catalog reload model transition was not durable: {error}"
                ))
            })?;
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
        self.model_route
            .replace(model_id.clone(), sampling_config.clone());
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
        workflow_admission.set_next_run_route(next_run_route);
        Ok(())
    }

    /// Adopt a validated live catalog/provider snapshot at a mailbox safe
    /// point. Direct callers receive an explicit busy error; the mailbox path
    /// uses [`Self::admit_model_config_reload`] so watcher reloads are deferred
    /// behind an admitted turn instead of being lost.
    pub(super) async fn handle_reload_model_config(
        &self,
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        image_description_model: Option<String>,
        inference_idle_timeout: std::time::Duration,
        max_retries: u32,
        auto_compact_threshold_percent: u8,
    ) -> Result<(), acp::Error> {
        // Lock order is Workflow admission -> foreground admission everywhere
        // these domains meet (Behavior switching uses the same order).
        let mut workflow_admission = self.workflow_manager.lock().await;
        let foreground_admission = self.state.lock().await;
        if !foreground_admission.foreground.is_idle() {
            return Err(acp::Error::internal_error()
                .data("an admitted foreground owns an immutable model route"));
        }
        let result = self
            .apply_model_config_reload(
                &mut workflow_admission,
                model_id,
                sampling_config,
                image_description_model,
                inference_idle_timeout,
                max_retries,
                auto_compact_threshold_percent,
            )
            .await;
        drop(foreground_admission);
        result
    }

    /// Mailbox admission for catalog hot reload. The current turn retains its
    /// exact provider route; a busy session coalesces snapshots and resolves
    /// every watcher acknowledgement only after the newest snapshot is
    /// durably applied at the next idle boundary.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn admit_model_config_reload(
        &self,
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        image_description_model: Option<String>,
        inference_idle_timeout: std::time::Duration,
        max_retries: u32,
        auto_compact_threshold_percent: u8,
        responds_to: tokio::sync::oneshot::Sender<Result<(), acp::Error>>,
    ) {
        let mut workflow_admission = self.workflow_manager.lock().await;
        let mut admission = self.state.lock().await;
        if !admission.foreground.is_idle() {
            let mut responders = admission
                .pending_model_reload
                .take()
                .map(|pending| pending.responders)
                .unwrap_or_default();
            responders.push(responds_to);
            admission.pending_model_reload = Some(PendingModelReload {
                model_id,
                sampling_config,
                image_description_model,
                inference_idle_timeout,
                max_retries,
                auto_compact_threshold_percent,
                responders,
            });
            return;
        }
        let result = self
            .apply_model_config_reload(
                &mut workflow_admission,
                model_id,
                sampling_config,
                image_description_model,
                inference_idle_timeout,
                max_retries,
                auto_compact_threshold_percent,
            )
            .await;
        drop(admission);
        let _ = responds_to.send(result);
    }

    /// Apply the coalesced watcher snapshot before any idle consumer can admit
    /// the next prompt, compaction, notification, or Goal continuation.
    pub(super) async fn apply_pending_model_reload_if_idle(&self) {
        let mut workflow_admission = self.workflow_manager.lock().await;
        let mut admission = self.state.lock().await;
        if !admission.foreground.is_idle() {
            return;
        }
        let Some(pending) = admission.pending_model_reload.take() else {
            return;
        };
        let result = self
            .apply_model_config_reload(
                &mut workflow_admission,
                pending.model_id,
                pending.sampling_config,
                pending.image_description_model,
                pending.inference_idle_timeout,
                pending.max_retries,
                pending.auto_compact_threshold_percent,
            )
            .await;
        drop(admission);
        for respond_to in pending.responders {
            let response = match &result {
                Ok(outcome) => Ok(*outcome),
                Err(error) => Err(error.clone()),
            };
            let _ = respond_to.send(response);
        }
    }

    pub(super) async fn handle_set_session_model(
        &self,
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        auto_compact_threshold_percent: u8,
    ) -> Result<acp::ModelId, acp::Error> {
        let mut workflow_admission = self.workflow_manager.lock().await;
        let foreground_admission = self.state.lock().await;
        if !foreground_admission.foreground.is_idle() {
            return Err(acp::Error::invalid_request().data(
                "stop the active foreground turn before changing model or reasoning effort",
            ));
        }
        let mut workflow_default_sampler = sampling_config.clone();
        workflow_default_sampler.idle_timeout_secs =
            Some(self.inference_idle_timeout.get().as_secs());
        workflow_default_sampler.doom_loop_recovery = self.doom_loop_recovery;
        let alpha_test_key = self
            .chat_state_handle
            .get_credentials()
            .await
            .alpha_test_key;
        let next_run_route = crate::session::workflow::tracker::WorkflowRuntimeRoute::capture(
            model_id.0.to_string(),
            workflow_default_sampler,
            &self.models_manager,
            alpha_test_key,
        )
        .map_err(|error| acp::Error::invalid_request().data(error))?;
        self.commit_model_change(&model_id, &sampling_config, "user_selection")
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("model change was not durably recorded: {error}"))
            })?;
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
        self.model_route
            .replace(model_id.clone(), sampling_config.clone());
        self.invalidate_model_auth_memo();
        self.signals_handle()
            .record_model_usage(&sampling_config.model);
        let agent_name = self.agent.borrow().definition().name.clone();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id: model_id.clone(),
                agent_name: Some(agent_name),
                reasoning_effort: Some(sampling_config.reasoning_effort),
            });
        workflow_admission.set_next_run_route(next_run_route);
        drop(foreground_admission);
        Ok(model_id)
    }
    /// Handle [`SessionCommand::RebuildAgentForDefinition`].
    ///
    /// Builds a fresh [`agent::Agent`] from the cached
    /// [`crate::session::agent_rebuild::AgentRebuildSpec`] + the supplied
    /// [`agent::AgentDefinition`], replaces `self.agent`,
    /// commits the rendered role as an append-only Timeline Control fact, and
    /// only then replaces the live harness.
    /// Agent selection is independent from model selection; this command
    /// preserves the current sampling configuration. Defense-in-depth:
    /// rejects if a turn is in flight.
    pub(super) async fn handle_rebuild_agent_for_definition(
        &self,
        definition: agent::AgentDefinition,
    ) -> Result<(), acp::Error> {
        let foreground_admission = self.state.lock().await;
        if !foreground_admission.foreground.is_idle() {
            tracing::warn!(
                session_id = %self.session_info.id.0,
                new_agent_type = %definition.name,
                "handle_rebuild_agent_for_definition: foreground active, rejecting rebuild"
            );
            return Err(acp::Error::internal_error()
                .data("rebuild_agent: foreground active, refusing to rebuild harness"));
        }
        let new_agent_name = definition.name.clone();
        let current_route = self.model_route.snapshot();
        let current_sampling = current_route.sampling_config;
        let current_model = current_route.model_id;
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
        let candidate_bridge = new_agent.tool_bridge().clone();
        let candidate_tool_names = candidate_bridge
            .tool_definitions_builtins_only()
            .await
            .into_iter()
            .map(|definition| definition.function.name)
            .collect::<Vec<_>>();
        let candidate_supports_plan = candidate_bridge
            .tool_for_kind(tools::types::tool::ToolKind::PlanControl)
            .await
            .is_some();
        let candidate_supports_workflow = candidate_bridge
            .tool_for_kind(tools::types::tool::ToolKind::Workflow)
            .await
            .is_some();
        let candidate_supports_goal = super::goal_support::goal_runtime_available_from_tools(
            self.goal_enabled,
            &candidate_tool_names,
        );
        let admitted_behavior = self.behavior.lock().behavior();
        let missing_runtime = match admitted_behavior {
            tool_types::BehaviorId::Plan if !candidate_supports_plan => Some("PlanControl"),
            tool_types::BehaviorId::Workflow if !candidate_supports_workflow => Some("Workflow"),
            tool_types::BehaviorId::Goal if !candidate_supports_goal => Some("Goal lifecycle"),
            _ => None,
        };
        if let Some(runtime) = missing_runtime {
            return Err(acp::Error::invalid_request().data(format!(
                "Agent `{new_agent_name}` cannot replace the current harness while {} Behavior is selected because it does not provide the required {runtime} tools.",
                admitted_behavior.display_label(),
            )));
        }
        self.persist_agent_transition_durably(new_agent.name(), new_agent.role_prompt())
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "rebuilt agent role was not durably recorded: {error}"
                ))
            })?;
        *self.agent.borrow_mut() = new_agent;
        // Keep foreground admission fenced until the new harness has every
        // critical runtime resource and MCP registration. Goal idle-driving is
        // detached from the command mailbox and could otherwise admit a turn
        // against a half-rebound Agent.
        if let Err(e) = self
            .workspace_ops
            .bind_local_session(
                &self.session_id_string(),
                self.tool_context.cwd.as_path().to_path_buf(),
                self.tool_context.hunk_tracker_handle.clone(),
                self.agent.borrow().tool_bridge().toolset(),
                None,
            )
            .await
        {
            tracing::warn!(error = %e, "failed to rebind local session toolset after agent rebuild");
        }
        {
            let bridge = self.agent.borrow().tool_bridge().clone();
            let snapshot = self.mcp.tool_metadata_snapshot.clone();
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
            self.inject_deny_read_globs().await;
        }
        {
            let notified = self.mcp.handshakes_done.notified();
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
        drop(foreground_admission);
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id: current_model,
                agent_name: Some(new_agent_name.clone()),
                reasoning_effort: Some(current_sampling.reasoning_effort),
            });
        self.mcp
            .reminder_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.refresh_goal_runtime_availability().await;
        self.send_available_commands_update().await;
        tracing::info!(
            session_id = %self.session_info.id.0,
            new_agent_type = %new_agent_name,
            "handle_rebuild_agent_for_definition: harness rebuild complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn consecutive_busy_catalog_reloads_coalesce_to_latest_and_ack_all() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
                let actor = super::super::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.state.lock().await.foreground = ForegroundState::Compaction;

                let mut first = sampler::SamplerConfig::default();
                first.model = "first-wire".into();
                first.base_url = "https://first.example/v1".into();
                first.context_window = 32_000;
                let (first_tx, mut first_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_model_config_reload(
                        acp::ModelId::new("first/catalog"),
                        first,
                        None,
                        std::time::Duration::from_secs(60),
                        3,
                        85,
                        first_tx,
                    )
                    .await;

                let mut latest = sampler::SamplerConfig::default();
                latest.model = "latest-wire".into();
                latest.base_url = "https://latest.example/v1".into();
                latest.context_window = 64_000;
                let (latest_tx, mut latest_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_model_config_reload(
                        acp::ModelId::new("latest/catalog"),
                        latest,
                        None,
                        std::time::Duration::from_secs(90),
                        4,
                        75,
                        latest_tx,
                    )
                    .await;

                assert!(matches!(
                    first_rx.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                assert!(matches!(
                    latest_rx.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));

                actor.state.lock().await.foreground = ForegroundState::Idle;
                actor.apply_pending_model_reload_if_idle().await;
                first_rx.await.unwrap().unwrap();
                latest_rx.await.unwrap().unwrap();
                let route = actor.model_route.snapshot();
                assert_eq!(route.model_id.0.as_ref(), "latest/catalog");
                assert_eq!(route.sampling_config.model, "latest-wire");
                assert_eq!(actor.inference_idle_timeout.get().as_secs(), 90);
                assert_eq!(actor.max_retries.get(), 4);
                assert_eq!(actor.compaction.threshold_percent.get(), 75);
            })
            .await;
    }

    #[tokio::test]
    async fn agent_switch_appends_one_typed_role_without_rebuilding_history() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let mut actor = super::super::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.todo_gate.enabled = true;
                let mut route = actor.model_route.snapshot().sampling_config;
                route.model = "provider-alias".into();
                actor
                    .model_route
                    .replace(acp::ModelId::new("catalog-alias"), route);
                actor.todo_gate.max_fires_per_prompt = 7;
                actor.compaction.threshold_percent.set(73);
                actor.compaction.memory_flush_enabled = true;
                actor.compaction.wall_clock_budget_secs = 41;
                let (_, revision) = actor
                    .chat_state_handle
                    .get_conversation_with_revision()
                    .await
                    .unwrap();
                let original = vec![
                    ConversationItem::system("test system prompt"),
                    ConversationItem::user("historical request"),
                ];
                actor
                    .chat_state_handle
                    .replace_context_durably(original.clone(), revision)
                    .await
                    .unwrap();

                let mut definition = agent::AgentDefinition::default_grow_build();
                definition.name = "reviewer".into();
                definition.prompt_body = Some("Review the implementation carefully.".into());
                let message_events_before = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|event| {
                        matches!(event.kind, chat_state::TimelineEventKind::Messages(_))
                    })
                    .count();
                actor
                    .handle_rebuild_agent_for_definition(definition)
                    .await
                    .unwrap();

                let surface = actor.chat_state_handle.get_conversation().await;
                assert_eq!(
                    serde_json::to_value(&surface[..original.len()]).unwrap(),
                    serde_json::to_value(&original).unwrap()
                );
                assert_eq!(surface.len(), original.len() + 1);
                assert!(
                    surface
                        .last()
                        .unwrap()
                        .text_content()
                        .contains("<agent-role>")
                );
                assert!(
                    surface
                        .last()
                        .unwrap()
                        .text_content()
                        .contains("`reviewer`")
                );
                assert_eq!(actor.agent.borrow().name(), "reviewer");
                assert!(actor.todo_gate.enabled);
                assert_eq!(actor.todo_gate.max_fires_per_prompt, 7);
                assert_eq!(actor.compaction.threshold_percent.get(), 73);
                assert!(actor.compaction.memory_flush_enabled);
                assert_eq!(actor.compaction.wall_clock_budget_secs, 41);
                let persisted_model =
                    tokio::time::timeout(std::time::Duration::from_secs(1), async {
                        loop {
                            if let Some(PersistenceMsg::CurrentModel { model_id, .. }) =
                                persistence_rx.recv().await
                            {
                                break model_id;
                            }
                        }
                    })
                    .await
                    .expect("Agent rebuild must publish its summary projection");
                assert_eq!(persisted_model.0.as_ref(), "catalog-alias");

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| {
                            matches!(event.kind, chat_state::TimelineEventKind::Messages(_))
                        })
                        .count(),
                    message_events_before,
                    "Agent selection must not perform a second ContextRebuild"
                );
                let control = events
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Control(control) => Some(control),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(control.snapshot["agent_name"], "reviewer");
                assert_eq!(
                    control.model_context.as_ref().unwrap().layer,
                    chat_state::ControlContextLayer::AgentRole
                );
            })
            .await;
    }

    #[tokio::test]
    async fn agent_switch_rejects_every_non_idle_foreground_without_surface_append() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = super::super::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.state.lock().await.foreground = ForegroundState::Compaction;
                let surface_before = actor.chat_state_handle.get_conversation().await;
                let mut definition = agent::AgentDefinition::default_grow_build();
                definition.name = "reviewer".into();

                let error = actor
                    .handle_rebuild_agent_for_definition(definition)
                    .await
                    .unwrap_err();
                assert!(format!("{error:?}").contains("foreground active"));
                assert_eq!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(surface_before).unwrap()
                );
                assert_ne!(actor.agent.borrow().name(), "reviewer");
            })
            .await;
    }

    #[tokio::test]
    async fn shadowed_agent_role_is_reprojected_before_the_next_sample() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = super::super::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                let (_, revision) = actor
                    .chat_state_handle
                    .get_conversation_with_revision()
                    .await
                    .unwrap();
                actor
                    .chat_state_handle
                    .replace_context_durably(
                        vec![ConversationItem::system("test system prompt")],
                        revision,
                    )
                    .await
                    .unwrap();
                let (agent_name, role_prompt) = {
                    let agent = actor.agent.borrow();
                    (
                        agent.name().to_owned(),
                        agent.role_prompt().map(str::to_owned),
                    )
                };
                actor
                    .persist_agent_transition_durably(&agent_name, role_prompt.as_deref())
                    .await
                    .unwrap();

                let (_, revision) = actor
                    .chat_state_handle
                    .get_conversation_with_revision()
                    .await
                    .unwrap();
                actor
                    .chat_state_handle
                    .replace_context_durably(
                        vec![ConversationItem::system("test system prompt")],
                        revision,
                    )
                    .await
                    .unwrap();
                actor
                    .repair_missing_control_contexts_durably()
                    .await
                    .unwrap();

                let surface = actor.chat_state_handle.get_conversation().await;
                assert!(
                    surface
                        .last()
                        .unwrap()
                        .text_content()
                        .contains("<agent-role>")
                );
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Control(chat_state::ControlEvent {
                                model_context: Some(chat_state::ControlContext {
                                    layer: chat_state::ControlContextLayer::AgentRole,
                                    activation: chat_state::ControlContextActivation::Reprojection,
                                    ..
                                }),
                                ..
                            })
                        ))
                        .count(),
                    1
                );
            })
            .await;
    }
}
