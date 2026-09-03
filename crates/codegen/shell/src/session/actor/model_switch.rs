use super::*;
use crate::remote::DEFAULT_CONTEXT_WINDOW;
use futures_util::FutureExt as _;

/// A user-facing desired-state request is not complete until its immutable
/// terminal UI projection is durable. Keeping the responder beside the
/// application result prevents an RPC success/error from overtaking that
/// projection and becoming unrecoverable after a process crash.
enum PendingControlSettlement {
    Sampling {
        respond_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
        >,
        result: Result<crate::agent::models::ModelId, acp::Error>,
        intent: Option<crate::session::ControlIntent>,
    },
    Agent {
        respond_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<()>, acp::Error>,
        >,
        result: Result<bool, acp::Error>,
        intent: Option<crate::session::ControlIntent>,
    },
}

impl PendingControlSettlement {
    fn settle_fatal(self) {
        match self {
            Self::Sampling {
                respond_to, result, ..
            } => {
                let _ =
                    respond_to
                        .send(result.map(|model_id| {
                            crate::session::DesiredStateOutcome::Applied(model_id)
                        }));
            }
            Self::Agent {
                respond_to, result, ..
            } => {
                let _ = respond_to
                    .send(result.map(|_| crate::session::DesiredStateOutcome::Applied(())));
            }
        }
    }

    fn control_intent(
        &self,
    ) -> (
        crate::extensions::notification::ControlDomain,
        Option<&crate::session::ControlIntent>,
    ) {
        match self {
            Self::Sampling { intent, .. } => (
                crate::extensions::notification::ControlDomain::Sampling,
                intent.as_ref(),
            ),
            Self::Agent { intent, .. } => (
                crate::extensions::notification::ControlDomain::Agent,
                intent.as_ref(),
            ),
        }
    }

    fn terminal_result(&self) -> Result<(), String> {
        match self {
            Self::Sampling { result, .. } => {
                result.as_ref().map(|_| ()).map_err(ToString::to_string)
            }
            Self::Agent { result, .. } => result.as_ref().map(|_| ()).map_err(ToString::to_string),
        }
    }

    fn settle(self, terminal_append: Result<(), crate::session::persistence::DurableAppendError>) {
        let terminal_error = terminal_append.err().map(|error| {
            acp::Error::internal_error().data(format!(
                "control state changed, but its terminal UI event was not durably recorded: {error}"
            ))
        });
        match self {
            Self::Sampling {
                respond_to, result, ..
            } => {
                let response = if let Some(error) = terminal_error {
                    Err(error)
                } else {
                    result
                        .map(|model_id| crate::session::DesiredStateOutcome::Applied(model_id))
                        .map_err(crate::session::mark_control_terminal_published)
                };
                let _ = respond_to.send(response);
            }
            Self::Agent {
                respond_to, result, ..
            } => {
                let response = if let Some(error) = terminal_error {
                    Err(error)
                } else {
                    result
                        .map(|_| crate::session::DesiredStateOutcome::Applied(()))
                        .map_err(crate::session::mark_control_terminal_published)
                };
                let _ = respond_to.send(response);
            }
        }
    }
}

impl SessionActor {
    fn current_control_target(
        &self,
        domain: crate::extensions::notification::ControlDomain,
    ) -> crate::extensions::notification::ControlTarget {
        use crate::extensions::notification::ControlTarget;
        match domain {
            crate::extensions::notification::ControlDomain::Sampling => {
                let route = self.model_route.snapshot();
                ControlTarget::Sampling {
                    model_id: route.model_id.0.to_string(),
                    reasoning_effort: route
                        .sampling_config
                        .reasoning_effort
                        .map(|effort| effort.to_string()),
                }
            }
            crate::extensions::notification::ControlDomain::Agent => ControlTarget::Agent {
                agent_name: self.agent.borrow().definition().selector_identity(),
            },
            crate::extensions::notification::ControlDomain::Behavior => ControlTarget::Behavior {
                behavior_id: self.behavior.lock().behavior().as_id().to_owned(),
            },
        }
    }

    pub(super) async fn publish_control_projection(
        &self,
        projection: &StepControlProjection,
        phase: crate::extensions::notification::ControlPhase,
        message: Option<String>,
        durable: bool,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        // Control feedback belongs to a client-authored intent. Session load,
        // startup routing, and automatic recovery reuse the same atomic
        // transition machinery with no intent; publishing those projections
        // would turn state hydration into a fake user-visible switch on every
        // resume. Their authoritative values are projected separately through
        // ModelChanged, AgentChanged, CurrentModeUpdate, and load snapshots.
        if projection.intent.is_none() {
            return Ok(());
        }
        let domain = projection.target.domain();
        let update = GrowSessionUpdate::ControlStateUpdate(
            crate::extensions::notification::ControlStateUpdate {
                epoch: self.control_epoch.clone(),
                domain,
                revision: projection.revision,
                intent: projection.intent.clone(),
                snapshot: false,
                receipt_only: false,
                phase,
                current: self.current_control_target(domain),
                desired: Some(projection.target.clone()),
                message,
            },
        );
        if durable {
            self.send_grow_passive_notification(update.clone(), update)
                .await
        } else {
            self.send_transient_grow_notification(update).await;
            Ok(())
        }
    }

    pub(super) async fn recover_missing_terminal_projection(
        &self,
        domain: crate::extensions::notification::ControlDomain,
        intent: &crate::session::ControlIntent,
        terminal: &ControlIntentTerminal,
        revision: u64,
    ) -> Result<(), acp::Error> {
        if terminal.ui_terminal_durable {
            return Ok(());
        }
        let message = terminal.message.clone().or_else(|| {
            (terminal.phase == crate::extensions::notification::ControlPhase::Applied)
                .then(|| Self::control_terminal_message(&terminal.target, &Ok(())))
        });
        let update = GrowSessionUpdate::ControlStateUpdate(
            crate::extensions::notification::ControlStateUpdate {
                epoch: self.control_epoch.clone(),
                domain,
                revision,
                intent: Some(intent.clone()),
                snapshot: false,
                receipt_only: true,
                phase: terminal.phase,
                current: self.current_control_target(domain),
                desired: Some(terminal.target.clone()),
                message,
            },
        );
        self.send_grow_passive_notification(update.clone(), update)
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "the applied control was recovered, but its terminal UI event could not be repaired: {error}"
                ))
            })?;
        self.state
            .lock()
            .await
            .mark_control_terminal_ui_durable(domain, intent);
        Ok(())
    }

    pub(super) fn control_terminal_message(
        target: &crate::extensions::notification::ControlTarget,
        result: &Result<(), String>,
    ) -> String {
        use crate::extensions::notification::ControlTarget;
        let label = match target {
            ControlTarget::Sampling {
                model_id,
                reasoning_effort,
            } => match reasoning_effort {
                Some(effort) => format!("Sampling switched to {model_id} ({effort})"),
                None => format!("Sampling switched to {model_id}"),
            },
            ControlTarget::Agent { agent_name } => format!("Agent switched to {agent_name}"),
            ControlTarget::Behavior { behavior_id } => {
                format!("Behavior switched to {behavior_id}")
            }
        };
        match result {
            Ok(()) => label,
            Err(error) => format!("{label} failed: {error}"),
        }
    }

    /// Re-publish the current desired-state projection after a client load or
    /// TUI renderer restart. Pending state is reconstructed from actor state,
    /// never from Pager memory or replayed transient rows.
    pub(super) async fn publish_control_state_snapshot(&self) {
        use crate::extensions::notification::{ControlDomain, ControlPhase, ControlStateUpdate};
        let snapshots = {
            let admission = self.state.lock().await;
            [ControlDomain::Sampling, ControlDomain::Agent]
                .into_iter()
                .map(|domain| {
                    let pending = admission
                        .pending_step_controls
                        .domain_projection(domain)
                        .filter(|projection| projection.intent.is_some());
                    let applying = admission
                        .applying_step_control
                        .as_ref()
                        .filter(|projection| projection.target.domain() == domain)
                        .cloned();
                    let applying = applying.filter(|projection| projection.intent.is_some());
                    let (projection, phase) = match (pending, applying) {
                        (Some(pending), Some(applying))
                            if applying.revision >= pending.revision =>
                        {
                            (Some(applying), ControlPhase::Applying)
                        }
                        (Some(pending), _) => (Some(pending), ControlPhase::Pending),
                        (None, Some(applying)) => (Some(applying), ControlPhase::Applying),
                        (None, None) => (None, ControlPhase::Applied),
                    };
                    (
                        domain,
                        projection
                            .as_ref()
                            .map_or(admission.pending_step_controls.revision(domain), |p| {
                                p.revision
                            }),
                        phase,
                        projection,
                    )
                })
                .collect::<Vec<_>>()
        };
        for (domain, revision, phase, projection) in snapshots {
            self.send_transient_grow_notification(GrowSessionUpdate::ControlStateUpdate(
                ControlStateUpdate {
                    epoch: self.control_epoch.clone(),
                    domain,
                    revision,
                    intent: projection
                        .as_ref()
                        .and_then(|projection| projection.intent.clone()),
                    snapshot: true,
                    receipt_only: false,
                    phase,
                    current: self.current_control_target(domain),
                    desired: projection.map(|projection| projection.target),
                    message: None,
                },
            ))
            .await;
        }
        let (behavior_revision, behavior_phase, behavior_projection) = {
            let admission = self.state.lock().await;
            let pending = admission
                .pending_behavior_control
                .as_ref()
                .filter(|pending| pending.intent.is_some())
                .map(|pending| StepControlProjection {
                    revision: pending.revision,
                    target: crate::extensions::notification::ControlTarget::Behavior {
                        behavior_id: pending.session_mode.0.to_string(),
                    },
                    intent: pending.intent.clone(),
                });
            let applying = admission
                .applying_behavior_control
                .clone()
                .filter(|projection| projection.intent.is_some());
            let (projection, phase) = match (pending, applying) {
                (Some(pending), Some(applying)) if applying.revision >= pending.revision => {
                    (Some(applying), ControlPhase::Applying)
                }
                (Some(pending), _) => (Some(pending), ControlPhase::Pending),
                (None, Some(applying)) => (Some(applying), ControlPhase::Applying),
                (None, None) => (None, ControlPhase::Applied),
            };
            (
                projection
                    .as_ref()
                    .map_or(admission.behavior_control_revision, |p| p.revision),
                phase,
                projection,
            )
        };
        self.send_transient_grow_notification(GrowSessionUpdate::ControlStateUpdate(
            ControlStateUpdate {
                epoch: self.control_epoch.clone(),
                domain: ControlDomain::Behavior,
                revision: behavior_revision,
                intent: behavior_projection
                    .as_ref()
                    .and_then(|projection| projection.intent.clone()),
                snapshot: true,
                receipt_only: false,
                phase: behavior_phase,
                current: self.current_control_target(ControlDomain::Behavior),
                desired: behavior_projection.map(|projection| projection.target),
                message: None,
            },
        ))
        .await;
    }
    #[cfg(test)]
    pub(super) fn selection_route_for_test(
        model_id: crate::agent::models::ModelId,
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
        model_id: crate::agent::models::ModelId,
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
        model_id: &crate::agent::models::ModelId,
        sampling_config: &sampler::SamplerConfig,
        reason: &str,
        control_intent: Option<&crate::session::ControlIntent>,
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
        if control_intent.is_some()
            || previous_model_id != *model_id
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
                    control_intent,
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
        model_id: crate::agent::models::ModelId,
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
        self.commit_model_change(&model_id, &sampling_config, "catalog_reload", None)
            .await
            .map_err(|error| {
                crate::session::commands::fatal_turn_boundary_error(
                    "catalog reload model control",
                    format!("catalog reload model transition was not durable: {error}"),
                )
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
        self: &std::sync::Arc<Self>,
        catalog: std::sync::Arc<crate::agent::models::PublishedModelCatalog>,
        responds_to: tokio::sync::oneshot::Sender<Result<(), acp::Error>>,
    ) {
        let gate = self.step_control_gate.lock().await;
        let mut admission = self.state.lock().await;
        if !admission.termination.is_open() {
            let _ = responds_to.send(Err(
                acp::Error::internal_error().data("session is shutting down")
            ));
            return;
        }
        admission
            .pending_step_controls
            .admit_model_reload(catalog, responds_to);
        let should_drain = admission.foreground.is_idle();
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        drop(gate);
        if should_drain {
            self.spawn_claimed_pending_step_control_drain();
        }
    }

    /// Claim and asynchronously apply every accepted selection before any
    /// idle consumer can admit the next prompt, compaction, notification, or
    /// Goal continuation. The worker owns `ApplyingControl`, so the actor
    /// mailbox remains free to accept a newer desired revision while an Agent
    /// rebuild or MCP/workspace preparation is still in flight.
    pub(super) async fn apply_pending_step_controls_if_idle(self: &std::sync::Arc<Self>) {
        let should_drain = {
            let _gate = self.step_control_gate.lock().await;
            let mut admission = self.state.lock().await;
            if !admission.termination.is_open()
                || !admission.foreground.is_idle()
                || admission.pending_step_controls.is_empty()
            {
                false
            } else {
                admission.foreground = ForegroundState::ApplyingControl;
                true
            }
        };
        if should_drain {
            self.spawn_claimed_pending_step_control_drain();
        }
    }

    async fn apply_user_model_selection(
        &self,
        workflow_admission: &mut crate::session::workflow::manager::WorkflowManager,
        route: crate::agent::models::PublishedSessionRoute,
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
        control_intent: Option<&crate::session::ControlIntent>,
    ) -> Result<(crate::agent::models::ModelId, bool), acp::Error> {
        let crate::agent::models::PublishedSessionRoute {
            model_id,
            sampling_config,
            image_description_model,
            inference_idle_timeout,
            max_retries,
            auto_compact_threshold_percent,
        } = route;
        let previous_route = self.model_route.snapshot();
        let previous_transport = sampling_types::model_image_input_key_from_parts(
            &previous_route.sampling_config.model,
            &previous_route.sampling_config.api_backend,
            &previous_route.sampling_config.base_url,
            &previous_route.sampling_config.query_params,
        );
        let next_transport = sampling_types::model_image_input_key_from_parts(
            &sampling_config.model,
            &sampling_config.api_backend,
            &sampling_config.base_url,
            &sampling_config.query_params,
        );
        let sampling_epoch_changed = previous_route.model_id != model_id
            || previous_route.sampling_config.model != sampling_config.model
            || previous_route.sampling_config.reasoning_effort != sampling_config.reasoning_effort
            || previous_transport != next_transport;
        // Stage every fallible derivative before publishing the authoritative
        // ModelChanged fact. After that durable commit, live activation must
        // be an infallible swap; returning Rejected would otherwise disagree
        // with replay and the control receipt.
        let next_run_route = if sampling_epoch_changed {
            let mut workflow_default_sampler = sampling_config.clone();
            workflow_default_sampler.idle_timeout_secs = Some(inference_idle_timeout.as_secs());
            workflow_default_sampler.max_retries = Some(max_retries);
            workflow_default_sampler.doom_loop_recovery = self.doom_loop_recovery;
            let alpha_test_key = self
                .chat_state_handle
                .get_credentials()
                .await
                .alpha_test_key;
            Some(
                if let Some(route) = &self.startup_hints.workflow_runtime_route {
                    route.clone()
                } else {
                    let catalog = catalog.as_ref().ok_or_else(|| {
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
                },
            )
        } else {
            None
        };
        self.commit_model_change(
            &model_id,
            &sampling_config,
            "user_selection",
            control_intent,
        )
        .await
        .map_err(|error| {
            crate::session::commands::fatal_turn_boundary_error(
                "model control",
                format!("model change was not durably recorded: {error}"),
            )
        })?;
        // A desired-state request still receives its durable control receipt,
        // but an identical Sampling epoch is not a model reload. In particular,
        // do not replace live credentials or invalidate provider/compaction
        // memoization merely because the client selected the current target.
        if !sampling_epoch_changed {
            return Ok((model_id, false));
        }
        let next_run_route = next_run_route.expect("changed Sampling route was staged");
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
        Ok((model_id, sampling_epoch_changed))
    }

    async fn begin_sampling_intent<'a>(
        &self,
        mut admission: tokio::sync::MutexGuard<'a, AdmissionState>,
        gate: &mut Option<tokio::sync::MutexGuard<'_, ()>>,
        intent: &Option<crate::session::ControlIntent>,
        responds_to: &mut Option<
            tokio::sync::oneshot::Sender<
                Result<
                    crate::session::DesiredStateOutcome<crate::agent::models::ModelId>,
                    acp::Error,
                >,
            >,
        >,
    ) -> Option<tokio::sync::MutexGuard<'a, AdmissionState>> {
        if !admission.termination.is_open() {
            let _ = responds_to
                .take()
                .expect("Sampling responder is available")
                .send(Err(
                    acp::Error::internal_error().data("session is shutting down")
                ));
            return None;
        }
        match admission.admit_control_intent(
            crate::extensions::notification::ControlDomain::Sampling,
            intent.as_ref(),
        ) {
            ControlIntentAdmission::New => Some(admission),
            ControlIntentAdmission::DuplicateInFlight => {
                let _ = responds_to
                    .take()
                    .expect("Sampling responder is available")
                    .send(Ok(crate::session::DesiredStateOutcome::InFlight));
                None
            }
            ControlIntentAdmission::Older => {
                let _ = responds_to
                    .take()
                    .expect("Sampling responder is available")
                    .send(Ok(crate::session::DesiredStateOutcome::Superseded));
                None
            }
            ControlIntentAdmission::ExactTerminal(terminal) => {
                let revision = (!terminal.ui_terminal_durable).then(|| {
                    admission
                        .pending_step_controls
                        .reserve_terminal_replay_revision(
                            crate::extensions::notification::ControlDomain::Sampling,
                        )
                });
                drop(admission);
                // Terminal UI repair is derived from an already-authoritative
                // durable receipt. It must not hold the Step admission gate
                // across an exact append retry, or Stop/Shutdown cannot enter.
                drop(gate.take());
                let response =
                    if let (Some(intent), Some(revision)) = (intent.as_ref(), revision) {
                        self.recover_missing_terminal_projection(
                            crate::extensions::notification::ControlDomain::Sampling,
                            intent,
                            &terminal,
                            revision,
                        )
                        .await
                    } else {
                        Ok(())
                    }
                    .and_then(|()| match terminal.phase {
                        crate::extensions::notification::ControlPhase::Applied => {
                            let crate::extensions::notification::ControlTarget::Sampling {
                                model_id,
                                ..
                            } = terminal.target
                            else {
                                return Err(acp::Error::internal_error()
                                    .data("persisted Sampling receipt has a non-Sampling target"));
                            };
                            Ok(crate::session::DesiredStateOutcome::Applied(
                                crate::agent::models::ModelId::new(model_id),
                            ))
                        }
                        crate::extensions::notification::ControlPhase::Rejected => Err(
                            acp::Error::invalid_request().data(terminal.message.unwrap_or_else(
                                || "the Sampling request was previously rejected".to_string(),
                            )),
                        ),
                        crate::extensions::notification::ControlPhase::Superseded => {
                            Ok(crate::session::DesiredStateOutcome::Superseded)
                        }
                        crate::extensions::notification::ControlPhase::Pending
                        | crate::extensions::notification::ControlPhase::Applying => {
                            Err(acp::Error::internal_error()
                                .data("persisted Sampling receipt is not terminal"))
                        }
                    });
                let _ = responds_to
                    .take()
                    .expect("Sampling responder is available")
                    .send(response);
                None
            }
        }
    }

    async fn finish_sampling_admission_locked(
        self: &std::sync::Arc<Self>,
        gate: tokio::sync::MutexGuard<'_, ()>,
        mut admission: tokio::sync::MutexGuard<'_, AdmissionState>,
        route: crate::agent::models::PublishedSessionRoute,
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
        >,
    ) {
        let (revision, superseded) =
            admission
                .pending_step_controls
                .admit_sampling(route, catalog, intent, responds_to);
        let projection = admission
            .pending_step_controls
            .domain_projection(crate::extensions::notification::ControlDomain::Sampling)
            .expect("admitted Sampling target has a projection");
        debug_assert_eq!(projection.revision, revision);
        let should_drain = admission.foreground.is_idle();
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if let Some((revision, superseded)) = superseded {
            let projection = StepControlProjection {
                revision,
                target: crate::extensions::notification::ControlTarget::Sampling {
                    model_id: superseded.route.model_id.0.to_string(),
                    reasoning_effort: superseded
                        .route
                        .sampling_config
                        .reasoning_effort
                        .map(|effort| effort.to_string()),
                },
                intent: superseded.intent.clone(),
            };
            self.state.lock().await.mark_control_intent_terminal(
                crate::extensions::notification::ControlDomain::Sampling,
                superseded.intent.as_ref(),
                ControlIntentTerminal {
                    phase: crate::extensions::notification::ControlPhase::Superseded,
                    target: projection.target.clone(),
                    message: None,
                    // Superseded desired state is intentionally UI-silent;
                    // the new Pending projection replaces it in place.
                    ui_terminal_durable: true,
                },
            );
            let _ = superseded
                .respond_to
                .send(Ok(crate::session::DesiredStateOutcome::Superseded));
        }
        let _ = self
            .publish_control_projection(
                &projection,
                crate::extensions::notification::ControlPhase::Pending,
                None,
                false,
            )
            .await;
        drop(gate);
        if should_drain {
            self.spawn_claimed_pending_step_control_drain();
        }
    }

    /// Compose an effort-only request with the newest actor-owned Sampling
    /// model. Intent admission, desired-model lookup, route resolution and
    /// slot replacement share one step gate, so the request cannot miss the
    /// boundary at which it was admitted.
    pub(super) async fn admit_session_effort_patch(
        self: &std::sync::Arc<Self>,
        effort: sampling_types::ReasoningEffort,
        authority: crate::session::SessionEffortAuthority,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
        >,
    ) {
        let mut gate = Some(self.step_control_gate.lock().await);
        let mut responds_to = Some(responds_to);
        let Some(mut admission) = self
            .begin_sampling_intent(
                self.state.lock().await,
                &mut gate,
                &intent,
                &mut responds_to,
            )
            .await
        else {
            return;
        };
        let model_id = admission
            .pending_step_controls
            .desired_sampling_model_id()
            .unwrap_or_else(|| crate::agent::models::ModelId::new(self.current_catalog_model_id()));
        let resolved = match authority {
            crate::session::SessionEffortAuthority::Catalog {
                catalog,
                origin_client,
            } => {
                let offered = catalog
                    .model_reasoning_efforts(model_id.0.as_ref())
                    .iter()
                    .any(|option| option.value == effort);
                if !offered {
                    Err(acp::Error::invalid_params().data(format!(
                        "model '{}' does not admit '{}' reasoning effort for this session",
                        model_id.0, effort
                    )))
                } else {
                    catalog
                        .resolve_session_route(&model_id, Some(effort))
                        .filter(|route| route.model_id == model_id)
                        .map(|mut route| {
                            route.sampling_config.origin_client = origin_client;
                            (route, Some(catalog))
                        })
                        .ok_or_else(|| {
                            acp::Error::invalid_params()
                                .data(format!("model '{}' is not routable", model_id.0))
                        })
                }
            }
            crate::session::SessionEffortAuthority::Workflow {
                route,
                models_manager,
            } => {
                if !route.supports_reasoning_effort(model_id.0.as_ref(), effort) {
                    Err(acp::Error::invalid_params().data(format!(
                        "model '{}' does not admit '{}' reasoning effort for this Workflow Run",
                        model_id.0, effort
                    )))
                } else {
                    route
                        .session_route_for(model_id.0.as_ref(), &models_manager, None)
                        .map(|mut resolved| {
                            resolved.sampling_config.reasoning_effort = Some(effort);
                            (resolved, None)
                        })
                        .map_err(|error| acp::Error::invalid_params().data(error))
                }
            }
        };
        match resolved {
            Ok((route, catalog)) => {
                self.finish_sampling_admission_locked(
                    gate.take().expect("Sampling admission gate is available"),
                    admission,
                    route,
                    catalog,
                    intent,
                    responds_to.take().expect("Sampling responder is available"),
                )
                .await;
            }
            Err(error) => {
                let message = error.to_string();
                admission.mark_control_intent_terminal(
                    crate::extensions::notification::ControlDomain::Sampling,
                    intent.as_ref(),
                    ControlIntentTerminal {
                        phase: crate::extensions::notification::ControlPhase::Rejected,
                        target: crate::extensions::notification::ControlTarget::Sampling {
                            model_id: model_id.0.to_string(),
                            reasoning_effort: Some(effort.to_string()),
                        },
                        message: Some(message),
                        ui_terminal_durable: false,
                    },
                );
                let _ = responds_to
                    .take()
                    .expect("Sampling responder is available")
                    .send(Err(error));
            }
        }
    }

    /// Admit a user model/effort selection without mutating the active step.
    /// The response completes after the selection is durably applied at the
    /// next step boundary (or immediately while the session is idle).
    pub(super) async fn admit_session_model_selection(
        self: &std::sync::Arc<Self>,
        route: crate::agent::models::PublishedSessionRoute,
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
        >,
    ) {
        let mut gate = Some(self.step_control_gate.lock().await);
        let mut responds_to = Some(responds_to);
        let Some(admission) = self
            .begin_sampling_intent(
                self.state.lock().await,
                &mut gate,
                &intent,
                &mut responds_to,
            )
            .await
        else {
            return;
        };
        self.finish_sampling_admission_locked(
            gate.take().expect("Sampling admission gate is available"),
            admission,
            route,
            catalog,
            intent,
            responds_to.take().expect("Sampling responder is available"),
        )
        .await;
    }

    /// Admit an Agent profile selection under the same step boundary as
    /// model/effort changes.
    pub(super) async fn admit_agent_selection(
        self: &std::sync::Arc<Self>,
        definition: agent::AgentDefinition,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<()>, acp::Error>,
        >,
    ) {
        let gate = self.step_control_gate.lock().await;
        let mut admission = self.state.lock().await;
        if !admission.termination.is_open() {
            let _ = responds_to.send(Err(
                acp::Error::internal_error().data("session is shutting down")
            ));
            return;
        }
        match admission.admit_control_intent(
            crate::extensions::notification::ControlDomain::Agent,
            intent.as_ref(),
        ) {
            ControlIntentAdmission::New => {}
            ControlIntentAdmission::DuplicateInFlight => {
                let _ = responds_to.send(Ok(crate::session::DesiredStateOutcome::InFlight));
                return;
            }
            ControlIntentAdmission::Older => {
                let _ = responds_to.send(Ok(crate::session::DesiredStateOutcome::Superseded));
                return;
            }
            ControlIntentAdmission::ExactTerminal(terminal) => {
                let revision = (!terminal.ui_terminal_durable).then(|| {
                    admission
                        .pending_step_controls
                        .reserve_terminal_replay_revision(
                            crate::extensions::notification::ControlDomain::Agent,
                        )
                });
                drop(admission);
                drop(gate);
                let response =
                    if let (Some(intent), Some(revision)) = (intent.as_ref(), revision) {
                        self.recover_missing_terminal_projection(
                            crate::extensions::notification::ControlDomain::Agent,
                            intent,
                            &terminal,
                            revision,
                        )
                        .await
                    } else {
                        Ok(())
                    }
                    .and_then(|()| match terminal.phase {
                        crate::extensions::notification::ControlPhase::Applied => {
                            Ok(crate::session::DesiredStateOutcome::Applied(()))
                        }
                        crate::extensions::notification::ControlPhase::Rejected => Err(
                            acp::Error::invalid_request().data(terminal.message.unwrap_or_else(
                                || "the Agent request was previously rejected".to_string(),
                            )),
                        ),
                        crate::extensions::notification::ControlPhase::Superseded => {
                            Ok(crate::session::DesiredStateOutcome::Superseded)
                        }
                        crate::extensions::notification::ControlPhase::Pending
                        | crate::extensions::notification::ControlPhase::Applying => {
                            Err(acp::Error::internal_error()
                                .data("persisted Agent receipt is not terminal"))
                        }
                    });
                let _ = responds_to.send(response);
                return;
            }
        }
        let preparation =
            if !self.startup_hints.is_subagent && !definition.is_primary_agent_eligible() {
                let issues = definition
                    .primary_agent_issues()
                    .into_iter()
                    .map(|issue| issue.message())
                    .collect::<Vec<_>>()
                    .join(", ");
                AgentPreparation::ready(
                    definition.selector_identity(),
                    Err(acp::Error::invalid_request().data(format!(
                        "Agent `{}` cannot own a primary session: {issues}",
                        definition.selector_identity()
                    ))),
                )
            } else {
                AgentPreparation::start(
                    self.rebuild_spec.clone(),
                    definition,
                    self.session_id_string(),
                )
            };
        let (revision, superseded) =
            admission
                .pending_step_controls
                .admit_agent(preparation, intent, responds_to);
        let projection = admission
            .pending_step_controls
            .domain_projection(crate::extensions::notification::ControlDomain::Agent)
            .expect("admitted Agent target has a projection");
        debug_assert_eq!(projection.revision, revision);
        let should_drain = admission.foreground.is_idle();
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if let Some((revision, superseded)) = superseded {
            let projection = StepControlProjection {
                revision,
                target: crate::extensions::notification::ControlTarget::Agent {
                    agent_name: superseded.preparation.target_name().to_owned(),
                },
                intent: superseded.intent.clone(),
            };
            self.state.lock().await.mark_control_intent_terminal(
                crate::extensions::notification::ControlDomain::Agent,
                superseded.intent.as_ref(),
                ControlIntentTerminal {
                    phase: crate::extensions::notification::ControlPhase::Superseded,
                    target: projection.target.clone(),
                    message: None,
                    // Superseded desired state is intentionally UI-silent;
                    // the new Pending projection replaces it in place.
                    ui_terminal_durable: true,
                },
            );
            let _ = superseded
                .respond_to
                .send(Ok(crate::session::DesiredStateOutcome::Superseded));
        }
        let _ = self
            .publish_control_projection(
                &projection,
                crate::extensions::notification::ControlPhase::Pending,
                None,
                false,
            )
            .await;
        drop(gate);
        if should_drain {
            self.spawn_claimed_pending_step_control_drain();
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
        let gate = self.step_control_gate.lock().await;
        let mut admission = self.state.lock().await;
        if !admission.termination.is_open() {
            return Err("session is shutting down".to_string());
        }
        let should_drain = admission.foreground.is_idle();
        let (responds_to, applied) = if should_drain {
            let (tx, rx) = tokio::sync::oneshot::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        admission
            .pending_step_controls
            .admit_goal_definition(PendingGoalDefinitionControl {
                goal_id,
                mutation,
                responds_to,
                invocation: HOST_COMMAND_INVOCATION.try_with(Clone::clone).ok(),
            });
        if should_drain {
            admission.foreground = ForegroundState::ApplyingControl;
        }
        drop(admission);
        if !should_drain {
            // Publish before releasing the boundary: application cannot race
            // ahead of its queued feedback and leave a stale spinner behind.
            self.send_host_turn_slash_command_notice(
                crate::extensions::notification::UiNoticeTone::Progress,
                "Goal update queued for the next step boundary.",
                None,
            )
            .await;
        }
        drop(gate);
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

    /// Apply the surviving Sampling/Agent targets and ordered lifecycle
    /// controls accepted during the completed step. The foreground remains
    /// owned by the same turn, so
    /// prompts, compaction, notification drains and Goal continuations stay
    /// fenced.
    /// Cancellation serializes on `step_control_gate` and therefore cannot
    /// tear a durable transition away from its live-state swap.
    pub(super) async fn apply_pending_controls_at_step_boundary(
        &self,
        mut boundary: StepControlBoundary,
    ) -> (bool, bool, bool) {
        let mut model_changed = false;
        let mut agent_changed = false;
        let mut behavior_changed = false;
        loop {
            let (key, preparation, projection) = {
                let mut admission = self.state.lock().await;
                if !matches!(&admission.foreground, ForegroundState::RegularTurn(_)) {
                    return (model_changed, agent_changed, behavior_changed);
                }
                let Some(key) = admission
                    .pending_step_controls
                    .next_key_at_boundary(boundary)
                else {
                    return (model_changed, agent_changed, behavior_changed);
                };
                let preparation = admission.pending_step_controls.agent_preparation(key);
                let projection = admission.pending_step_controls.projection(key);
                admission.applying_step_control = projection.clone();
                (key, preparation, projection)
            };
            if let Some(projection) = &projection {
                let _ = self
                    .publish_control_projection(
                        projection,
                        crate::extensions::notification::ControlPhase::Applying,
                        None,
                        false,
                    )
                    .await;
            }
            let workspace_binding = if let Some(preparation) = preparation {
                // Filesystem/plugin/skill discovery is deliberately outside
                // the cancellation-critical step gate. If Stop aborts this
                // turn while preparation is pending, the queue retains the
                // result and the idle control drain applies it exactly once.
                if !preparation.wait_ready().await {
                    Ok(None)
                } else if preparation.has_agent() {
                    tokio::select! {
                        binding = self.prepare_agent_workspace_binding() => binding.map(Some),
                        () = preparation.wait_superseded() => Ok(None),
                    }
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
                    if admission.applying_step_control.as_ref() == projection.as_ref() {
                        admission.applying_step_control = None;
                    }
                    return (model_changed, agent_changed, behavior_changed);
                }
                let control = admission.pending_step_controls.take(key);
                if control.is_none()
                    && admission.applying_step_control.as_ref() == projection.as_ref()
                {
                    admission.applying_step_control = None;
                }
                control
            };
            let Some(control) = control else {
                // The desired Agent/Sampling revision changed while an older
                // Agent preparation was in flight. Discard the stale result
                // and resolve the new authoritative target instead.
                continue;
            };
            let (
                applied_model,
                applied_agent,
                applied_behavior,
                retired_goal_owner,
                deferred_goal_result,
                mut terminal_settlement,
                fatal_error,
            ) = self.apply_pending_control(control, workspace_binding).await;
            // The control was atomically claimed under `state` immediately
            // before application. Admission itself does not wait on this gate:
            // a later desired revision is accepted while the durable commit is
            // in flight and remains the sole target for the following Step.
            boundary.close_domain(key);
            {
                let mut admission = self.state.lock().await;
                if admission.applying_step_control.as_ref() == projection.as_ref() {
                    admission.applying_step_control = None;
                }
            }
            if let Some(error) = fatal_error {
                self.state
                    .lock()
                    .await
                    .termination
                    .request(TerminationState::Fatal);
                drop(_gate);
                if let Some(settlement) = terminal_settlement.take() {
                    settlement.settle_fatal();
                }
                let _ = self.event_tx.send(SessionEvent::ControlWorkerFailed {
                    message: format!("control Timeline commit failed: {error}"),
                });
                return (model_changed, agent_changed, behavior_changed);
            }
            if let (Some(projection), Some(settlement)) =
                (&projection, terminal_settlement.as_ref())
            {
                let result = settlement.terminal_result();
                let phase = if result.is_ok() {
                    crate::extensions::notification::ControlPhase::Applied
                } else {
                    crate::extensions::notification::ControlPhase::Rejected
                };
                let message = Some(Self::control_terminal_message(&projection.target, &result));
                let (domain, intent) = settlement.control_intent();
                // The Timeline transition and live route are authoritative at
                // this point. Publish that terminal fact before releasing the
                // Step gate so Stop cannot strand an applied intent as
                // `InFlight`. The append-only UI projection is independently
                // repairable from this receipt after reconnect/retry.
                self.state.lock().await.mark_control_intent_terminal(
                    domain,
                    intent,
                    ControlIntentTerminal {
                        phase,
                        target: projection.target.clone(),
                        message,
                        ui_terminal_durable: false,
                    },
                );
            }
            // An uncertain UI acknowledgement must not prevent Stop/Shutdown
            // from entering its boundary. The terminal receipt above makes a
            // missing append recoverable without replaying the control.
            drop(_gate);
            if let (Some(projection), Some(settlement)) =
                (&projection, terminal_settlement.as_ref())
            {
                let result = settlement.terminal_result();
                let phase = if result.is_ok() {
                    crate::extensions::notification::ControlPhase::Applied
                } else {
                    crate::extensions::notification::ControlPhase::Rejected
                };
                let message = Some(Self::control_terminal_message(&projection.target, &result));
                let terminal_append = self
                    .publish_control_projection(projection, phase, message.clone(), true)
                    .await;
                let (domain, intent) = settlement.control_intent();
                if terminal_append.is_ok()
                    && let Some(intent) = intent
                {
                    self.state
                        .lock()
                        .await
                        .mark_control_terminal_ui_durable(domain, intent);
                }
                if let Some(settlement) = terminal_settlement {
                    settlement.settle(terminal_append);
                }
            }
            if let Some((goal_id, definition_revision)) = retired_goal_owner {
                self.cancel_goal_owned_work(&goal_id, definition_revision)
                    .await;
            }
            if let Some((invocation, outcome)) = deferred_goal_result {
                self.publish_deferred_goal_result(invocation, outcome).await;
            }
            model_changed |= applied_model;
            agent_changed |= applied_agent;
            behavior_changed |= applied_behavior;
        }
    }

    /// Close one causal Step and freeze the exact control-admission horizon
    /// it is allowed to consume. A later request can supersede a preparation
    /// that has not committed yet, but a request admitted after a successful
    /// commit remains pending for the following Step instead of triggering a
    /// second transition in the same `StepEnded → StepStarted` interval.
    pub(super) async fn end_step_control_boundary(
        &self,
        outcome: &str,
    ) -> Option<StepControlBoundary> {
        let _gate = self.step_control_gate.lock().await;
        if !self.events.end_step(outcome) {
            return None;
        }
        Some(self.state.lock().await.pending_step_controls.boundary())
    }

    /// Freeze the control horizon for the first sampling Step of a turn.
    ///
    /// A newly admitted turn has a durable `TurnStarted` but no active Step
    /// yet, so there is no predecessor to end. Controls accepted while the
    /// turn was being prepared must still govern its first provider request.
    /// This is intentionally distinct from [`Self::end_step_control_boundary`]:
    /// after any Step has started, a missing active Step is a causal error and
    /// must not be mistaken for another initial boundary.
    pub(super) async fn initial_step_control_boundary(
        &self,
        prompt_id: &str,
    ) -> Option<StepControlBoundary> {
        let _gate = self.step_control_gate.lock().await;
        if self.events.has_active_step() || self.events.next_step_index() != 0 {
            return None;
        }
        let admission = self.state.lock().await;
        if !admission.can_continue_regular_turn(prompt_id) {
            return None;
        }
        Some(admission.pending_step_controls.boundary())
    }

    async fn apply_pending_control(
        &self,
        control: PendingStepControl,
        workspace_binding: Result<Option<workspace::PreparedLocalSessionBind>, acp::Error>,
    ) -> (
        bool,
        bool,
        bool,
        Option<(String, u64)>,
        Option<(crate::session::HostCommandInvocation, Result<bool, String>)>,
        Option<PendingControlSettlement>,
        Option<acp::Error>,
    ) {
        if let Err(error) = self
            .cancel_background_compaction("control_authority_changed")
            .await
        {
            return (false, false, false, None, None, None, Some(error));
        }
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
                let fatal = result
                    .as_ref()
                    .err()
                    .filter(|error| crate::session::commands::is_fatal_turn_boundary_error(error))
                    .cloned();
                for respond_to in pending.responders {
                    let response = match &result {
                        Ok(()) => Ok(()),
                        Err(error) => Err(error.clone()),
                    };
                    let _ = respond_to.send(response);
                }
                (applied, false, false, None, None, None, fatal)
            }
            PendingStepControl::ModelSelection(pending) => {
                let mut workflow_admission = self.workflow_manager.lock().await;
                let result = self
                    .apply_user_model_selection(
                        &mut workflow_admission,
                        pending.route,
                        pending.catalog,
                        pending.intent.as_ref(),
                    )
                    .await;
                let changed = result
                    .as_ref()
                    .map(|(_, changed)| *changed)
                    .unwrap_or(false);
                let fatal = result
                    .as_ref()
                    .err()
                    .filter(|error| crate::session::commands::is_fatal_turn_boundary_error(error))
                    .cloned();
                (
                    changed,
                    false,
                    false,
                    None,
                    None,
                    Some(PendingControlSettlement::Sampling {
                        respond_to: pending.respond_to,
                        result: result.map(|(model_id, _)| model_id),
                        intent: pending.intent,
                    }),
                    fatal,
                )
            }
            PendingStepControl::AgentSelection(pending) => {
                let result = match (pending.preparation.take(), workspace_binding) {
                    (Ok(agent), Ok(Some(binding))) => {
                        self.apply_prepared_agent(agent, binding, pending.intent.as_ref())
                            .await
                    }
                    (Ok(_), Ok(None)) => Err(acp::Error::internal_error()
                        .data("Agent activation was not given a prepared workspace binding")),
                    (Ok(_), Err(error)) | (Err(error), _) => Err(error),
                };
                let applied = result.as_ref().is_ok_and(|applied| *applied);
                let fatal = result
                    .as_ref()
                    .err()
                    .filter(|error| crate::session::commands::is_fatal_turn_boundary_error(error))
                    .cloned();
                (
                    false,
                    applied,
                    false,
                    None,
                    None,
                    Some(PendingControlSettlement::Agent {
                        respond_to: pending.respond_to,
                        result,
                        intent: pending.intent,
                    }),
                    fatal,
                )
            }
            PendingStepControl::GoalDefinition(pending) => {
                let result = self.apply_pending_goal_definition_control(&pending).await;
                let (retired_goal_owner, behavior_changed) = match &result {
                    Ok((_, retired_goal_owner, behavior_changed)) => {
                        (retired_goal_owner.clone(), *behavior_changed)
                    }
                    Err(_) => (None, false),
                };
                let fatal = result
                    .as_ref()
                    .err()
                    .filter(|error| crate::session::commands::is_fatal_turn_boundary_error(error))
                    .cloned();
                let outcome = result
                    .as_ref()
                    .map(|(changed, _, _)| *changed)
                    .map_err(|error| {
                        error
                            .data
                            .as_ref()
                            .and_then(|data| {
                                data.as_str().or_else(|| {
                                    data.get("message").and_then(serde_json::Value::as_str)
                                })
                            })
                            .map(str::to_owned)
                            .unwrap_or_else(|| error.to_string())
                    });
                let deferred_goal_result = pending
                    .invocation
                    .filter(|_| pending.responds_to.is_none())
                    .map(|invocation| (invocation, outcome));
                if let Some(respond_to) = pending.responds_to {
                    let _ = respond_to.send(
                        result
                            .map(|(changed, _, _)| changed)
                            .map_err(|error| error.to_string()),
                    );
                }
                (
                    false,
                    false,
                    behavior_changed,
                    retired_goal_owner,
                    deferred_goal_result,
                    None,
                    fatal,
                )
            }
        }
    }

    async fn publish_deferred_goal_result(
        &self,
        invocation: crate::session::HostCommandInvocation,
        outcome: Result<bool, String>,
    ) {
        HOST_COMMAND_INVOCATION.scope(invocation, async {
            match outcome {
                Ok(true) => self.send_host_turn_slash_command_success("Goal definition updated.").await,
                Ok(false) => self.send_host_turn_slash_command_output("Goal already has that definition; nothing changed.").await,
                Err(error) => self.send_host_turn_slash_command_error(
                    "Scheduled Goal update was rejected",
                    format!("Reason: {error}\nInspect /goal status, correct the definition, and retry."),
                ).await,
            }
        }).await;
    }

    fn spawn_claimed_pending_step_control_drain(self: &std::sync::Arc<Self>) {
        let session = std::sync::Arc::clone(self);
        let handle = tokio::task::spawn_local(async move {
            let result =
                std::panic::AssertUnwindSafe(session.drain_claimed_pending_step_controls())
                    .catch_unwind()
                    .await
                    .map_err(|payload| {
                        payload
                            .downcast_ref::<&str>()
                            .copied()
                            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                            .unwrap_or("non-string panic payload")
                            .to_string()
                    });
            // Re-enter the single idle arbiter after releasing the foreground
            // fence. It will promote queued prompts/compaction/notifications
            // in their normal priority order, or schedule Goal continuation.
            session.idle_arbiter.notify_waiters();
            match result {
                Ok(()) => Ok(()),
                Err(panic) => {
                    let message = format!("Sampling/Agent control worker panicked: {panic}");
                    {
                        let mut state = session.state.lock().await;
                        state.termination.request(TerminationState::Fatal);
                        state.applying_step_control = None;
                        if matches!(state.foreground, ForegroundState::ApplyingControl) {
                            state.foreground = ForegroundState::Idle;
                        }
                    }
                    let _ = session.event_tx.send(SessionEvent::ControlWorkerFailed {
                        message: message.clone(),
                    });
                    Err(message)
                }
            }
        });
        self.step_control_worker.arm(handle);
    }

    /// Drain a queue whose caller atomically changed idle foreground ownership
    /// to `ApplyingControl` while admitting the first control. This closes the
    /// enqueue-to-drain race in which a prompt or Goal continuation could
    /// otherwise claim the old route.
    async fn drain_claimed_pending_step_controls(&self) {
        loop {
            let (key, preparation, projection) = {
                let mut admission = self.state.lock().await;
                let Some(key) = admission.pending_step_controls.next_key() else {
                    if matches!(admission.foreground, ForegroundState::ApplyingControl) {
                        admission.foreground = ForegroundState::Idle;
                    }
                    admission.applying_step_control = None;
                    return;
                };
                let preparation = admission.pending_step_controls.agent_preparation(key);
                let projection = admission.pending_step_controls.projection(key);
                admission.applying_step_control = projection.clone();
                (key, preparation, projection)
            };
            if let Some(projection) = &projection {
                let _ = self
                    .publish_control_projection(
                        projection,
                        crate::extensions::notification::ControlPhase::Applying,
                        None,
                        false,
                    )
                    .await;
            }
            let workspace_binding = if let Some(preparation) = preparation {
                if !preparation.wait_ready().await {
                    Ok(None)
                } else if preparation.has_agent() {
                    tokio::select! {
                        binding = self.prepare_agent_workspace_binding() => binding.map(Some),
                        () = preparation.wait_superseded() => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            };
            let _gate = self.step_control_gate.lock().await;
            let mut admission = self.state.lock().await;
            if !admission.termination.is_open() {
                let cancelled = admission.pending_step_controls.cancel_for_shutdown();
                admission.applying_step_control = None;
                if matches!(admission.foreground, ForegroundState::ApplyingControl) {
                    admission.foreground = ForegroundState::Idle;
                }
                drop(admission);
                self.publish_cancelled_goal_commands(cancelled).await;
                return;
            }
            debug_assert!(matches!(
                admission.foreground,
                ForegroundState::ApplyingControl
            ));
            let Some(control) = admission.pending_step_controls.take(key) else {
                // A newer desired revision replaced the prepared target.
                if admission.applying_step_control.as_ref() == projection.as_ref() {
                    admission.applying_step_control = None;
                }
                drop(admission);
                drop(_gate);
                continue;
            };
            drop(admission);
            let (
                _,
                _,
                _,
                retired_goal_owner,
                deferred_goal_result,
                mut terminal_settlement,
                fatal_error,
            ) = self.apply_pending_control(control, workspace_binding).await;
            {
                let mut admission = self.state.lock().await;
                if admission.applying_step_control.as_ref() == projection.as_ref() {
                    admission.applying_step_control = None;
                }
            }
            if let Some(error) = fatal_error {
                self.state
                    .lock()
                    .await
                    .termination
                    .request(TerminationState::Fatal);
                drop(_gate);
                if let Some(settlement) = terminal_settlement.take() {
                    settlement.settle_fatal();
                }
                let _ = self.event_tx.send(SessionEvent::ControlWorkerFailed {
                    message: format!("control Timeline commit failed: {error}"),
                });
                return;
            }
            if let (Some(projection), Some(settlement)) =
                (&projection, terminal_settlement.as_ref())
            {
                let result = settlement.terminal_result();
                let phase = if result.is_ok() {
                    crate::extensions::notification::ControlPhase::Applied
                } else {
                    crate::extensions::notification::ControlPhase::Rejected
                };
                let message = Some(Self::control_terminal_message(&projection.target, &result));
                let (domain, intent) = settlement.control_intent();
                self.state.lock().await.mark_control_intent_terminal(
                    domain,
                    intent,
                    ControlIntentTerminal {
                        phase,
                        target: projection.target.clone(),
                        message,
                        ui_terminal_durable: false,
                    },
                );
            }
            // The authoritative receipt is visible before the cancellation
            // gate opens; UI persistence may now happen without blocking Stop.
            drop(_gate);
            if let (Some(projection), Some(settlement)) =
                (&projection, terminal_settlement.as_ref())
            {
                let result = settlement.terminal_result();
                let phase = if result.is_ok() {
                    crate::extensions::notification::ControlPhase::Applied
                } else {
                    crate::extensions::notification::ControlPhase::Rejected
                };
                let message = Some(Self::control_terminal_message(&projection.target, &result));
                let terminal_append = self
                    .publish_control_projection(projection, phase, message.clone(), true)
                    .await;
                let (domain, intent) = settlement.control_intent();
                if terminal_append.is_ok()
                    && let Some(intent) = intent
                {
                    self.state
                        .lock()
                        .await
                        .mark_control_terminal_ui_durable(domain, intent);
                }
                if let Some(settlement) = terminal_settlement {
                    settlement.settle(terminal_append);
                }
            }
            if let Some((goal_id, definition_revision)) = retired_goal_owner {
                self.cancel_goal_owned_work(&goal_id, definition_revision)
                    .await;
            }
            if let Some((invocation, outcome)) = deferred_goal_result {
                self.publish_deferred_goal_result(invocation, outcome).await;
            }
        }
    }

    /// Stop only controls that have not entered their durable commit section.
    /// Taking the step gate waits for an already-claimed commit to finish, but
    /// can cancel Agent preparation and queued desired state immediately.
    pub(super) async fn cancel_uncommitted_controls_for_shutdown(&self) {
        let _gate = self.step_control_gate.lock().await;
        let (pending_behavior, cancelled) = {
            let mut state = self.state.lock().await;
            let cancelled = state.pending_step_controls.cancel_for_shutdown();
            (state.pending_behavior_control.take(), cancelled)
        };
        self.publish_cancelled_goal_commands(cancelled).await;
        if let Some(pending) = pending_behavior {
            let _ = pending.responds_to.send(Err(
                acp::Error::internal_error().data("session is shutting down")
            ));
        }
        self.idle_arbiter.notify_waiters();
    }

    pub(super) async fn publish_cancelled_goal_commands(
        &self,
        invocations: Vec<crate::session::HostCommandInvocation>,
    ) {
        // Shutdown can hold the admission gate. Enqueue before its existing
        // persistence barrier instead of awaiting a UI ack under that gate.
        let sender = self.goal_notify_sender();
        for invocation in invocations {
            sender.send_update(crate::extensions::notification::SessionUpdate::UiNotice(
                crate::extensions::notification::UiNotice {
                    correlation_id: invocation.invocation_id,
                    category: crate::extensions::notification::UiNoticeCategory::Command,
                    subject: Some(invocation.command),
                    description: Some(invocation.description),
                    tone: crate::extensions::notification::UiNoticeTone::Warning,
                    message:
                        "Scheduled Goal update cancelled because the session is shutting down."
                            .into(),
                    details: None,
                },
            ));
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
        self.apply_prepared_agent(new_agent, workspace_binding, None)
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
        control_intent: Option<&crate::session::ControlIntent>,
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
            if let Some(intent) = control_intent {
                let _transaction = self.goal_transaction_gate.lock().await;
                let (behavior, goal) = self.capture_control_authorities();
                self.persist_applied_control_receipt_durably(
                    behavior,
                    goal,
                    crate::extensions::notification::ControlDomain::Agent,
                    crate::extensions::notification::ControlTarget::Agent {
                        agent_name: new_agent_name.clone(),
                    },
                    intent.clone(),
                )
                .await
                .map_err(|error| {
                    crate::session::commands::fatal_turn_boundary_error(
                        "Agent control",
                        format!("Agent acknowledgement was not durably recorded: {error}"),
                    )
                })?;
            }
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
        // Agent construction may have started before a same-boundary Sampling
        // revision committed. Rebind model-derived tool limits at activation
        // so the new harness cannot retain the previous context window.
        candidate_bridge
            .set_context_window_tokens(
                self.rebuild_spec
                    .context_window_tokens
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .await;
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
        let candidate_supports_workflow = self.background_workflows_enabled
            && !self.startup_hints.is_subagent
            && !self.workflow_service_shutdown.is_cancelled();
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
        let persistence = {
            let _transaction = self.goal_transaction_gate.lock().await;
            if let Some(intent) = control_intent {
                let (behavior, goal) = self.capture_control_authorities();
                self.persist_agent_transition_for_control_durably(
                    behavior,
                    goal,
                    &new_agent_name,
                    new_agent.role_prompt(),
                    candidate_capability_catalog.as_deref(),
                    intent.clone(),
                )
                .await
            } else {
                self.persist_agent_transition_durably(
                    &new_agent_name,
                    new_agent.role_prompt(),
                    candidate_capability_catalog.as_deref(),
                )
                .await
            }
        };
        persistence.map_err(|error| {
            crate::session::commands::fatal_turn_boundary_error(
                "Agent control",
                format!("rebuilt agent role was not durably recorded: {error}"),
            )
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
    use crate::session::events::Event;

    #[tokio::test]
    async fn internal_control_transition_does_not_publish_ui_feedback() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, mut gateway_rx) = super::super::tests::support::build_actor().await;
                while gateway_rx.try_recv().is_ok() {}
                let projection = StepControlProjection {
                    revision: 1,
                    target: crate::extensions::notification::ControlTarget::Sampling {
                        model_id: "provider/restored".into(),
                        reasoning_effort: Some("high".into()),
                    },
                    intent: None,
                };

                actor
                    .publish_control_projection(
                        &projection,
                        crate::extensions::notification::ControlPhase::Applied,
                        Some("Sampling switched to provider/restored (high)".into()),
                        false,
                    )
                    .await
                    .unwrap();

                assert!(
                    gateway_rx.try_recv().is_err(),
                    "session restoration must not emit a user control notification"
                );

                let user_projection = StepControlProjection {
                    intent: Some(crate::session::ControlIntent {
                        client_id: "pager".into(),
                        generation: 1,
                        sequence: 1,
                    }),
                    ..projection.clone()
                };
                actor
                    .publish_control_projection(
                        &user_projection,
                        crate::extensions::notification::ControlPhase::Pending,
                        None,
                        false,
                    )
                    .await
                    .unwrap();
                assert!(
                    gateway_rx.try_recv().is_ok(),
                    "a client-authored control must retain live feedback"
                );

                while gateway_rx.try_recv().is_ok() {}
                actor.state.lock().await.applying_step_control = Some(projection);
                actor.publish_control_state_snapshot().await;
                let sampling_snapshot = std::iter::from_fn(|| gateway_rx.try_recv().ok())
                    .filter_map(|message| {
                        let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                            return None;
                        };
                        serde_json::from_str::<
                            crate::extensions::notification::SessionNotification,
                        >(args.request.params.get())
                        .ok()
                    })
                    .find_map(|notification| {
                        let crate::extensions::notification::SessionUpdate::ControlStateUpdate(
                            update,
                        ) = notification.update
                        else {
                            return None;
                        };
                        (update.domain == crate::extensions::notification::ControlDomain::Sampling)
                            .then_some(update)
                    })
                    .expect("Sampling snapshot");
                assert!(sampling_snapshot.snapshot);
                assert_eq!(
                    sampling_snapshot.phase,
                    crate::extensions::notification::ControlPhase::Applied
                );
                assert!(
                    sampling_snapshot.desired.is_none(),
                    "internal restoration must not leak into reconnect pending state"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn applied_control_receipt_is_terminal_before_ui_repair() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                let intent = crate::session::ControlIntent {
                    client_id: "control-client".into(),
                    generation: 7,
                    sequence: 11,
                };
                let target = crate::extensions::notification::ControlTarget::Agent {
                    agent_name: "coder".into(),
                };
                let mut state = actor.state.lock().await;
                assert!(matches!(
                    state.admit_control_intent(
                        crate::extensions::notification::ControlDomain::Agent,
                        Some(&intent),
                    ),
                    ControlIntentAdmission::New
                ));
                state.mark_control_intent_terminal(
                    crate::extensions::notification::ControlDomain::Agent,
                    Some(&intent),
                    ControlIntentTerminal {
                        phase: crate::extensions::notification::ControlPhase::Applied,
                        target: target.clone(),
                        message: Some("Agent switched to coder".into()),
                        ui_terminal_durable: false,
                    },
                );
                let ControlIntentAdmission::ExactTerminal(terminal) = state.admit_control_intent(
                    crate::extensions::notification::ControlDomain::Agent,
                    Some(&intent),
                ) else {
                    panic!("an applied control must not remain classified as in-flight");
                };
                assert_eq!(terminal.target, target);
                assert!(!terminal.ui_terminal_durable);

                state.mark_control_terminal_ui_durable(
                    crate::extensions::notification::ControlDomain::Agent,
                    &intent,
                );
                let ControlIntentAdmission::ExactTerminal(terminal) = state.admit_control_intent(
                    crate::extensions::notification::ControlDomain::Agent,
                    Some(&intent),
                ) else {
                    panic!("the repaired terminal receipt must remain exactly replayable");
                };
                assert!(terminal.ui_terminal_durable);
            })
            .await;
    }

    #[tokio::test]
    async fn identical_sampling_application_preserves_runtime_state() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                let current = actor.model_route.snapshot();
                let image_model = Some("provider/vision".to_owned());
                *actor.image_description_model.write() = image_model.clone();
                actor
                    .compactions_remaining
                    .set(Some(sampling_types::CompactionsRemaining::Fixed(4)));
                actor
                    .compaction_at_tokens
                    .set(Some(sampling_types::CompactionAtTokens::Fixed(1234)));
                actor.compaction.previous_model.set(Some(
                    crate::session::compaction_config::PreviousModelInfo {
                        model_slug: "previous-wire-model".to_owned(),
                        context_window: 16_000,
                    },
                ));
                actor.compaction.auto_compact_suppressed.store(
                    crate::session::compaction_config::SUPPRESS_STICKY,
                    std::sync::atomic::Ordering::Relaxed,
                );
                actor.model_auth_memo.replace(Some(ModelAuthMemo {
                    catalog_model_id: current.model_id.0.to_string(),
                    facts: crate::agent::config::ModelAuthFacts {
                        byok: crate::agent::auth_method::ModelByok::Byok,
                        auth_scheme: sampler::AuthScheme::default(),
                    },
                    provider: None,
                }));
                let mut credentials = actor.chat_state_handle.get_credentials().await;
                credentials.api_key = Some("runtime-auth-sentinel".to_owned());
                actor.chat_state_handle.update_credentials(credentials);

                let route = crate::agent::models::PublishedSessionRoute {
                    model_id: current.model_id.clone(),
                    sampling_config: current.sampling_config.clone(),
                    image_description_model: image_model,
                    inference_idle_timeout: actor.inference_idle_timeout.get(),
                    max_retries: actor.max_retries.get(),
                    auto_compact_threshold_percent: actor.compaction.threshold_percent.get(),
                };
                let catalog = SessionActor::published_catalog_for_test(
                    current.model_id.clone(),
                    current.sampling_config,
                    actor.image_description_model.read().clone(),
                    actor.inference_idle_timeout.get(),
                    actor.max_retries.get(),
                    actor.compaction.threshold_percent.get(),
                );
                let mut workflow_admission = actor.workflow_manager.lock().await;
                let (_, sampling_epoch_changed) = actor
                    .apply_user_model_selection(&mut workflow_admission, route, Some(catalog), None)
                    .await
                    .expect("identical sampling route must apply");
                drop(workflow_admission);

                assert!(!sampling_epoch_changed);
                assert_eq!(
                    actor
                        .chat_state_handle
                        .get_credentials()
                        .await
                        .api_key
                        .as_deref(),
                    Some("runtime-auth-sentinel")
                );
                assert!(actor.model_auth_memo.borrow().is_some());
                assert_eq!(
                    actor.compactions_remaining.get(),
                    Some(sampling_types::CompactionsRemaining::Fixed(4))
                );
                assert_eq!(
                    actor.compaction_at_tokens.get(),
                    Some(sampling_types::CompactionAtTokens::Fixed(1234))
                );
                assert_eq!(
                    actor.image_description_model.read().as_deref(),
                    Some("provider/vision")
                );
                assert_eq!(
                    actor
                        .compaction
                        .auto_compact_suppressed
                        .load(std::sync::atomic::Ordering::Relaxed),
                    crate::session::compaction_config::SUPPRESS_STICKY
                );
                assert_eq!(
                    actor
                        .compaction
                        .previous_model
                        .take()
                        .expect("no-op must retain previous model state")
                        .model_slug,
                    "previous-wire-model"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn effort_patch_uses_newest_actor_desired_model_not_stale_client_hint() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
                let actor = std::sync::Arc::new(
                    super::super::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                super::super::tests::support::begin_test_active_causal_turn(&actor).await;

                let mut selected = crate::agent::config::ModelEntry::baseline("new-wire-model");
                selected.info.reasoning_efforts = vec![sampling_types::ReasoningEffortOption {
                    id: "high".to_owned(),
                    value: sampling_types::ReasoningEffort::High,
                    label: "High".to_owned(),
                    description: None,
                    default: true,
                }];
                let mut catalog_config = crate::agent::config::Config::default();
                catalog_config.image_description_model = None;
                let manager = crate::agent::models::ModelsManager::new(
                    indexmap::IndexMap::from([("provider/new".to_owned(), selected)]),
                    crate::agent::models::ModelId::new("provider/new"),
                    catalog_config,
                );
                let catalog = std::sync::Arc::new(manager.published_catalog());
                let mut selected_config = sampler::SamplerConfig::default();
                selected_config.model = "new-wire-model".to_owned();
                let selected_route = SessionActor::selection_route_for_test(
                    crate::agent::models::ModelId::new("provider/new"),
                    selected_config,
                    85,
                );
                let (selection_tx, selection_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_session_model_selection(
                        selected_route,
                        Some(std::sync::Arc::clone(&catalog)),
                        None,
                        selection_tx,
                    )
                    .await;

                // The caller may still display a stale old-model hint, but
                // the actor's pending desired route is authoritative.
                let (effort_tx, mut effort_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_session_effort_patch(
                        sampling_types::ReasoningEffort::High,
                        crate::session::SessionEffortAuthority::Catalog {
                            catalog: std::sync::Arc::clone(&catalog),
                            origin_client: None,
                        },
                        None,
                        effort_tx,
                    )
                    .await;
                assert!(matches!(
                    selection_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Superseded
                ));
                assert!(matches!(
                    effort_rx.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));

                let boundary = actor
                    .end_step_control_boundary("continued")
                    .await
                    .expect("active step boundary must exist");
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
                    (true, false, false)
                );
                assert!(matches!(
                    effort_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Applied(model)
                        if model.0.as_ref() == "provider/new"
                ));
                let applied = actor.model_route.snapshot();
                assert_eq!(applied.model_id.0.as_ref(), "provider/new");
                assert_eq!(
                    applied.sampling_config.reasoning_effort,
                    Some(sampling_types::ReasoningEffort::High)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn stale_credential_refresh_cannot_cross_route_revision() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
                let actor = std::sync::Arc::new(
                    super::super::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                let old_route = actor.model_route.snapshot();
                let previous_key = actor.chat_state_handle.get_credentials().await.api_key;
                actor.model_route.replace(
                    crate::agent::models::ModelId::new("provider/new-model"),
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
                let actor = std::sync::Arc::new(
                    super::super::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                let mut route = actor.model_route.snapshot().sampling_config;
                route.auth_scheme = sampler::AuthScheme::XApiKey;
                route.bearer_resolver = Some(std::sync::Arc::new(EmptyBearerResolver));
                actor.model_route.replace(
                    crate::agent::models::ModelId::new("removed-provider/frozen-model"),
                    route,
                );

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
                actor.model_route.replace(
                    crate::agent::models::ModelId::new("bigmodel/glm-5.3"),
                    route,
                );

                assert_eq!(actor.current_catalog_model_id(), "bigmodel/glm-5.3");
                assert_eq!(
                    actor.build_session_info().await.model.as_deref(),
                    Some("bigmodel/glm-5.3")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn busy_sampling_desired_state_supersedes_intermediate_request() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                actor.state.lock().await.foreground = ForegroundState::Compaction;

                let mut first = actor.model_route.snapshot().sampling_config;
                first.model = "first-wire".into();
                let first_catalog = SessionActor::published_catalog_for_test(
                    crate::agent::models::ModelId::new("provider/first"),
                    first.clone(),
                    None,
                    std::time::Duration::from_secs(60),
                    3,
                    80,
                );
                let (first_tx, first_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_session_model_selection(
                        SessionActor::selection_route_for_test(
                            crate::agent::models::ModelId::new("provider/first"),
                            first,
                            80,
                        ),
                        Some(first_catalog),
                        None,
                        first_tx,
                    )
                    .await;

                let mut final_route = actor.model_route.snapshot().sampling_config;
                final_route.model = "final-wire".into();
                let final_catalog = SessionActor::published_catalog_for_test(
                    crate::agent::models::ModelId::new("provider/final"),
                    final_route.clone(),
                    None,
                    std::time::Duration::from_secs(90),
                    4,
                    70,
                );
                let (final_tx, mut final_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_session_model_selection(
                        SessionActor::selection_route_for_test(
                            crate::agent::models::ModelId::new("provider/final"),
                            final_route,
                            70,
                        ),
                        Some(final_catalog),
                        None,
                        final_tx,
                    )
                    .await;

                assert_eq!(
                    first_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Superseded
                );
                assert!(matches!(
                    final_rx.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                actor.state.lock().await.foreground = ForegroundState::Idle;
                actor.apply_pending_step_controls_if_idle().await;
                assert!(matches!(
                    final_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Applied(model)
                        if model.0.as_ref() == "provider/final"
                ));
                assert_eq!(
                    actor.model_route.snapshot().model_id.0.as_ref(),
                    "provider/final"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn busy_agent_desired_state_discards_stale_preparation() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                actor.state.lock().await.foreground = ForegroundState::Compaction;

                let mut first = agent::AgentDefinition::default_grow_build();
                first.name = "first-agent".into();
                let (first_tx, first_rx) = tokio::sync::oneshot::channel();
                actor.admit_agent_selection(first, None, first_tx).await;

                let mut final_agent = agent::AgentDefinition::default_grow_build();
                final_agent.name = "final-agent".into();
                let (final_tx, mut final_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(final_agent, None, final_tx)
                    .await;

                assert_eq!(
                    first_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Superseded
                );
                assert!(matches!(
                    final_rx.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                actor.state.lock().await.foreground = ForegroundState::Idle;
                actor.apply_pending_step_controls_if_idle().await;
                assert_eq!(
                    final_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Applied(())
                );
                assert_eq!(actor.agent.borrow().name(), "final-agent");
            })
            .await;
    }

    #[tokio::test]
    async fn idle_agent_desired_state_remains_supersedable_while_worker_applies() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;

                let mut first = agent::AgentDefinition::default_grow_build();
                first.name = "idle-first-agent".into();
                let (first_tx, first_rx) = tokio::sync::oneshot::channel();
                actor.admit_agent_selection(first, None, first_tx).await;

                let mut final_agent = agent::AgentDefinition::default_grow_build();
                final_agent.name = "idle-final-agent".into();
                let (final_tx, final_rx) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(final_agent, None, final_tx)
                    .await;

                assert_eq!(
                    first_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Superseded,
                    "the actor mailbox must remain free to replace an idle Agent target"
                );
                assert_eq!(
                    tokio::time::timeout(std::time::Duration::from_secs(2), final_rx)
                        .await
                        .expect("idle control worker must finish")
                        .unwrap()
                        .unwrap(),
                    crate::session::DesiredStateOutcome::Applied(())
                );
                assert_eq!(actor.agent.borrow().name(), "idle-final-agent");
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    loop {
                        if actor.state.lock().await.foreground.is_idle() {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("idle control worker must release its foreground fence");
            })
            .await;
    }

    #[tokio::test]
    async fn step_boundary_retargets_a_sampling_revision_superseded_before_commit() {
        let mut controls = PendingStepControls::default();
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        controls.admit_sampling(
            SessionActor::selection_route_for_test(
                crate::agent::models::ModelId::new("provider/first"),
                sampler::SamplerConfig::default(),
                85,
            ),
            None,
            None,
            first_tx,
        );
        let boundary = controls.boundary();

        let (next_tx, _next_rx) = tokio::sync::oneshot::channel();
        let (_, superseded) = controls.admit_sampling(
            SessionActor::selection_route_for_test(
                crate::agent::models::ModelId::new("provider/next"),
                sampler::SamplerConfig::default(),
                85,
            ),
            None,
            None,
            next_tx,
        );

        assert!(
            superseded.is_some(),
            "the previous desired target is retired"
        );
        drop(first_rx);
        assert!(matches!(
            controls.next_key_at_boundary(boundary),
            Some(PendingStepControlKey::Sampling { revision: 2, .. })
        ));
    }

    #[tokio::test]
    async fn step_boundary_defers_a_control_domain_first_admitted_after_step_ended() {
        let mut controls = PendingStepControls::default();
        let boundary = controls.boundary();
        let (next_tx, _next_rx) = tokio::sync::oneshot::channel();
        controls.admit_sampling(
            SessionActor::selection_route_for_test(
                crate::agent::models::ModelId::new("provider/next"),
                sampler::SamplerConfig::default(),
                85,
            ),
            None,
            None,
            next_tx,
        );

        assert!(
            controls.next_key_at_boundary(boundary).is_none(),
            "a domain first admitted after StepEnded must wait for the next boundary"
        );
        assert!(controls.next_key().is_some());
    }

    #[tokio::test]
    async fn agent_admission_cannot_enter_the_boundary_after_step_ended_is_durable() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                super::super::tests::support::begin_test_active_causal_turn(&actor).await;
                let actor = std::sync::Arc::new(actor);
                let gate = actor.step_control_gate.lock().await;
                assert!(actor.events.end_step("continued"));

                let mut definition = agent::AgentDefinition::default_grow_build();
                definition.name = "next-boundary-agent".into();
                let (responds_to, _response) = tokio::sync::oneshot::channel();
                let admission = tokio::task::spawn_local({
                    let actor = std::sync::Arc::clone(&actor);
                    async move {
                        actor
                            .admit_agent_selection(definition, None, responds_to)
                            .await;
                    }
                });
                tokio::task::yield_now().await;

                let boundary = actor.state.lock().await.pending_step_controls.boundary();
                assert!(
                    !boundary.agent_eligible,
                    "an Agent request blocked behind the StepEnded gate belongs to the next Step"
                );
                drop(gate);
                admission.await.unwrap();

                let controls = &actor.state.lock().await.pending_step_controls;
                assert!(controls.next_key_at_boundary(boundary).is_none());
                assert!(matches!(
                    controls.next_key(),
                    Some(PendingStepControlKey::Agent { .. })
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn step_boundary_applies_latest_agent_when_claimed_preparation_is_superseded() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                super::super::tests::support::begin_test_active_causal_turn(&actor).await;
                let actor = std::sync::Arc::new(actor);

                let stalled = std::rc::Rc::new(AgentPreparation {
                    target_name: "stalled-agent".into(),
                    result: std::cell::RefCell::new(None),
                    superseded: std::cell::Cell::new(false),
                    ready: tokio::sync::Notify::new(),
                    abort: std::cell::RefCell::new(None),
                });
                let (stalled_tx, stalled_rx) = tokio::sync::oneshot::channel();
                actor
                    .state
                    .lock()
                    .await
                    .pending_step_controls
                    .admit_agent(stalled, None, stalled_tx);
                let boundary = actor
                    .end_step_control_boundary("continued")
                    .await
                    .expect("active Step boundary");
                let applying = {
                    let actor = actor.clone();
                    tokio::task::spawn_local(async move {
                        actor
                            .apply_pending_controls_at_step_boundary(boundary)
                            .await
                    })
                };
                tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    loop {
                        let claimed = actor
                            .state
                            .lock()
                            .await
                            .applying_step_control
                            .as_ref()
                            .is_some_and(|projection| {
                                matches!(
                                    &projection.target,
                                    crate::extensions::notification::ControlTarget::Agent {
                                        agent_name
                                    } if agent_name == "stalled-agent"
                                )
                            });
                        if claimed {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("the stale preparation must be claimed before replacement");

                let mut latest = agent::AgentDefinition::default_grow_build();
                latest.name = "latest-agent".into();
                let (latest_tx, latest_rx) = tokio::sync::oneshot::channel();
                actor.admit_agent_selection(latest, None, latest_tx).await;

                assert_eq!(
                    stalled_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Superseded
                );
                assert_eq!(
                    tokio::time::timeout(std::time::Duration::from_secs(2), applying)
                        .await
                        .expect("replacement must wake the stale preparation wait")
                        .unwrap(),
                    (false, true, false)
                );
                assert_eq!(
                    latest_rx.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Applied(())
                );
                assert_eq!(actor.agent.borrow().name(), "latest-agent");
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
                let actor = std::sync::Arc::new(
                    super::super::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                actor.state.lock().await.foreground = ForegroundState::Compaction;

                let mut first = sampler::SamplerConfig::default();
                first.model = "first-wire".into();
                first.base_url = "https://first.example/v1".into();
                first.context_window = 32_000;
                let first_catalog = SessionActor::published_catalog_for_test(
                    crate::agent::models::ModelId::new("first/catalog"),
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
                    crate::agent::models::ModelId::new("latest/catalog"),
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
                let actor = std::sync::Arc::new(
                    super::super::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                actor.state.lock().await.foreground = ForegroundState::Compaction;

                let mut first_reload = sampler::SamplerConfig::default();
                first_reload.model = "reload-one-wire".into();
                first_reload.context_window = 32_000;
                let first_catalog = SessionActor::published_catalog_for_test(
                    crate::agent::models::ModelId::new("reload/one"),
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
                    crate::agent::models::ModelId::new("user/selection"),
                    selected.clone(),
                    None,
                    std::time::Duration::from_secs(300),
                    3,
                    80,
                );
                actor
                    .admit_session_model_selection(
                        SessionActor::selection_route_for_test(
                            crate::agent::models::ModelId::new("user/selection"),
                            selected,
                            80,
                        ),
                        Some(selection_catalog),
                        None,
                        selection_tx,
                    )
                    .await;

                let mut second_reload = sampler::SamplerConfig::default();
                second_reload.model = "user-wire-refreshed".into();
                second_reload.context_window = 96_000;
                let second_catalog = SessionActor::published_catalog_for_test(
                    crate::agent::models::ModelId::new("user/selection"),
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
                    .replace(crate::agent::models::ModelId::new("catalog-alias"), route);
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
                let actor = std::sync::Arc::new(actor);
                let (responds_to, response) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(definition, None, responds_to)
                    .await;
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
                    .admit_agent_selection(replay_definition, None, responds_to)
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

                let actor = std::sync::Arc::new(actor);
                let (responds_to, mut response) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(definition, None, responds_to)
                    .await;
                assert!(matches!(
                    response.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));
                assert_eq!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(surface_before).unwrap()
                );
                assert_ne!(actor.agent.borrow().name(), "reviewer");

                let boundary = actor
                    .end_step_control_boundary("continued")
                    .await
                    .expect("active model-switch step boundary");
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
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
                let agent_role_count_before = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .into_iter()
                    .filter(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Control(control)
                                if matches!(control.model_contexts.as_slice(), [context]
                                    if context.layer
                                        == chat_state::ControlContextLayer::AgentRole)
                        )
                    })
                    .count();
                let actor = std::sync::Arc::new(actor);
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
                    crate::agent::models::ModelId::new("catalog/next"),
                    config,
                );
                let selection_catalog = std::sync::Arc::new(selected_manager.published_catalog());
                let selected_route = selection_catalog
                    .resolve_session_route(
                        &crate::agent::models::ModelId::new("catalog/next"),
                        Some(sampling_types::ReasoningEffort::High),
                    )
                    .unwrap();

                actor
                    .admit_session_model_selection(
                        selected_route,
                        Some(selection_catalog),
                        None,
                        responds_to,
                    )
                    .await;
                let mut next_agent = agent::AgentDefinition::default_grow_build();
                next_agent.name = "step-reviewer".into();
                let (agent_responds_to, mut agent_response) = tokio::sync::oneshot::channel();
                actor
                    .admit_agent_selection(next_agent, None, agent_responds_to)
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

                let boundary = actor
                    .end_step_control_boundary("continued")
                    .await
                    .expect("active combined-control step boundary");
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
                    (true, true, false)
                );
                assert!(matches!(
                    response.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Applied(model)
                        if model.0.as_ref() == "catalog/next"
                ));
                assert_eq!(
                    agent_response.await.unwrap().unwrap(),
                    crate::session::DesiredStateOutcome::Applied(())
                );
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
                actor.events.emit(Event::LoopStarted {
                    loop_index: actor.events.next_step_index(),
                });
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
                let next_step_started = events
                    .iter()
                    .rposition(|event| {
                        matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Step(
                                chat_state::StepEvent::Started { .. }
                            )
                        )
                    })
                    .expect("the next step must start after controls settle");
                assert!(
                    step_ended < model_changed_event
                        && model_changed_event < agent_changed_event
                        && agent_changed_event < next_step_started,
                    "causal order must be StepEnded → Sampling → Agent rebuild → StepStarted"
                );
                let agent_role_count_after = events
                    .iter()
                    .filter(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Control(control)
                                if matches!(control.model_contexts.as_slice(), [context]
                                    if context.layer
                                        == chat_state::ControlContextLayer::AgentRole)
                        )
                    })
                    .count();
                assert_eq!(
                    agent_role_count_after,
                    agent_role_count_before + 1,
                    "one boundary may publish exactly one Agent-role reprojection"
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
