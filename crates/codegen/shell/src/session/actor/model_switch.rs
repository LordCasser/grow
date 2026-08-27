use super::*;
use crate::remote::DEFAULT_CONTEXT_WINDOW;
impl SessionActor {
    #[cfg(test)]
    pub(super) fn selection_route_for_test(
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        auto_compact_threshold_percent: u8,
    ) -> crate::agent::models::PublishedSessionRoute {
        let inference_idle_timeout =
            std::time::Duration::from_secs(sampling_config.idle_timeout_secs.unwrap_or(300));
        crate::agent::models::PublishedSessionRoute {
            model_id,
            max_retries: sampler::resolve_max_retries(sampling_config.max_retries),
            sampling_config,
            image_description_model: None,
            inference_idle_timeout,
            auto_compact_threshold_percent,
        }
    }

    #[cfg(test)]
    pub(super) fn published_catalog_for_test(
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        image_description_model: Option<String>,
        inference_idle_timeout: std::time::Duration,
        max_retries: u32,
        auto_compact_threshold_percent: u8,
    ) -> std::sync::Arc<crate::agent::models::PublishedModelCatalog> {
        let mut entry = crate::agent::config::ModelEntry::baseline(&sampling_config.model);
        entry.info.base_url = sampling_config.base_url;
        entry.info.output_limit = sampling_config.output_limit;
        entry.info.temperature = sampling_config.temperature;
        entry.info.top_p = sampling_config.top_p;
        entry.info.api_backend = sampling_config.api_backend;
        entry.info.auth_scheme = sampling_config.auth_scheme;
        entry.info.extra_headers = sampling_config.extra_headers;
        entry.info.query_params = sampling_config.query_params;
        entry.info.env_http_headers = sampling_config.env_http_headers;
        entry.info.context_window =
            std::num::NonZeroU64::new(sampling_config.context_window.max(1))
                .expect("test context window is non-zero");
        entry.info.inference_idle_timeout_secs = Some(inference_idle_timeout.as_secs());
        entry.info.max_retries = Some(max_retries);
        entry.info.auto_compact_threshold_percent = Some(auto_compact_threshold_percent);
        entry.info.stream_tool_calls = Some(sampling_config.stream_tool_calls);
        entry.info.compactions_remaining = sampling_config.compactions_remaining;
        entry.info.compaction_at_tokens = sampling_config.compaction_at_tokens;
        if let Some(effort) = sampling_config.reasoning_effort {
            entry.info.reasoning_efforts = vec![sampling_types::ReasoningEffortOption {
                id: effort.to_string(),
                value: effort,
                label: effort.to_string(),
                description: None,
                default: true,
            }];
        }
        entry.api_key = sampling_config.api_key;
        let mut models = indexmap::indexmap! { model_id.0.to_string() => entry };
        if let Some(auxiliary_id) = image_description_model.as_deref()
            && !models.contains_key(auxiliary_id)
        {
            models.insert(
                auxiliary_id.to_owned(),
                crate::agent::config::ModelEntry::baseline(auxiliary_id),
            );
        }
        let mut config = crate::agent::config::Config::default();
        config.image_description_model = image_description_model;
        let manager = crate::agent::models::ModelsManager::new(models, model_id, config);
        std::sync::Arc::new(manager.published_catalog())
    }

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
        catalog_authority: &crate::agent::models::PublishedModelCatalog,
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
        let next_run_route =
            crate::session::workflow::tracker::WorkflowRuntimeRoute::capture_from_catalog(
                model_id.0.to_string(),
                workflow_default_sampler,
                catalog_authority,
                alpha_test_key,
                self.agent_profile.subagent_filter(),
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
        self.rebuild_spec
            .context_window_tokens
            .store(context_window.get(), std::sync::atomic::Ordering::Relaxed);
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
        let agent_name = self.agent.borrow().definition().selector_identity();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id: model_id.clone(),
                agent_name: Some(agent_name),
                reasoning_effort: Some(sampling_config.reasoning_effort),
            });
        workflow_admission.set_next_run_route(next_run_route);
        // Catalog publication is global, but the final fallback/effort route is
        // actor-specific and is resolved only at this mailbox boundary. Always
        // publish that authoritative per-session result so reconnecting or
        // concurrently controlling clients cannot remain on a removed model.
        self.forward_grow_notification(
            self.build_grow_notification(
                GrowSessionUpdate::ModelChanged {
                    model_id: model_id.0.to_string(),
                    reasoning_effort: sampling_config
                        .reasoning_effort
                        .map(|effort| effort.to_string()),
                },
                None,
            ),
        )
        .await;
        Ok(())
    }

    async fn apply_published_model_catalog(
        &self,
        workflow_admission: &mut crate::session::workflow::manager::WorkflowManager,
        catalog: &crate::agent::models::PublishedModelCatalog,
    ) -> Result<(), acp::Error> {
        let current = self.model_route.snapshot();
        let route = catalog
            .resolve_session_route(&current.model_id, current.sampling_config.reasoning_effort)
            .ok_or_else(|| {
                acp::Error::internal_error().data(format!(
                    "catalog revision {} contains no routable session model",
                    catalog.revision
                ))
            })?;
        self.apply_model_config_reload(
            workflow_admission,
            catalog,
            route.model_id,
            route.sampling_config,
            route.image_description_model,
            route.inference_idle_timeout,
            route.max_retries,
            route.auto_compact_threshold_percent,
        )
        .await
    }

    /// Mailbox admission for catalog hot reload. The current step retains its
    /// exact provider route; adjacent busy reloads coalesce, but every user
    /// model, Agent or Goal selection is an ordering barrier in the unified
    /// step-control queue.
    pub(super) async fn admit_model_catalog_reload(
        &self,
        catalog: std::sync::Arc<crate::agent::models::PublishedModelCatalog>,
        responds_to: tokio::sync::oneshot::Sender<Result<(), acp::Error>>,
    ) {
        let mut admission = self.state.lock().await;
        if !admission.foreground.is_idle()
            && let Some(PendingStepControl::ModelReload(pending)) =
                admission.pending_step_controls.back_mut()
        {
            pending.catalog = catalog;
            pending.responders.push(responds_to);
            return;
        }
        admission
            .pending_step_controls
            .push_back(PendingStepControl::ModelReload(PendingModelReload {
                catalog,
                responders: vec![responds_to],
            }));
        let should_drain = admission.foreground.is_idle();
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if should_drain {
            self.drain_claimed_pending_step_controls().await;
        }
    }

    /// Apply every accepted selection before any idle consumer can admit the
    /// next prompt, compaction, notification, or Goal continuation.
    pub(super) async fn apply_pending_step_controls_if_idle(&self) {
        self.drain_pending_step_controls().await;
    }

    async fn apply_user_model_selection(
        &self,
        workflow_admission: &mut crate::session::workflow::manager::WorkflowManager,
        route: crate::agent::models::PublishedSessionRoute,
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
    ) -> Result<acp::ModelId, acp::Error> {
        let crate::agent::models::PublishedSessionRoute {
            model_id,
            sampling_config,
            image_description_model,
            inference_idle_timeout,
            max_retries,
            auto_compact_threshold_percent,
        } = route;
        let mut workflow_default_sampler = sampling_config.clone();
        workflow_default_sampler.idle_timeout_secs = Some(inference_idle_timeout.as_secs());
        workflow_default_sampler.max_retries = Some(max_retries);
        workflow_default_sampler.doom_loop_recovery = self.doom_loop_recovery;
        let alpha_test_key = self
            .chat_state_handle
            .get_credentials()
            .await
            .alpha_test_key;
        let next_run_route = if let Some(route) = &self.startup_hints.workflow_runtime_route {
            route.clone()
        } else {
            let catalog = catalog.ok_or_else(|| {
                acp::Error::invalid_request()
                    .data("ordinary model selection is missing its catalog authority")
            })?;
            crate::session::workflow::tracker::WorkflowRuntimeRoute::capture_from_catalog(
                model_id.0.to_string(),
                workflow_default_sampler,
                catalog.as_ref(),
                alpha_test_key,
                self.agent_profile.subagent_filter(),
            )
            .map_err(|error| acp::Error::invalid_request().data(error))?
        };
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
        let new_context_window_tokens = new_context_window.get();
        self.rebuild_spec.context_window_tokens.store(
            new_context_window_tokens,
            std::sync::atomic::Ordering::Relaxed,
        );
        let active_bridge = self.agent.borrow().tool_bridge().clone();
        active_bridge
            .set_context_window_tokens(new_context_window_tokens)
            .await;
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
        self.inference_idle_timeout.set(inference_idle_timeout);
        self.max_retries.set(max_retries);
        self.signals_handle()
            .set_tracing_config(inference_idle_timeout.as_secs());
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
        *self.image_description_model.write() = image_description_model;
        self.invalidate_model_auth_memo();
        self.signals_handle()
            .record_model_usage(model_id.0.as_ref());
        let agent_name = self.agent.borrow().definition().selector_identity();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::CurrentModel {
                model_id: model_id.clone(),
                agent_name: Some(agent_name),
                reasoning_effort: Some(sampling_config.reasoning_effort),
            });
        workflow_admission.set_next_run_route(next_run_route);
        self.forward_grow_notification(
            self.build_grow_notification(
                GrowSessionUpdate::ModelChanged {
                    model_id: model_id.0.to_string(),
                    reasoning_effort: sampling_config
                        .reasoning_effort
                        .map(|effort| effort.to_string()),
                },
                None,
            ),
        )
        .await;
        Ok(model_id)
    }
    /// Admit a user model/effort selection without mutating the active step.
    /// The response completes after the selection is durably applied at the
    /// next step boundary (or immediately while the session is idle).
    pub(super) async fn admit_session_model_selection(
        &self,
        route: crate::agent::models::PublishedSessionRoute,
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
        responds_to: tokio::sync::oneshot::Sender<Result<acp::ModelId, acp::Error>>,
    ) {
        let mut admission = self.state.lock().await;
        admission
            .pending_step_controls
            .push_back(PendingStepControl::ModelSelection(PendingModelSelection {
                route,
                catalog,
                responds_to,
            }));
        let should_drain = admission.foreground.is_idle();
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if should_drain {
            self.drain_claimed_pending_step_controls().await;
        }
    }

    /// Admit an Agent profile selection under the same step boundary as
    /// model/effort changes.
    pub(super) async fn admit_agent_selection(
        &self,
        definition: agent::AgentDefinition,
        responds_to: tokio::sync::oneshot::Sender<Result<(), acp::Error>>,
    ) {
        let preparation =
            if !self.startup_hints.is_subagent && !definition.is_primary_agent_eligible() {
                let issues = definition
                    .primary_agent_issues()
                    .into_iter()
                    .map(|issue| issue.message())
                    .collect::<Vec<_>>()
                    .join(", ");
                AgentPreparation::ready(Err(acp::Error::invalid_request().data(format!(
                    "Agent `{}` cannot own a primary session: {issues}",
                    definition.selector_identity()
                ))))
            } else {
                AgentPreparation::start(
                    self.rebuild_spec.clone(),
                    definition,
                    self.session_id_string(),
                )
            };
        let mut admission = self.state.lock().await;
        admission
            .pending_step_controls
            .push_back(PendingStepControl::AgentSelection(PendingAgentSelection {
                preparation,
                responds_to,
            }));
        let should_drain = admission.foreground.is_idle();
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if should_drain {
            self.drain_claimed_pending_step_controls().await;
        }
    }

    /// Admit a Goal-definition mutation into the same FIFO as model, effort
    /// and Agent changes. An active turn receives an immediate "scheduled"
    /// acknowledgement: waiting for StepEnded in the actor mailbox would
    /// prevent Stop and descendant usage settlements from making progress.
    /// Idle sessions claim the foreground and may wait for the immediate
    /// durable application result.
    pub(super) async fn admit_goal_definition_control(
        &self,
        goal_id: String,
        mutation: PendingGoalDefinitionMutation,
    ) -> Result<Option<bool>, String> {
        let mut admission = self.state.lock().await;
        let should_drain = admission.foreground.is_idle();
        let (responds_to, applied) = if should_drain {
            let (tx, rx) = tokio::sync::oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        admission
            .pending_step_controls
            .push_back(PendingStepControl::GoalDefinition(
                PendingGoalDefinitionControl {
                    goal_id,
                    mutation,
                    responds_to,
                },
            ));
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if should_drain {
            self.drain_claimed_pending_step_controls().await;
        }
        match applied {
            Some(applied) => applied
                .await
                .map_err(|_| "Goal step control ended without an application result.".to_string())?
                .map(Some),
            None => Ok(None),
        }
    }

    /// Apply every model/Agent/Goal control accepted during the completed step
    /// in FIFO order. The foreground remains owned by the same turn, so
    /// prompts, compaction, notification drains and Goal continuations stay
    /// fenced.
    /// Cancellation serializes on `step_control_gate` and therefore cannot
    /// tear a durable transition away from its live-state swap.
    pub(super) async fn apply_pending_controls_at_step_boundary(&self) -> (bool, bool, bool) {
        let mut model_changed = false;
        let mut agent_changed = false;
        let mut behavior_changed = false;
        loop {
            let preparation = {
                let admission = self.state.lock().await;
                if !matches!(&admission.foreground, ForegroundState::RegularTurn(_)) {
                    return (model_changed, agent_changed, behavior_changed);
                }
                match admission.pending_step_controls.front() {
                    Some(PendingStepControl::AgentSelection(pending)) => {
                        Some(std::rc::Rc::clone(&pending.preparation))
                    }
                    _ => None,
                }
            };
            let workspace_binding = if let Some(preparation) = preparation {
                // Filesystem/plugin/skill discovery is deliberately outside
                // the cancellation-critical step gate. If Stop aborts this
                // turn while preparation is pending, the queue retains the
                // result and the idle control drain applies it exactly once.
                preparation.wait_ready().await;
                if preparation.has_agent() {
                    self.prepare_agent_workspace_binding().await.map(Some)
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            };
            let _gate = self.step_control_gate.lock().await;
            let control = {
                let mut admission = self.state.lock().await;
                if !matches!(&admission.foreground, ForegroundState::RegularTurn(_)) {
                    return (model_changed, agent_changed, behavior_changed);
                }
                admission.pending_step_controls.pop_front()
            };
            let Some(control) = control else {
                return (model_changed, agent_changed, behavior_changed);
            };
            let (
                applied_model,
                applied_agent,
                applied_behavior,
                retired_goal_owner,
                deferred_error,
            ) = self.apply_pending_control(control, workspace_binding).await;
            drop(_gate);
            if let Some((goal_id, definition_revision)) = retired_goal_owner {
                self.cancel_goal_owned_work(&goal_id, definition_revision)
                    .await;
            }
            if let Some(error) = deferred_error {
                self.send_host_turn_slash_command_output(&format!(
                    "A scheduled Goal definition change failed at the step boundary: {error}"
                ))
                .await;
            }
            model_changed |= applied_model;
            agent_changed |= applied_agent;
            behavior_changed |= applied_behavior;
        }
    }

    async fn apply_pending_control(
        &self,
        control: PendingStepControl,
        workspace_binding: Result<Option<workspace::PreparedLocalSessionBind>, acp::Error>,
    ) -> (bool, bool, bool, Option<(String, u64)>, Option<String>) {
        match control {
            PendingStepControl::ModelReload(pending) => {
                let mut workflow_admission = self.workflow_manager.lock().await;
                let result = self
                    .apply_published_model_catalog(
                        &mut workflow_admission,
                        pending.catalog.as_ref(),
                    )
                    .await;
                let applied = result.is_ok();
                for respond_to in pending.responders {
                    let response = match &result {
                        Ok(()) => Ok(()),
                        Err(error) => Err(error.clone()),
                    };
                    let _ = respond_to.send(response);
                }
                (applied, false, false, None, None)
            }
            PendingStepControl::ModelSelection(pending) => {
                let mut workflow_admission = self.workflow_manager.lock().await;
                let result = self
                    .apply_user_model_selection(
                        &mut workflow_admission,
                        pending.route,
                        pending.catalog,
                    )
                    .await;
                let applied = result.is_ok();
                let _ = pending.responds_to.send(result);
                (applied, false, false, None, None)
            }
            PendingStepControl::AgentSelection(pending) => {
                let result = match (pending.preparation.take(), workspace_binding) {
                    (Ok(agent), Ok(Some(binding))) => {
                        self.apply_prepared_agent(agent, binding).await
                    }
                    (Ok(_), Ok(None)) => Err(acp::Error::internal_error()
                        .data("Agent activation was not given a prepared workspace binding")),
                    (Ok(_), Err(error)) | (Err(error), _) => Err(error),
                };
                let applied = result.as_ref().is_ok_and(|applied| *applied);
                let _ = pending.responds_to.send(result.map(|_| ()));
                (false, applied, false, None, None)
            }
            PendingStepControl::GoalDefinition(pending) => {
                let result = self.apply_pending_goal_definition_control(&pending).await;
                let (retired_goal_owner, behavior_changed) = match &result {
                    Ok((_, retired_goal_owner, behavior_changed)) => {
                        (retired_goal_owner.clone(), *behavior_changed)
                    }
                    Err(_) => (None, false),
                };
                let deferred_error = result
                    .as_ref()
                    .err()
                    .cloned()
                    .filter(|_| pending.responds_to.as_ref().is_none());
                if let Some(respond_to) = pending.responds_to {
                    let _ = respond_to.send(result.map(|(changed, _, _)| changed));
                }
                (
                    false,
                    false,
                    behavior_changed,
                    retired_goal_owner,
                    deferred_error,
                )
            }
        }
    }

    async fn drain_pending_step_controls(&self) {
        {
            let _gate = self.step_control_gate.lock().await;
            let mut admission = self.state.lock().await;
            if !admission.foreground.is_idle() || admission.pending_step_controls.is_empty() {
                return;
            }
            admission.foreground = ForegroundState::ApplyingControl;
        }
        self.drain_claimed_pending_step_controls().await;
    }

    /// Drain a queue whose caller atomically changed idle foreground ownership
    /// to `ApplyingControl` while admitting the first control. This closes the
    /// enqueue-to-drain race in which a prompt or Goal continuation could
    /// otherwise claim the old route.
    async fn drain_claimed_pending_step_controls(&self) {
        loop {
            let preparation = {
                let admission = self.state.lock().await;
                match admission.pending_step_controls.front() {
                    Some(PendingStepControl::AgentSelection(pending)) => {
                        Some(std::rc::Rc::clone(&pending.preparation))
                    }
                    _ => None,
                }
            };
            let workspace_binding = if let Some(preparation) = preparation {
                preparation.wait_ready().await;
                if preparation.has_agent() {
                    self.prepare_agent_workspace_binding().await.map(Some)
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            };
            let _gate = self.step_control_gate.lock().await;
            let mut admission = self.state.lock().await;
            debug_assert!(matches!(
                admission.foreground,
                ForegroundState::ApplyingControl
            ));
            let Some(control) = admission.pending_step_controls.pop_front() else {
                admission.foreground = ForegroundState::Idle;
                return;
            };
            drop(admission);
            let (_, _, _, retired_goal_owner, deferred_error) =
                self.apply_pending_control(control, workspace_binding).await;
            drop(_gate);
            if let Some((goal_id, definition_revision)) = retired_goal_owner {
                self.cancel_goal_owned_work(&goal_id, definition_revision)
                    .await;
            }
            if let Some(error) = deferred_error {
                self.send_host_turn_slash_command_output(&format!(
                    "A scheduled Goal definition change failed at the step boundary: {error}"
                ))
                .await;
            }
        }
    }

    /// Apply an admitted Agent selection while the caller owns the idle
    /// foreground fence.
    ///
    /// Builds a fresh [`agent::Agent`] from the cached
    /// [`crate::session::agent_rebuild::AgentRebuildSpec`] + the supplied
    /// [`agent::AgentDefinition`], replaces `self.agent`,
    /// commits the rendered role as an append-only Timeline Control fact, and
    /// only then replaces the live harness. Agent selection is independent
    /// from model selection and preserves the current sampling configuration.
    /// An idle caller keeps [`ForegroundState::ApplyingControl`] as the
    /// exclusive admission fence; an active turn holds `step_control_gate`.
    async fn apply_agent_definition(
        &self,
        definition: agent::AgentDefinition,
    ) -> Result<bool, acp::Error> {
        if !self.startup_hints.is_subagent && !definition.is_primary_agent_eligible() {
            let issues = definition
                .primary_agent_issues()
                .into_iter()
                .map(|issue| issue.message())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(acp::Error::invalid_request().data(format!(
                "Agent `{}` cannot own a primary session: {issues}",
                definition.selector_identity()
            )));
        }
        let new_agent_name = definition.selector_identity();
        let new_agent = self
            .rebuild_spec
            .build_agent(definition)
            .await
            .map_err(|error| {
                tracing::error!(
                    session_id = %self.session_info.id.0,
                    new_agent_type = %new_agent_name,
                    %error,
                    "Agent preparation failed"
                );
                acp::Error::internal_error().data(format!(
                    "rebuild_agent: build failed for agent_type={new_agent_name}: {error}"
                ))
            })?;
        let workspace_binding = self.prepare_agent_workspace_binding().await?;
        self.apply_prepared_agent(new_agent, workspace_binding)
            .await
    }

    async fn prepare_agent_workspace_binding(
        &self,
    ) -> Result<workspace::PreparedLocalSessionBind, acp::Error> {
        self.workspace_ops
            .prepare_local_session_bind(
                &self.session_id_string(),
                self.tool_context.cwd.as_path().to_path_buf(),
                self.tool_context.hunk_tracker_handle.clone(),
                None,
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "Agent workspace binding could not be prepared: {error}"
                ))
            })
    }

    /// Commit a fully constructed candidate under the short causal control
    /// gate. Expensive filesystem/plugin/skill discovery has already finished.
    async fn apply_prepared_agent(
        &self,
        mut new_agent: agent::Agent,
        workspace_binding: workspace::PreparedLocalSessionBind,
    ) -> Result<bool, acp::Error> {
        let definition = new_agent.definition();
        let new_agent_name = definition.selector_identity();
        let new_subagent_filter = definition.subagent_filter();
        let new_mcp_inheritance = definition.mcp_inheritance.clone();
        let current_route = self.model_route.snapshot();
        let current_sampling = current_route.sampling_config;
        let current_model = current_route.model_id;
        let current_bridge = self.agent.borrow().tool_bridge().clone();
        let current_goal_context = current_bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalContextSnapshotResource>()
            .await;
        let current_prompt_id = current_bridge
            .read_resource::<tools::implementations::grow_build::task::types::CurrentPromptIdResource>()
            .await;
        let current_subagent_owner = current_bridge
            .read_resource::<tools::implementations::grow_build::task::types::CurrentSubagentOwnerResource>()
            .await;
        let current_goal_delegation = current_bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalDelegationSnapshotResource>()
            .await;
        let current_goal_authority = current_bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalMutationAuthorityResource>()
            .await;
        let already_applied = {
            let current = self.agent.borrow();
            current
                .definition()
                .runtime_equivalent(new_agent.definition())
                && current.role_prompt() == new_agent.role_prompt()
        };
        if already_applied {
            // Reconnect is at-least-once: a rearmed Agent control may be
            // replayed after the Shell applied it but before Pager observed
            // the terminal. Compare fully built profiles so skill discovery,
            // AGENTS.md, templates, and runtime tool projection participate.
            // An exact match is an acknowledgement, not a second AgentRole
            // fact or harness replacement; same-name changed definitions fall
            // through and rebuild exactly once.
            self.forward_grow_notification(self.build_grow_notification(
                GrowSessionUpdate::AgentChanged {
                    agent_name: new_agent_name,
                },
                None,
            ))
            .await;
            return Ok(false);
        }
        let candidate_bridge = new_agent.tool_bridge().clone();
        if let Some(goal_context) = current_goal_context {
            candidate_bridge.update_resource(goal_context).await;
        }
        if let Some(prompt_id) = current_prompt_id {
            candidate_bridge.update_resource(prompt_id).await;
        }
        if let Some(owner) = current_subagent_owner {
            candidate_bridge.update_resource(owner).await;
        }
        if let Some(delegation) = current_goal_delegation {
            candidate_bridge.update_resource(delegation).await;
        }
        if let Some(authority) = current_goal_authority {
            candidate_bridge.update_resource(authority).await;
        }
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
        let authored_capability_tools = new_agent
            .definition()
            .authored_capability_tools
            .as_ref()
            .cloned()
            .unwrap_or_else(|| new_agent.definition().tool_config.clone());
        let candidate_capability_catalog =
            self.subagent_capabilities.as_ref().map(|capabilities| {
                capabilities.preview_native_catalog_prompt(
                    new_agent.tool_bridge(),
                    &authored_capability_tools,
                )
            });
        let candidate_mcp_bindings = if self.subagent_capabilities.is_some() {
            Some(
                crate::session::subagent_capability::project_agent_mcp_bindings(
                    &new_mcp_inheritance,
                    self.mcp_state.lock().await.shared_client_ids(),
                ),
            )
        } else {
            None
        };
        drop(candidate_bridge);
        let resource_activation = new_agent
            .prepare_resource_domain_activation(&self.rebuild_spec.resource_domain)
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "rebuilt Agent resources could not be staged for the session domain: {error}"
                ))
            })?;
        self.persist_agent_transition_durably(
            &new_agent_name,
            new_agent.role_prompt(),
            candidate_capability_catalog.as_deref(),
        )
        .await
        .map_err(|error| {
            acp::Error::internal_error().data(format!(
                "rebuilt agent role was not durably recorded: {error}"
            ))
        })?;
        resource_activation.commit().await;
        if let Some(capabilities) = &self.subagent_capabilities {
            capabilities.replace_agent_harness(
                new_agent.tool_bridge(),
                &authored_capability_tools,
                candidate_mcp_bindings.unwrap_or_default(),
            );
        }
        *self.agent.borrow_mut() = new_agent;
        // Keep foreground admission fenced until the new harness has every
        // critical runtime resource and MCP registration. Goal idle-driving is
        // detached from the command mailbox and could otherwise admit a turn
        // against a half-rebound Agent.
        workspace_binding.commit(self.agent.borrow().tool_bridge().toolset());
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
        // Restore the already-observed catalog using local metadata only.
        // Remote list calls do not belong inside the step-control/cancel gate.
        self.restore_mcp_tools_from_snapshot().await;
        self.agent_profile
            .replace(new_agent_name.clone(), new_subagent_filter);
        self.workflow_manager
            .lock()
            .await
            .set_next_run_agent_profile(
                new_agent_name.clone(),
                self.agent_profile.subagent_filter(),
            );
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
        self.forward_grow_notification(self.build_grow_notification(
            GrowSessionUpdate::AgentChanged {
                agent_name: new_agent_name.clone(),
            },
            None,
        ))
        .await;
        tracing::info!(
            session_id = %self.session_info.id.0,
            new_agent_type = %new_agent_name,
            "apply_agent_definition: harness rebuild complete"
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_credential_refresh_cannot_cross_route_revision() {
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
                let old_route = actor.model_route.snapshot();
                let previous_key = actor.chat_state_handle.get_credentials().await.api_key;
                actor.model_route.replace(
                    acp::ModelId::new("provider/new-model"),
                    old_route.sampling_config,
                );

                assert!(
                    !actor
                        .set_chat_api_key(
                            "fixture-credential".to_owned(),
                            Some(old_route.revision),
                        )
                        .await
                );
                assert_eq!(
                    actor.chat_state_handle.get_credentials().await.api_key,
                    previous_key
                );
            })
            .await;
    }

    #[tokio::test]
    async fn sampler_rebuild_keeps_frozen_route_auth_axes() {
        #[derive(Debug)]
        struct EmptyBearerResolver;
        impl sampler::BearerResolver for EmptyBearerResolver {
            fn current_bearer(&self) -> Option<String> {
                None
            }
        }

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
                let mut route = actor.model_route.snapshot().sampling_config;
                route.auth_scheme = sampler::AuthScheme::XApiKey;
                route.bearer_resolver = Some(std::sync::Arc::new(EmptyBearerResolver));
                actor
                    .model_route
                    .replace(acp::ModelId::new("removed-provider/frozen-model"), route);

                let rebuilt = actor.reconstruct_full_config().await;

                assert_eq!(rebuilt.auth_scheme, sampler::AuthScheme::XApiKey);
                assert!(rebuilt.bearer_resolver.is_some());
            })
            .await;
    }

    #[tokio::test]
    async fn current_model_surfaces_use_provider_qualified_identity() {
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
                let mut route = actor.model_route.snapshot().sampling_config;
                route.model = "glm-5.3".into();
                actor
                    .model_route
                    .replace(acp::ModelId::new("bigmodel/glm-5.3"), route);

                assert_eq!(actor.current_catalog_model_id(), "bigmodel/glm-5.3");
                assert_eq!(
                    actor.build_session_info().await.model.as_deref(),
                    Some("bigmodel/glm-5.3")
                );
            })
            .await;
    }

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
                let first_catalog = SessionActor::published_catalog_for_test(
                    acp::ModelId::new("first/catalog"),
                    first,
                    None,
                    std::time::Duration::from_secs(60),
                    3,
                    85,
                );
                let (first_tx, mut first_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_model_catalog_reload(first_catalog, first_tx)
                    .await;

                let mut latest = sampler::SamplerConfig::default();
                latest.model = "latest-wire".into();
                latest.base_url = "https://latest.example/v1".into();
                latest.context_window = 64_000;
                let latest_catalog = SessionActor::published_catalog_for_test(
                    acp::ModelId::new("latest/catalog"),
                    latest,
                    None,
                    std::time::Duration::from_secs(90),
                    4,
                    75,
                );
                let (latest_tx, mut latest_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_model_catalog_reload(latest_catalog, latest_tx)
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
                actor.apply_pending_step_controls_if_idle().await;
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
    async fn user_selection_is_an_ordering_barrier_between_catalog_generations() {
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

                let mut first_reload = sampler::SamplerConfig::default();
                first_reload.model = "reload-one-wire".into();
                first_reload.context_window = 32_000;
                let first_catalog = SessionActor::published_catalog_for_test(
                    acp::ModelId::new("reload/one"),
                    first_reload,
                    None,
                    std::time::Duration::from_secs(60),
                    3,
                    85,
                );
                let (first_tx, first_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_model_catalog_reload(first_catalog, first_tx)
                    .await;

                let mut selected = sampler::SamplerConfig::default();
                selected.model = "user-wire".into();
                selected.context_window = 64_000;
                let (selection_tx, selection_rx) = tokio::sync::oneshot::channel();
                let selection_catalog = SessionActor::published_catalog_for_test(
                    acp::ModelId::new("user/selection"),
                    selected.clone(),
                    None,
                    std::time::Duration::from_secs(300),
                    3,
                    80,
                );
                actor
                    .admit_session_model_selection(
                        SessionActor::selection_route_for_test(
                            acp::ModelId::new("user/selection"),
                            selected,
                            80,
                        ),
                        Some(selection_catalog),
                        selection_tx,
                    )
                    .await;

                let mut second_reload = sampler::SamplerConfig::default();
                second_reload.model = "user-wire-refreshed".into();
                second_reload.context_window = 96_000;
                let second_catalog = SessionActor::published_catalog_for_test(
                    acp::ModelId::new("user/selection"),
                    second_reload,
                    None,
                    std::time::Duration::from_secs(120),
                    5,
                    70,
                );
                let (second_tx, second_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_model_catalog_reload(second_catalog, second_tx)
                    .await;

                actor.state.lock().await.foreground = ForegroundState::Idle;
                actor.apply_pending_step_controls_if_idle().await;
                first_rx.await.unwrap().unwrap();
                selection_rx.await.unwrap().unwrap();
                second_rx.await.unwrap().unwrap();
                let route = actor.model_route.snapshot();
                assert_eq!(route.model_id.0.as_ref(), "user/selection");
                assert_eq!(route.sampling_config.model, "user-wire-refreshed");
                assert_eq!(actor.inference_idle_timeout.get().as_secs(), 120);
                assert_eq!(actor.max_retries.get(), 5);
                assert_eq!(actor.compaction.threshold_percent.get(), 70);
            })
            .await;
    }

    #[tokio::test]
    async fn primary_agent_switch_rejects_subagent_only_definition() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                let original = actor.agent.borrow().definition().selector_identity();
                let error = actor
                    .apply_agent_definition(agent::AgentDefinition::explore())
                    .await
                    .expect_err("subagent-only Agent cannot own the primary session");
                assert!(
                    error.to_string().contains("declared subagentOnly"),
                    "unexpected rejection: {error:?}"
                );
                assert_eq!(
                    actor.agent.borrow().definition().selector_identity(),
                    original
                );
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
                let (initial_bridge, initial_authored_tools) = {
                    let agent = actor.agent.borrow();
                    (
                        agent.tool_bridge().clone(),
                        agent
                            .definition()
                            .authored_capability_tools
                            .clone()
                            .unwrap_or_else(|| agent.definition().tool_config.clone()),
                    )
                };
                actor.subagent_capabilities = Some(
                    crate::session::subagent_capability::SubagentCapabilityState::from_bridge(
                        &initial_bridge,
                        &initial_authored_tools,
                        tool_types::SubagentCapabilityMode::ReadOnly,
                        None,
                        Default::default(),
                    )
                    .await,
                );
                initial_bridge
                    .update_resource(
                        tools::implementations::grow_build::update_goal::GoalContextSnapshotResource(
                            Some(
                                tools::implementations::grow_build::update_goal::GoalContextSnapshot {
                                    view: tools::implementations::grow_build::update_goal::GoalView {
                                        goal_id: "goal-owner".into(),
                                        definition_revision: 3,
                                        objective: "preserve delegated ownership".into(),
                                        status: "active".into(),
                                        token_budget: None,
                                        tokens_used: 0,
                                        usage_incomplete: false,
                                        elapsed_ms: 0,
                                        created_at: "now".into(),
                                        updated_at: "now".into(),
                                        status_message: None,
                                    },
                                },
                            ),
                        ),
                    )
                    .await;
                let mut route = actor.model_route.snapshot().sampling_config;
                route.model = "provider-alias".into();
                actor
                    .model_route
                    .replace(acp::ModelId::new("catalog-alias"), route);
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
                let replay_definition = definition.clone();
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
                let (responds_to, response) = tokio::sync::oneshot::channel();
                actor.admit_agent_selection(definition, responds_to).await;
                response.await.unwrap().unwrap();

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
                assert!(
                    surface
                        .last()
                        .unwrap()
                        .text_content()
                        .contains("<subagent-capability-catalog>"),
                    "the active AgentRole must carry the matching child capability projection"
                );
                assert_eq!(actor.agent.borrow().name(), "reviewer");
                let rebuilt_bridge = actor.agent.borrow().tool_bridge().clone();
                let inherited_goal = rebuilt_bridge
                    .read_resource::<tools::implementations::grow_build::update_goal::GoalContextSnapshotResource>()
                    .await
                    .and_then(|resource| resource.0)
                    .expect("Agent rebuild must retain delegated Goal ownership");
                assert_eq!(inherited_goal.view.goal_id, "goal-owner");
                assert_eq!(inherited_goal.view.definition_revision, 3);
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
                    control.model_contexts.first().unwrap().layer,
                    chat_state::ControlContextLayer::AgentRole
                );

                let control_count_before = events
                    .iter()
                    .filter(|event| matches!(event.kind, chat_state::TimelineEventKind::Control(_)))
                    .count();
                let (responds_to, response) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(replay_definition, responds_to)
                    .await;
                response.await.unwrap().unwrap();
                let control_count_after = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|event| matches!(event.kind, chat_state::TimelineEventKind::Control(_)))
                    .count();
                assert_eq!(
                    control_count_after, control_count_before,
                    "an exact at-least-once Agent replay must not append a second AgentRole"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn busy_agent_switch_applies_after_the_active_step() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, mut gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let mut actor = super::super::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.goal_enabled = true;
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                super::super::tests::support::begin_test_active_causal_turn(&actor).await;
                let surface_before = actor.chat_state_handle.get_conversation().await;
                let runtime_profile_clone = actor.agent_profile.clone();
                let mut definition = agent::AgentDefinition::default_grow_build();
                definition.name = "reviewer".into();
                definition.subagents.deny = vec!["blocked-child".into()];

                let (responds_to, mut response) = tokio::sync::oneshot::channel();
                actor.admit_agent_selection(definition, responds_to).await;
                assert!(matches!(
                    response.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                assert_eq!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(surface_before).unwrap()
                );
                assert_ne!(actor.agent.borrow().name(), "reviewer");

                assert!(actor.events.end_step("continued"));
                assert_eq!(
                    actor.apply_pending_controls_at_step_boundary().await,
                    (false, true, false)
                );
                response.await.unwrap().unwrap();
                assert_eq!(actor.agent.borrow().name(), "reviewer");
                assert_eq!(runtime_profile_clone.name(), "reviewer");
                assert!(
                    !runtime_profile_clone
                        .subagent_filter()
                        .allows("blocked-child"),
                    "every pre-existing child runtime handle clone must observe the new filter"
                );
                let agent_changed = std::iter::from_fn(|| gateway_rx.try_recv().ok()).any(|msg| {
                    let acp_transport::AcpClientMessage::ExtNotification(args) = msg else {
                        return false;
                    };
                    if args.request.method.as_ref() != "grow/session_notification" {
                        return false;
                    }
                    serde_json::from_str::<crate::extensions::notification::SessionNotification>(
                        args.request.params.get(),
                    )
                    .is_ok_and(|notification| {
                        matches!(
                            notification.update,
                            GrowSessionUpdate::AgentChanged { agent_name }
                                if agent_name == "reviewer"
                        )
                    })
                });
                assert!(
                    agent_changed,
                    "actor commit must publish AgentChanged before ack"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn busy_model_and_effort_switch_applies_after_the_active_step() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, mut gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
                let mut actor = super::super::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.goal_enabled = true;
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                super::super::tests::support::begin_test_active_causal_turn(&actor).await;
                let previous = actor.model_route.snapshot();
                let (responds_to, mut response) = tokio::sync::oneshot::channel();
                let mut selected_entry =
                    crate::agent::config::ModelEntry::baseline("next-wire-model");
                selected_entry.info.context_window = std::num::NonZeroU64::new(64_000).unwrap();
                selected_entry.info.inference_idle_timeout_secs = Some(77);
                selected_entry.info.max_retries = Some(7);
                selected_entry.info.auto_compact_threshold_percent = Some(72);
                selected_entry.info.reasoning_efforts =
                    vec![sampling_types::ReasoningEffortOption {
                        id: "high".to_owned(),
                        value: sampling_types::ReasoningEffort::High,
                        label: "High".to_owned(),
                        description: None,
                        default: true,
                    }];
                let vision_entry = crate::agent::config::ModelEntry::baseline("vision-wire-model");
                let mut config = crate::agent::config::Config::default();
                config.image_description_model = Some("catalog/vision".to_owned());
                let selected_manager = crate::agent::models::ModelsManager::new(
                    indexmap::IndexMap::from([
                        ("catalog/next".to_owned(), selected_entry),
                        ("catalog/vision".to_owned(), vision_entry),
                    ]),
                    acp::ModelId::new("catalog/next"),
                    config,
                );
                let selection_catalog = std::sync::Arc::new(selected_manager.published_catalog());
                let selected_route = selection_catalog
                    .resolve_session_route(
                        &acp::ModelId::new("catalog/next"),
                        Some(sampling_types::ReasoningEffort::High),
                    )
                    .unwrap();

                actor
                    .admit_session_model_selection(
                        selected_route,
                        Some(selection_catalog),
                        responds_to,
                    )
                    .await;
                let mut next_agent = agent::AgentDefinition::default_grow_build();
                next_agent.name = "step-reviewer".into();
                let (agent_responds_to, mut agent_response) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(next_agent, agent_responds_to)
                    .await;

                assert!(matches!(
                    response.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                assert!(matches!(
                    agent_response.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                let during = actor.model_route.snapshot();
                assert_eq!(during.revision, previous.revision);
                assert_eq!(during.model_id, previous.model_id);

                assert!(actor.events.end_step("continued"));
                assert_eq!(
                    actor.apply_pending_controls_at_step_boundary().await,
                    (true, true, false)
                );
                assert_eq!(response.await.unwrap().unwrap().0.as_ref(), "catalog/next");
                agent_response.await.unwrap().unwrap();
                let applied = actor.model_route.snapshot();
                assert_eq!(applied.model_id.0.as_ref(), "catalog/next");
                assert_eq!(
                    applied.sampling_config.reasoning_effort,
                    Some(sampling_types::ReasoningEffort::High)
                );
                assert_eq!(actor.compaction.threshold_percent.get(), 72);
                assert_eq!(actor.inference_idle_timeout.get().as_secs(), 77);
                assert_eq!(actor.max_retries.get(), 7);
                assert_eq!(
                    actor.image_description_model.read().as_deref(),
                    Some("catalog/vision")
                );
                assert_eq!(
                    actor
                        .rebuild_spec
                        .context_window_tokens
                        .load(std::sync::atomic::Ordering::Relaxed),
                    64_000,
                    "a later Agent rebuild must use the newly committed model window"
                );
                assert_eq!(actor.agent.borrow().name(), "step-reviewer");
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let step_ended = events
                    .iter()
                    .position(|event| {
                        matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Step(chat_state::StepEvent::Ended { .. })
                        )
                    })
                    .expect("completed step must be recorded");
                let model_changed_event = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Observation(observation)
                                if observation.scope == crate::session::persistence::MODEL_CHANGE_SCOPE
                                    && observation.name == crate::session::persistence::MODEL_CHANGE_NAME
                        )
                    })
                    .expect("model transition must be recorded");
                let agent_changed_event = events
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Control(control)
                                if matches!(control.model_contexts.as_slice(), [context]
                                    if context.layer
                                        == chat_state::ControlContextLayer::AgentRole)
                        )
                    })
                    .expect("Agent transition must be recorded");
                assert!(
                    step_ended < model_changed_event && model_changed_event < agent_changed_event,
                    "FIFO controls must follow StepEnded in their admitted order"
                );
                let model_changed = std::iter::from_fn(|| gateway_rx.try_recv().ok()).any(|msg| {
                    let acp_transport::AcpClientMessage::ExtNotification(args) = msg else {
                        return false;
                    };
                    if args.request.method.as_ref() != "grow/session_notification" {
                        return false;
                    }
                    serde_json::from_str::<crate::extensions::notification::SessionNotification>(
                        args.request.params.get(),
                    )
                    .is_ok_and(|notification| {
                        matches!(
                            notification.update,
                            GrowSessionUpdate::ModelChanged { model_id, .. }
                                if model_id == "catalog/next"
                        )
                    })
                });
                assert!(
                    model_changed,
                    "actor commit must publish ModelChanged before ack"
                );
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
                        agent.definition().selector_identity(),
                        agent.role_prompt().map(str::to_owned),
                    )
                };
                actor
                    .persist_agent_transition_durably(&agent_name, role_prompt.as_deref(), None)
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
                            chat_state::TimelineEventKind::Control(control)
                                if matches!(control.model_contexts.as_slice(), [context]
                                    if context.layer
                                        == chat_state::ControlContextLayer::AgentRole
                                        && context.activation
                                            == chat_state::ControlContextActivation::Reprojection)
                        ))
                        .count(),
                    1
                );
            })
            .await;
    }

    #[tokio::test]
    async fn control_reprojection_preserves_cross_layer_causal_order() {
        tokio::task::LocalSet::new()
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

                actor
                    .persist_behavior_transition_durably(
                        crate::session::behavior::BehaviorSnapshot::normal(),
                        None,
                    )
                    .await
                    .unwrap();
                actor
                    .persist_agent_transition_durably(
                        "reviewer",
                        Some("reviewer role after behavior"),
                        None,
                    )
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
                let restored = surface
                    .iter()
                    .skip(1)
                    .map(ConversationItem::text_content)
                    .collect::<Vec<_>>();
                assert_eq!(restored.len(), 2);
                assert!(restored[0].contains("Normal Behavior is now active"));
                assert!(restored[1].contains("reviewer role after behavior"));

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let contexts = events
                    .iter()
                    .rev()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Control(control)
                            if control.model_contexts.iter().all(|context| {
                                context.activation
                                    == chat_state::ControlContextActivation::Reprojection
                            }) =>
                        {
                            Some(&control.model_contexts)
                        }
                        _ => None,
                    })
                    .expect("repair must commit one atomic reprojection");
                assert_eq!(contexts.len(), 2);
                assert_eq!(contexts[0].layer, chat_state::ControlContextLayer::Behavior);
                assert_eq!(
                    contexts[1].layer,
                    chat_state::ControlContextLayer::AgentRole
                );
            })
            .await;
    }
}
