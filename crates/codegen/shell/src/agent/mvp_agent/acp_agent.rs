#![cfg_attr(rustfmt, rustfmt::skip)]
#![allow(unused_imports)]
//! [`acp::Agent`] trait implementation for [`MvpAgent`].
//! Co-located child of `mvp_agent` (`use super::*`).
use super::*;
#[async_trait::async_trait(?Send)]
impl acp::Agent for MvpAgent {
    /// In the meta, we provide
    ///   - model_state: the model state, useful for the client to display available models and the default model.
    ///
    /// SINGLE-CALL INVARIANT: this method is the sole writer of
    /// `self.auth_method_id` during initialization. It is called exactly once
    /// per agent process by the ACP server before any session-creating
    /// requests, while `auth_method_id` is still `None` (initialized at
    /// `MvpAgent::new`). The auth-method block below relies on that
    /// invariant when it unconditionally writes the default id returned by
    /// `auth_method::build_auth_methods`. If you ever need to call
    /// `initialize()` more than once, restore an `is_none()` guard around
    /// the `auth_method_id` write at the call site so a re-init doesn't
    /// silently downgrade an api-key user to a session-token user.
    async fn initialize(
        &self,
        arguments: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        tracing::debug!(target: "sampling_log", "Received initialize request");
        ::diagnostics::unified_log::info("agent initialized", None, None);
        self.start_subagent_coordinator();
        let auto_gc_policy = self.cfg.borrow().resolve_worktree_auto_gc();
        tokio::task::spawn_blocking(move || {
            crate::session::worktree_pool::cleanup_stale_pool_worktrees(None);
            let opts = fast_worktree::AutoGcOptions::from_resolved(auto_gc_policy);
            if let Err(e) = fast_worktree::WorktreeDb::open_default()
                .and_then(|db| fast_worktree::maybe_auto_gc(&db, &opts))
            {
                tracing::warn!(error = %e, "auto worktree gc failed");
            }
        });
        tokio::task::spawn_blocking(|| {
            crate::session::persistence::cleanup_stale_sessions(None);
        });
        {
            let root = crate::util::grow_home::grow_home();
            crate::session::storage::search::SEARCH_INDEX_MANAGER.bootstrap_once(root);
        }
        const PERMISSION_CLEANUP_TTL_DAYS: u64 = 30;
        static CLEANUP_PERMISSIONS_ONCE: std::sync::Once = std::sync::Once::new();
        CLEANUP_PERMISSIONS_ONCE
            .call_once(|| {
                tokio::task::spawn(
                    workspace::permission::cleanup_stale_permission_state(
                        std::time::Duration::from_secs(
                            PERMISSION_CLEANUP_TTL_DAYS * 24 * 60 * 60,
                        ),
                    ),
                );
            });
        let mut client_type = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("clientType"))
            .and_then(|v| serde_json::from_value::<ClientType>(v.clone()).ok())
            .unwrap_or_default();
        let client_identifier = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("clientIdentifier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(ref id) = client_identifier {
            tracing::info!("Client identifier set to: {}", id);
        }
        if client_type == ClientType::Generic {
            match client_identifier.as_deref() {
                Some("grow-web") => client_type = ClientType::GrowWeb,
                Some("nebula") => client_type = ClientType::Nebula,
                Some("grow-code-extension") => client_type = ClientType::Extension,
                _ => {}
            }
        }
        *self.client_type.borrow_mut() = client_type;
        tracing::info!("Client type set to: {:?}", client_type);
        let code_nav_enabled = Self::parse_code_nav_capability(&arguments);
        self.code_nav_enabled.set(code_nav_enabled);
        tracing::info!(
            code_nav_enabled,
            client_type = ?client_type,
            event = "code_nav_capability_parsed",
            "code-nav capability initialized from initialize request; \
             index will start lazily on first grow/code/* request if eligible"
        );
        let client_supports_mcp_apps = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("mcpApps"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if client_supports_mcp_apps {
            tracing::info!("Client supports MCP Apps");
        }
        let buffering_settings = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("bufferingSettings"))
            .map(|value| serde_json::from_value::<
                update_chunk_merge::BufferingSettings,
            >(value.clone()))
            .transpose()
            .map_err(|err| {
                tracing::warn!(
                    error = ?err,
                    "Failed to parse buffering settings from init meta"
                );
                err
            })
            .unwrap_or(None);
        tracing::info!(?buffering_settings, "Buffering settings from init");
        *self.buffering_settings.borrow_mut() = buffering_settings;
        if self.initialize_request.set(arguments).is_err() {
            tracing::info!("Initialize called on reconnect (already initialized)");
        }
        let has_byok_provider = auth_method::should_advertise_provider_api_key(
            self.models_manager.models().values(),
        );
        if !has_byok_provider {
            return Err(acp::Error::auth_required().data(
                "no BYOK provider is configured; add a provider/model to ~/.grow/config.toml",
            ));
        }
        let built = auth_method::build_auth_methods();
        let auth_methods = built.methods;
        let default_auth_method_id_wire = Some(built.default_auth_method_id.0.to_string());
        self.set_auth_method(built.default_auth_method_id);
        let current_working_directory = self.launch_cwd.clone();
        let hostname = gethostname::gethostname();
        let mcp_servers: Vec<crate::extensions::mcp::McpServerEntry> = Vec::new();
        self.spawn_initialize_launch_mcp_setup();
        {
            let agent_ref = LocalRef::new(self);
            tokio::task::spawn_local(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                agent_ref.get().emit_announcements();
            });
        }
        let init_model_state = self.model_state(None);
        Ok(
            acp::InitializeResponse::new(acp::ProtocolVersion::V1)
                .agent_capabilities(
                    acp::AgentCapabilities::new()
                        .load_session(true)
                        .meta(
                            serde_json::json!({
                    "grow/fs_notify": true,
                    // Advertised so SDKs can warn when a registration depends on
                    // hook behavior this agent doesn't honor.
                    "grow/hooks": {
                        "blockingEvents": crate::extensions::hooks::ADVERTISED_BLOCKING_EVENTS,
                        "decisions": crate::extensions::hooks::ADVERTISED_DECISIONS,
                        "stopSignals": crate::extensions::hooks::ADVERTISED_STOP_SIGNALS,
                    },
                })
                                .as_object()
                                .cloned(),
                        )
                        .prompt_capabilities(
                            acp::PromptCapabilities::new().embedded_context(true),
                        )
                        .mcp_capabilities(
                            acp::McpCapabilities::new().http(true).sse(true),
                        ),
                )
                .auth_methods(auth_methods)
                .meta({
                    let metadata = parse_json_object_env("GROW_AGENT_METADATA");
                    serde_json::json!({
                    "growShell": true,
                    // Clients consume the agent's single BYOK method from here.
                    "defaultAuthMethodId": default_auth_method_id_wire,
                    // The agent can drive in-process SDK MCP servers over the ACP reverse
                    // channel (`grow/mcp/sdk_call`); the SDK reads this to enable transport="acp".
                    (mcp::wire::MCP_SDK): true,
                    // `session/new` / `session/load` accept per-session plugin roots in
                    // `_meta.pluginDirs`; the SDKs gate `GrowOptions.plugins` on this.
                    (SESSION_PLUGIN_DIRS_CAPABILITY_KEY): true,
                    "currentWorkingDirectory": current_working_directory.to_string_lossy().to_string(),
                    "agentVersion": version::VERSION,
                    "hostname": hostname.to_string_lossy().to_string(),
                    "modelState": init_model_state,
                    "mcpServers": mcp_servers,
                    "mcpApps": client_supports_mcp_apps,
                    "metadata": metadata,
                    "availableCommands": crate::session::slash_commands::builtin_commands(self.command_availability()),
                    "cancelRewind": self.cfg.borrow().resolve_cancel_rewind().value,
                    // Resolved session-recap state (remote settings / config / env;
                    // default ON). The client gates BOTH its automatic
                    // away-recap poll and the manual `/recap` on this so a
                    // disabled feature produces zero `grow/recap` traffic.
                    "sessionRecap": self.cfg.borrow().is_session_recap_enabled(),
                })
                        .as_object()
                        .cloned()
                }),
        )
    }
    async fn authenticate(
        &self,
        arguments: acp::AuthenticateRequest,
    ) -> Result<AuthenticateResponse, acp::Error> {
        if arguments.method_id.0.as_ref() != auth_method::PROVIDER_API_KEY_METHOD_ID {
            return Err(acp::Error::invalid_params().data(format!(
                "unsupported auth method: {}; Grow is BYOK-only",
                arguments.method_id.0
            )));
        }

        if let Ok(api_key) = auth_method::read_provider_api_key_env() {
            self.sampling_config.borrow_mut().api_key = Some(api_key);
        }
        if !auth_method::should_advertise_provider_api_key(
            self.models_manager.models().values(),
        ) {
            return Err(acp::Error::auth_required().data(
                "no BYOK provider is configured; set api_key/env_key/auth_provider in config.toml",
            ));
        }

        self.set_auth_method(arguments.method_id);
        Ok(Default::default())
    }
    async fn new_session(
        &self,
        arguments: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let catalog_transaction = self.model_reload_lock.lock().await;
        tracing::debug!(config = ?self.sampling_config, "Received new session request {arguments:?}");
        let init = self
            .initialize_request
            .get()
            .ok_or_else(|| {
                acp::Error::invalid_params()
                    .data("initialize must be called before new_session")
            })?;
        self.seed_client_config_auth_if_available();
        let cwd = AbsPathBuf::new(arguments.cwd.clone())
            .map_err(|e| acp::Error::invalid_params().data(e.to_string()))?;
        let remote_settings = self.cfg.borrow().remote_settings.clone();
        folder_trust::resolve_and_record(cwd.as_path(), remote_settings.as_ref(), false);
        let initial_client_mcp_servers = arguments.mcp_servers.clone();
        let mcp_servers = self
            .resolve_mcp_servers(arguments.mcp_servers, cwd.as_path());
        let mcp_meta_config_map = parse_mcp_meta_config(arguments.meta.as_ref());
        let client_session_id = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("sessionId"))
            .and_then(|v| v.as_str());
        let custom_model_id = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("modelId").and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty());
        let session_permission_mode = resolve_session_permission_mode(
            arguments.meta.as_ref(),
            self.default_permission_mode,
        )?;
        let session_id = match client_session_id {
            Some(s) => {
                uuid::Uuid::try_parse(s)
                    .map_err(|e| {
                        acp::Error::invalid_params()
                            .data(
                                format!(
                        "Invalid UUID format for _meta.sessionId '{}': {}",
                        s, e
                    ),
                            )
                    })?;
                acp::SessionId::new(s.to_string())
            }
            None => acp::SessionId::new(uuid::Uuid::now_v7().to_string()),
        };
        let mut session_timer = crate::instrumentation_timer!("session.new_session");
        session_timer.with_field("session_id", session_id.0.as_ref());
        session_timer.with_field("cwd", cwd.as_str());
        let client_identifier = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("clientIdentifier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                self
                    .initialize_request
                    .get()
                    .and_then(|req| req.meta.as_ref())
                    .and_then(|m| m.get("clientIdentifier"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });
        ::diagnostics::session_ctx::log_session_event(crate::agent::session_metrics::SessionStarted {
            session_id: session_id.0.to_string(),
        });
        let session_info = SessionInfo {
            id: session_id.clone(),
            cwd: cwd.as_str().to_owned(),
        };
        let mut session_sampling_override: Option<SamplingConfig> = None;
        let mut disallowed_custom: Option<String> = None;
        let campaign_nudge = crate::util::config::campaign_driven_models_default().filter(|c| {
            custom_model_id.is_none()
                || custom_model_id == c.pre_campaign.as_deref()
                || custom_model_id == Some(c.value.as_str())
        });
        let campaign_nudged = campaign_nudge.is_some();
        if let Some(c) = &campaign_nudge {
            tracing::info!(
                model = %c.value,
                requested = ?custom_model_id,
                "new_session: applying campaign-driven default model"
            );
        }
        let build_custom_model_id: Option<String> = campaign_nudge
            .map(|c| c.value)
            .or_else(|| custom_model_id.map(str::to_owned));
        let resolved_custom_model = build_custom_model_id
            .as_deref()
            .and_then(|custom_model| match self
                .resolve_model_id(&acp::ModelId::new(custom_model))
            {
                Ok(model) if model.info.user_selectable => {
                    let origin_client = self
                        .origin_client_info_from_meta(arguments.meta.as_ref());
                    session_sampling_override = Some(
                        self.prepare_sampling_config_for_model(&model, origin_client),
                    );
                    Some(custom_model)
                }
                Ok(_) => {
                    tracing::warn!(
                        requested_model = custom_model,
                        "Requested model not allowed by allowed_models; falling back to current default model"
                    );
                    if !campaign_nudged {
                        disallowed_custom = Some(custom_model.to_string());
                    }
                    None
                }
                Err(_) => {
                    tracing::warn!(
                        requested_model = custom_model,
                        fallback_model = %self.models_manager.current_model_id().0,
                        "Requested model not found, falling back to current default model"
                    );
                    None
                }
            });
        let origin_client = self.origin_client_info_from_meta(arguments.meta.as_ref());
        let mut session_sampling = session_sampling_override
            .unwrap_or_else(|| {
                self
                    .resolve_sampling_config_for_model(
                        &self.models_manager.current_model_id(),
                        origin_client.clone(),
                    )
            });
        let (model_id, fallback_notice) = crate::agent::models::resolve_new_session_model_id(
            &self.models_manager.models(),
            resolved_custom_model,
            &self.models_manager.current_model_id(),
        );
        if let Some(notice) = fallback_notice {
            session_sampling =
                self.resolve_sampling_config_for_model(&model_id, origin_client.clone());
            tracing::warn!(
                requested_model = %notice.requested.0,
                fallback_model = %model_id.0,
                "new_session: requested catalog model disappeared; falling back to the default model"
            );
            self.send_model_auto_switched(&session_id, &notice.requested, &model_id, &notice.reason)
                .await;
        }
        if let Some(effort) = self
            .models_manager
            .model_default_reasoning_effort(model_id.0.as_ref())
        {
            session_sampling.reasoning_effort = Some(effort);
        }
        let (summary_client, summary_model) = self.build_session_title_client(&session_sampling)?;
        let session_title_route = Some(crate::session::actor::summary::SessionTitleRoute::new(
            summary_client,
            summary_model,
        ));
        let session_model_id = model_id.clone();
        let _timer = crate::instrumentation_timer!("session.persistence_init");
        let persistence = crate::session::persistence::new(
            &session_info,
            model_id,
            Some(self.gateway.clone()),
        )
        .await
        .map_err(|e| crate::session::persistence::io_error_to_acp(&e))?;
        self.set_turn_number(&session_id, 0u64);
        let client_code_nav_enabled = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("codeNavEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| self.code_nav_enabled.get());
        let (client_terminal, client_fs_read, client_fs_write) = Self::resolve_client_io_caps(
            arguments.meta.as_ref(),
            init,
        );
        let spawn_res = {
            let mut timer = crate::instrumentation_timer!("session.spawn_session_actor");
            timer.with_field("session_id", session_id.0.as_ref());
            let spawn_opts = SessionSpawnOptions {
                        session_info: session_info.clone(),
                        cwd: cwd.clone(),
                        mcp_servers,
                        initial_client_mcp_servers,
                        mcp_meta_config_map,
                        persistence,
                        session_title_route,
                        timeline_bootstrap: crate::session::TimelineBootstrap::Fresh {
                            session_rules: session_rules_from_meta(
                                arguments.meta.as_ref(),
                                init.meta.as_ref(),
                            ),
                        },
                        rewind_points_source: None,
                        origin_client: origin_client.clone(),
                        client_code_nav_enabled,
                        client_terminal,
                        client_fs_read,
                        client_fs_write,
                        preloaded_envrc: None,
                        persisted_signals: None,
                        persisted_behavior: None,
                        persisted_goal_mode: None,
                        persisted_control_revision: 0,
                        persisted_workflow_runs: Vec::new(),
                        persisted_announcement_state: None,
                        session_meta: arguments.meta.as_ref(),
                        persisted_agent_name: None,
                        session_model_id,
                        session_permission_mode,
                        prompt_display_cwd: None,
            };
            self.spawn_and_register_session(init, spawn_opts).await
        };
        spawn_res?;
        tracing::debug!(session_id = %session_id.0, "new_session: spawn_session_actor");
        {
            let sid = session_id.0.to_string();
            let ci = client_identifier.clone();
            let cv = self.client_version();
            let cwd_str = cwd.as_str().to_owned();
            let perm = session_permission_mode;
            tokio::spawn(async move {
                let git = ::diagnostics::context::collect_git_context(&cwd_str);
                let ev = ::diagnostics::events::SessionNew {
                    session_id: sid,
                    client_identifier: ci,
                    client_version: cv,
                    is_git_repo: git.is_git_repo,
                    permission_mode: perm,
                };
                ::diagnostics::session_ctx::log_event(ev);
            });
        }
        let enqueued_custom_model = if let Some(model_id) = resolved_custom_model {
            Some(crate::timed!(log: "new_session: enqueue_session_model", {
                self.control_session_handle(&session_id)
                    .ok_or_else(|| acp::Error::internal_error().data("new session actor missing"))
                    .and_then(|handle| {
                        crate::agent::handlers::model_switch::enqueue(
                            self,
                            &catalog_transaction,
                            handle,
                            acp::SetSessionModelRequest::new(
                                session_id.clone(),
                                acp::ModelId::new(model_id),
                            ),
                        )
                    })
            }))
        } else {
            None
        };
        drop(catalog_transaction);
        if let Some(enqueued) = enqueued_custom_model {
            let _ = match enqueued {
                Ok(enqueued) => crate::agent::handlers::model_switch::finish(self, enqueued).await,
                Err(error) => Err(error),
            };
            tracing::debug!(session_id = %session_id.0, "new_session: set_session_model");
        }
        if let Some(requested) = disallowed_custom {
            let current = self.models_manager.current_model_id();
            let reason = format!(
                "\"{requested}\" isn't allowed by your allowed_models setting, so this session is using \"{}\".",
                current.0
            );
            self.send_model_auto_switched(
                    &session_id,
                    &acp::ModelId::new(requested),
                    &current,
                    &reason,
                )
                .await;
        }
        let indexed_roots = self.indexed_roots_for(cwd.as_path());
        let (git_root, is_git_repo, discovery_failed) = match workspace::session::git::discover_git_root(
            cwd.as_path(),
        ) {
            GitDiscoveryResult::Found(root) => {
                let root_str = root.to_string_lossy().trim_end_matches('/').to_string();
                (Some(root_str), true, false)
            }
            GitDiscoveryResult::NotARepo => {
                tracing::debug!("new_session: not a git repository");
                (None, false, false)
            }
            GitDiscoveryResult::DiscoveryFailed(e) => {
                tracing::warn!(
                        error = %e,
                        cwd = %cwd.as_str(),
                        "new_session: git repo discovery failed unexpectedly"
                    );
                (None, false, true)
            }
        };
        let show_non_git_warning = {
            let cfg = self.cfg.borrow();
            !is_git_repo && !discovery_failed
                && cfg
                    .remote_settings
                    .as_ref()
                    .and_then(|s| s.non_git_warning)
                    .unwrap_or(cfg.features.non_git_warning)
        };
        ::diagnostics::unified_log::info(
            "session created",
            Some(session_id.0.as_ref()),
            Some(serde_json::json!({"cwd": cwd.as_str()})),
        );
        let models = self.model_state(Some(&session_id));
        let mut meta = serde_json::json!({
            "currentWorkingDirectory": cwd.as_str().to_owned(),
            "codebaseIndexed": indexed_roots,
            "isGitRepo": is_git_repo,
            "gitRoot": git_root,
            "showNonGitWarning": show_non_git_warning,
        });
        if let Some(obj) = meta.as_object_mut() {
            self.insert_session_config_meta(
                obj,
                &session_id,
                cwd.as_str().to_owned(),
                None,
                &models,
            );
        }
        Ok(
            acp::NewSessionResponse::new(session_id)
                .models(Some(models))
                .meta(meta.as_object().cloned()),
        )
    }
    async fn load_session(
        &self,
        arguments: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionResponse, acp::Error> {
        let _load_guard = self.begin_session_load(&arguments.session_id);
        let catalog_transaction = self.model_reload_lock.lock().await;
        self.sweep_dead_sessions();
        self.drain_old_session_thread(&arguments.session_id).await;
        tracing::debug!("Received load session request {arguments:?}");
        let init = self
            .initialize_request
            .get()
            .ok_or_else(|| {
                acp::Error::invalid_params()
                    .data("initialize must be called before load_session")
            })?;
        self.seed_client_config_auth_if_available();
        let persist_data = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("grow/persist"))
            .cloned();
        let target_client_id = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("grow/leaderClientId"))
            .cloned();
        let acp::LoadSessionRequest {
            session_id,
            cwd,
            mcp_servers: client_mcp_servers,
            meta: request_meta,
            ..
        } = arguments;
        let cwd = AbsPathBuf::new(cwd)
            .map_err(|e| acp::Error::invalid_params().data(e.to_string()))?;
        let remote_settings = self.cfg.borrow().remote_settings.clone();
        folder_trust::resolve_and_record(cwd.as_path(), remote_settings.as_ref(), false);
        let initial_client_mcp_servers = client_mcp_servers.clone();
        let mcp_servers = self
            .resolve_mcp_servers(client_mcp_servers, cwd.as_path());
        let mcp_meta_config_map = parse_mcp_meta_config(request_meta.as_ref());
        let mut load_timer = crate::instrumentation_timer!("session.load_session");
        load_timer.with_field("session_id", session_id.0.as_ref());
        load_timer.with_field("cwd", cwd.as_str());
        let git_root = workspace::session::git::find_git_root_from_path(
                cwd.as_path(),
            )
            .ok();
        if let Some(root) = git_root {
            tokio::task::spawn_blocking(move || {
                crate::session::worktree_pool::cleanup_stale_pool_worktrees(Some(&root));
            });
        }
        ::diagnostics::session_ctx::log_session_event(crate::agent::session_metrics::SessionStarted {
            session_id: session_id.0.to_string(),
        });
        let session_info = SessionInfo {
            id: session_id.clone(),
            cwd: cwd.as_str().to_owned(),
        };
        let current_session_dir = crate::session::persistence::session_dir(
            &session_info,
        );
        tokio::task::spawn_blocking(move || {
            crate::session::persistence::cleanup_stale_sessions(
                Some(&current_session_dir),
            );
        });
        let session_exists = self
            .sessions
            .borrow()
            .get(&session_id)
            .is_some_and(|handle| !handle.cmd_tx.is_closed());
        if session_exists {
            tracing::info!(
                session_id = %session_id.0,
                "Reconnect detected: flushing persistence buffer before replay"
            );
            if let Some(handle) = self.sessions.borrow().get(&session_id) {
                handle
                    .gateway_enabled
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
            let mut flush_timer = crate::instrumentation_timer!("session.reconnect_flush");
            flush_timer.with_field("session_id", session_id.0.as_ref());
            if let Err(reason) = self.flush_session(&session_id).await {
                tracing::warn!(
                    session_id = %session_id.0,
                    reason,
                    "Reconnect flush failed"
                );
            }
            drop(flush_timer);
        }
        let origin_client = self.origin_client_info_from_meta(request_meta.as_ref());
        let load_session_sampling = self
            .resolve_sampling_config_for_model(
                &self.models_manager.current_model_id(),
                origin_client.clone(),
            );
        let mut persistence_timer = crate::instrumentation_timer!("session.load_light");
        persistence_timer.with_field("session_id", session_id.0.as_ref());
        let claim_writer = !session_exists;
        let (observed_info, observed_persistence) = crate::session::persistence::load_light(
                &session_info,
                Some(self.gateway.clone()),
                claim_writer,
            )
            .await
            .map_err(|e| crate::session::persistence::io_error_to_acp(&e))?;
        // A resident actor can terminate while its observational replay is in
        // flight. Never let that stale observer snapshot fall through into a
        // writer spawn: reacquire the writer epoch and replay from scratch
        // before deriving any runtime state.
        let (persistence_info, persistence, spawn_new_actor) =
            if !claim_writer
                && self
                    .sessions
                    .borrow()
                    .get(&session_id)
                    .is_none_or(|handle| handle.cmd_tx.is_closed())
            {
                drop(observed_persistence);
                let (owned_info, owned_persistence) = crate::session::persistence::load_light(
                        &session_info,
                        Some(self.gateway.clone()),
                        true,
                    )
                    .await
                    .map_err(|e| crate::session::persistence::io_error_to_acp(&e))?;
                (owned_info, owned_persistence, true)
            } else {
                (observed_info, observed_persistence, claim_writer)
            };
        drop(persistence_timer);
        let crate::session::persistence::PersistedInfoLight {
            mut summary,
            timeline_events,
            mut control_snapshot,
            session_directory,
            rewind_points_source,
            signals: persisted_signals,
            announcement_state: persisted_announcement_state,
            workflow_runs: persisted_workflow_runs,
        } = persistence_info;
        let persisted_control_revision = control_snapshot
            .as_ref()
            .map_or(0, |control| control.control_revision);
        let persisted_behavior = control_snapshot
            .as_ref()
            .map(|control| control.behavior.clone());
        let persisted_agent_name = control_snapshot
            .as_ref()
            .map(|control| control.agent_name.clone())
            .or_else(|| summary.agent_name.clone());
        if summary.agent_name != persisted_agent_name {
            tracing::warn!(
                session_id = %session_id.0,
                summary_agent = ?summary.agent_name,
                control_agent = ?persisted_agent_name,
                "session Agent summary projection diverged from Timeline Control; using Control"
            );
            summary.agent_name.clone_from(&persisted_agent_name);
        }
        let _persisted_goal_mode = control_snapshot.and_then(|control| control.goal);
        let configured_models = self.models_manager.models();
        let available_models = self.models_manager.available();
        let persisted_model_id = summary.current_model_id.clone();
        summary.current_model_id = selectable_catalog_key_for_persisted(
            &configured_models,
            &available_models,
            &summary.current_model_id,
        )
        .ok_or_else(|| {
            let provider_list = configured_models
                .keys()
                .filter_map(|key| key.split_once('/').map(|(provider, _)| provider))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ");
            let provider_clause = if provider_list.is_empty() {
                String::new()
            } else {
                format!(" Configured providers: {provider_list}.")
            };
            acp::Error::invalid_params().data(format!(
                "Session model `{}` is no longer configured.{provider_clause} Add it under [provider.<id>.models] before reopening this session.",
                persisted_model_id.0
            ))
        })?;
        let restored_compaction_count = persisted_signals
            .as_ref()
            .map(|s| s.compaction_count as u64)
            .unwrap_or(0);
        let restored_turn_count = persisted_signals
            .as_ref()
            .map(|s| s.turn_count as u64)
            .unwrap_or(0);
        let restored_tool_call_count = persisted_signals
            .as_ref()
            .map(|s| s.tool_call_count as u64)
            .unwrap_or(0);
        let (restored_behavior, restored_plan_phase) = match persisted_behavior
            .as_ref()
            .map(|snapshot| &snapshot.state)
        {
            Some(crate::session::behavior::BehaviorState::Clarify) => {
                (tool_types::BehaviorId::Clarify, None)
            }
            Some(crate::session::behavior::BehaviorState::Plan(phase)) => {
                let phase = match phase {
                    crate::session::behavior::PlanPhase::Drafting => {
                        ::diagnostics::events::PlanPhase::Drafting
                    }
                    crate::session::behavior::PlanPhase::AwaitingApproval => {
                        ::diagnostics::events::PlanPhase::AwaitingApproval
                    }
                    crate::session::behavior::PlanPhase::Executing => {
                        ::diagnostics::events::PlanPhase::Executing
                    }
                    crate::session::behavior::PlanPhase::Amending => {
                        ::diagnostics::events::PlanPhase::Amending
                    }
                };
                (tool_types::BehaviorId::Plan, Some(phase))
            }
            Some(crate::session::behavior::BehaviorState::Workflow) => {
                (tool_types::BehaviorId::Workflow, None)
            }
            Some(crate::session::behavior::BehaviorState::Goal) => {
                (tool_types::BehaviorId::Goal, None)
            }
            Some(crate::session::behavior::BehaviorState::Normal) | None => {
                (tool_types::BehaviorId::Normal, None)
            }
        };
        let restored_approval_pending = persisted_behavior
            .as_ref()
            .is_some_and(|s| s.approval_pending);
        self.set_turn_number(&session_id, 0);
        let no_replay = parse_no_replay(request_meta.as_ref());
        let cursor = request_meta
            .as_ref()
            .and_then(|m| m.get("cursor"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let session_permission_mode = resolve_session_permission_mode(
            request_meta.as_ref(),
            self.default_permission_mode,
        )?;
        let restore_code_requested = request_meta
            .as_ref()
            .and_then(|m| m.get("grow/restore_code"))
            .and_then(|v| v.as_bool())
            .unwrap_or(self.restore_code);
        let restore_checkout_allowed = workspace::session::git::restore_code_checkout_allowed(
            cwd.as_path(),
            Some(summary.info.cwd.as_str()),
        );
        if restore_code_requested && !restore_checkout_allowed
            && let Some(ref target_sha) = summary.head_commit
        {
            tracing::warn!(
                target: workspace::session::git::RESTORE_CODE_LOG,
                session_id = %session_id.0,
                supplied_cwd = %cwd.as_str(),
                persisted_cwd = %summary.info.cwd,
                target_sha = %target_sha,
                "restore_code: skipping session HEAD checkout — supplied cwd is neither a grow worktree nor the session's persisted cwd (refusing to detach the source repo)"
            );
            ::diagnostics::unified_log::warn(
                "restore_code: skipped session HEAD checkout (unsafe cwd)",
                Some(session_id.0.as_ref()),
                Some(
                    serde_json::json!({
                    "supplied_cwd": cwd.as_str(),
                    "persisted_cwd": summary.info.cwd,
                    "target_sha": target_sha,
                }),
                ),
            );
        }
        let mut code_restore_info: Option<serde_json::Value> = None;
        if restore_code_requested && restore_checkout_allowed
            && let Some(ref target_sha) = summary.head_commit
        {
            use workspace::session::git::RestoreKind;
            let outcome = workspace::session::git::checkout_session_commit(
                    cwd.as_path(),
                    target_sha,
                    true,
                    session_id.0.as_ref(),
                )
                .await;
            let kind = if outcome.checked_out {
                RestoreKind::RegistryOff
            } else {
                RestoreKind::CheckoutFailed
            };
            code_restore_info = crate::agent::restore_code::build_code_restore_meta(
                target_sha,
                &outcome,
                kind,
            );
        }
        let load_envrc = {
            let skip_envrc = request_meta
                .as_ref()
                .and_then(|m| m.get("grow/skip_envrc"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if skip_envrc {
                false
            } else {
                self.cfg.borrow().session.load_envrc.unwrap_or(true)
            }
        };
        let (delta_completions, subagent_projections) = if no_replay {
            tracing::info!(
                session_id = %session_id.0,
                "Skipping session replay (noReplay requested by client)"
            );
            (Vec::new(), Default::default())
        } else {
            let (replay_end_offset, subagent_projections) = self
                .replay_session_updates(
                    &session_id,
                    &cwd,
                    &session_directory,
                    persist_data.as_ref(),
                    target_client_id.as_ref(),
                    cursor.as_deref(),
                )
                .await?;
            let cursor_mark_replay = cursor.is_none();
            let _timer = crate::instrumentation_timer!("session.delta_flush_replay");
            let completions = match self.flush_session(&session_id).await {
                Ok(()) => {
                    self.replay_session_updates_from_offset_enqueue(
                        &session_id,
                        &session_directory,
                        replay_end_offset,
                        persist_data.as_ref(),
                        target_client_id.as_ref(),
                        cursor_mark_replay,
                    )
                }
                Err(reason) => {
                    tracing::warn!(
                        session_id = %session_id.0,
                        reason,
                        "Post-replay flush failed, skipping delta replay"
                    );
                    Vec::new()
                }
            };
            (completions, subagent_projections)
        };
        if let Some(handle) = self.sessions.borrow().get(&session_id) {
            handle.gateway_enabled.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        for rx in delta_completions {
            let _ = rx.await;
        }
        let reconcile_completions = {
            let _timer = crate::instrumentation_timer!("session.reconcile_stale_tasks");
            self.repair_stale_background_task_projections(&session_id, &session_directory)
        };
        for rx in reconcile_completions {
            let _ = rx.await;
        }
        let preloaded_envrc = workspace::envrc::load_envrc_or_empty_when_trusted(
            cwd.as_path(),
            load_envrc && folder_trust::project_scope_allowed(cwd.as_path()),
        );
        let client_code_nav_enabled = request_meta
            .as_ref()
            .and_then(|m| m.get("codeNavEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| self.code_nav_enabled.get());
        let (client_terminal, client_fs_read, client_fs_write) = Self::resolve_client_io_caps(
            request_meta.as_ref(),
            init,
        );
        let prompt_display_cwd = request_meta
            .as_ref()
            .and_then(|m| m.get("grow/display_cwd"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| summary.prompt_display_cwd.clone());
        if spawn_new_actor {
            tracing::info!(
                session_id = %session_id.0,
                "load_session: spawning new session actor (session not in memory)"
            );
            let mut spawn_timer = crate::instrumentation_timer!("session.spawn_and_register_session");
            spawn_timer.with_field("session_id", session_id.0.as_ref());
            let session_title_route = if summary.display_title().is_empty() {
                let (client, model) = self.build_session_title_client(&load_session_sampling)?;
                Some(crate::session::actor::summary::SessionTitleRoute::new(
                    client, model,
                ))
            } else {
                None
            };
            self.spawn_and_register_session(
                    init,
                    SessionSpawnOptions {
                        session_info,
                        cwd: cwd.clone(),
                        mcp_servers,
                        initial_client_mcp_servers,
                        mcp_meta_config_map,
                        persistence,
                        session_title_route,
                        timeline_bootstrap: crate::session::TimelineBootstrap::Existing(
                            timeline_events,
                        ),
                        rewind_points_source,
                        origin_client: origin_client.clone(),
                        client_code_nav_enabled,
                        client_terminal,
                        client_fs_read,
                        client_fs_write,
                        preloaded_envrc: Some(preloaded_envrc),
                        persisted_signals,
                        persisted_behavior,
                        persisted_goal_mode: _persisted_goal_mode,
                        persisted_control_revision,
                        persisted_workflow_runs,
                        persisted_announcement_state,
                        session_meta: request_meta.as_ref(),
                        persisted_agent_name: persisted_agent_name.as_deref(),
                        session_model_id: summary.current_model_id.clone(),
                        session_permission_mode,
                        prompt_display_cwd,
                    },
                )
                .await?;
            drop(spawn_timer);
        } else if self
            .sessions
            .borrow()
            .get(&session_id)
            .is_none_or(|handle| handle.cmd_tx.is_closed())
        {
            return Err(acp::Error::internal_error().data(
                "Resident session ended during reconnect; retry the session load.",
            ));
        } else if !mcp_servers.is_empty() {
            tracing::info!(
                session_id = %session_id.0,
                mcp_server_count = mcp_servers.len(),
                "load_session: reconnecting to existing session, updating MCP servers"
            );
            if let Some(handle) = self.sessions.borrow_mut().get_mut(&session_id) {
                handle.initial_client_mcp_servers = initial_client_mcp_servers;
                let (tx, _rx) = tokio::sync::oneshot::channel();
                let _ = handle
                    .cmd_tx
                    .send(crate::session::SessionCommand::UpdateMcpServers {
                        mcp_servers,
                        respond_to: tx,
                    });
            }
        } else {
            tracing::info!(
                session_id = %session_id.0,
                "load_session: reconnecting to existing session (feedback manager already initialized)"
            );
        }
        if session_exists
            && let Some(hooks) = crate::extensions::hooks::reconnect_client_hooks(
                request_meta.as_ref(),
            ) && let Some(handle) = self.sessions.borrow().get(&session_id)
        {
            handle.set_client_hooks(hooks);
        }
        if let Some(handle) = self.sessions.borrow_mut().get_mut(&session_id) {
            handle.code_nav_enabled = client_code_nav_enabled;
            handle.permission_mode = session_permission_mode;
            let _ = handle.cmd_tx.send(SessionCommand::SetPermissionMode {
                mode: session_permission_mode,
            });
        }
        if restored_approval_pending {
            let command_tx = self
                .sessions
                .borrow()
                .get(&session_id)
                .map(|handle| handle.cmd_tx.clone())
                .ok_or_else(|| {
                    acp::Error::internal_error()
                        .data("Session ended before Plan approval could be reconciled.")
                })?;
            let (respond_to, response) = tokio::sync::oneshot::channel();
            command_tx
                .send(SessionCommand::RestorePlanApproval { respond_to })
                .map_err(|_| {
                    acp::Error::internal_error()
                        .data("Session ended before Plan approval could be reconciled.")
                })?;
            response
                .await
                .map_err(|_| {
                    acp::Error::internal_error()
                        .data("Plan approval reconciliation acknowledgement was lost.")
                })?
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("Plan approval reconciliation failed: {error}"))
                })?;
        }
        let orphan_parent = {
            let sessions = self.sessions.borrow();
            sessions
                .get(&session_id)
                .map(|handle| {
                    (
                        handle.cmd_tx.clone(),
                        handle.info.cwd.clone(),
                        handle.chat_state_handle.clone(),
                        handle.workflow_tracker.clone(),
                    )
                })
        };
        if let Some((parent_cmd_tx, _session_cwd, parent_chat_state, workflow_tracker)) =
            orphan_parent
        {
            crate::agent::subagent::reconcile_orphaned_subagents_with_backend(
                    &subagent_projections,
                    !no_replay,
                    &tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
                        self.subagent_event_tx.clone(),
                        session_id.0.clone(),
                    ),
                    session_id.0.as_ref(),
                    &parent_chat_state,
                    Some(&workflow_tracker),
                    &self.gateway,
                    Some(&parent_cmd_tx),
                )
                .await;
        }
        let model_id = summary.current_model_id.clone();
        self.model_unavailable_sessions.borrow_mut().remove(session_id.0.as_ref());
        tracing::debug!(
            session_id = %session_id.0,
            final_model_id = %model_id.0,
            "load_session: resolved final model_id for set_session_model"
        );
        let enqueued_restore = {
            let _timer = crate::instrumentation_timer!("session.restore_model");
            let restore_meta = summary
                .reasoning_effort
                .map(|effort| {
                    let mut map = acp::Meta::new();
                    map.insert(
                        REASONING_EFFORT_META_KEY.to_string(),
                        reasoning_effort_meta_value(effort),
                    );
                    map
                });
            self.control_session_handle(&session_id)
                .ok_or_else(|| acp::Error::internal_error().data("loaded session actor missing"))
                .and_then(|handle| {
                    crate::agent::handlers::model_switch::enqueue(
                        self,
                        &catalog_transaction,
                        handle,
                        acp::SetSessionModelRequest::new(session_id.to_owned(), model_id)
                            .meta(restore_meta),
                    )
                })
        };
        drop(catalog_transaction);
        if let Ok(enqueued) = enqueued_restore {
            let _ = crate::agent::handlers::model_switch::finish(self, enqueued).await;
        }
        let mut response_meta_map = serde_json::Map::new();
        response_meta_map.insert("sessionId".to_string(), serde_json::json!(session_id));
        if let Some(persist) = persist_data {
            response_meta_map.insert("grow/persist".to_string(), persist);
        }
        let session_cwd = self
            .sessions
            .borrow()
            .get(&session_id)
            .map(|h| h.info.cwd.clone());
        let indexed_roots = session_cwd
            .as_deref()
            .map(|c| self.indexed_roots_for(std::path::Path::new(c)))
            .unwrap_or_default();
        response_meta_map
            .insert("codebaseIndexed".to_string(), serde_json::json!(indexed_roots));
        if summary.head_commit.is_some() && let Some(ref cwd) = session_cwd
            && summary
                .git_root_dir
                .as_deref()
                .is_none_or(|root| {
                    workspace::session::git::find_git_root_from_path(
                            std::path::Path::new(cwd.as_str()),
                        )
                        .ok()
                        .is_some_and(|current_root| {
                            current_root == std::path::Path::new(root)
                        })
                })
        {
            let _timer = crate::instrumentation_timer!("session.git_divergence");
            let cwd_path = std::path::Path::new(cwd.as_str());
            let current_head = workspace::session::git::git_cli(
                    cwd_path,
                    &["rev-parse", "HEAD"],
                )
                .await
                .ok();
            if let Some(divergence) = workspace::session::git::detect_head_divergence(
                summary.head_commit.as_deref(),
                summary.head_branch.as_deref(),
                current_head.as_deref(),
            ) {
                response_meta_map
                    .insert("gitDivergence".to_string(), serde_json::json!(divergence));
            }
        }
        if let Some(info) = code_restore_info {
            response_meta_map.insert("codeRestore".to_string(), info);
        }
        let foreground_tx = self
            .sessions
            .borrow()
            .get(&session_id)
            .map(|handle| handle.cmd_tx.clone());
        if let Some(foreground_tx) = foreground_tx {
            let (respond_to, response) = tokio::sync::oneshot::channel();
            if foreground_tx
                .send(SessionCommand::QueryForeground { respond_to })
                .is_ok()
                && let Ok(Some(foreground)) = response.await
            {
                response_meta_map.insert(
                    "grow/foreground".to_string(),
                    serde_json::to_value(foreground).expect("foreground snapshot serializes"),
                );
            }
        }
        let model_state = self.model_state(Some(&session_id));
        self.insert_session_config_meta(
            &mut response_meta_map,
            &session_id,
            session_cwd.clone().unwrap_or_default(),
            summary.display_title_opt(),
            &model_state,
        );
        if let Some(agent_name) = summary.agent_name.as_deref() {
            response_meta_map.insert(
                "grow/agentName".to_owned(),
                serde_json::Value::String(agent_name.to_owned()),
            );
        }
        // The resident actor may terminate during any of the asynchronous
        // restore work above. Never acknowledge a reconnect that has no live
        // command endpoint at the response boundary.
        if self
            .sessions
            .borrow()
            .get(&session_id)
            .is_none_or(|handle| handle.cmd_tx.is_closed())
        {
            return Err(acp::Error::internal_error()
                .data("Session ended while loading; retry the session load."));
        }
        let response_meta = serde_json::Value::Object(response_meta_map);
        ::diagnostics::unified_log::info(
            "session loaded",
            Some(session_id.0.as_ref()),
            None,
        );
        let response = acp::LoadSessionResponse::new()
            .models(Some(model_state))
            .meta(response_meta.as_object().cloned());
        if let Some(handle) = self.sessions.borrow().get(&session_id) {
            let _ = handle.cmd_tx.send(SessionCommand::AdvertiseCommands);
        }
        {
            log_event(::diagnostics::events::SessionLoad {
                session_id: session_id.0.to_string(),
                compaction_count: restored_compaction_count,
                turn_count: restored_turn_count,
                tool_call_count: restored_tool_call_count,
                behavior: restored_behavior,
                plan_phase: restored_plan_phase,
                permission_mode: session_permission_mode,
                model_id: summary.current_model_id.0.to_string(),
                restored_from_disk: true,
            });
        }
        Ok(response)
    }
    #[tracing::instrument(
        name = "agent.prompt",
        skip_all,
        fields(session_id = %arguments.session_id.0, turn_number = tracing::field::Empty)
    )]
    #[allow(unused_mut)]
    async fn prompt(
        &self,
        mut arguments: acp::PromptRequest,
    ) -> Result<acp::PromptResponse, acp::Error> {
        tracing::debug!(
            target: "sampling_log",
            session_id = %arguments.session_id.0,
            "Received prompt request"
        );
        ::diagnostics::unified_log::info(
            "prompt received",
            Some(arguments.session_id.0.as_ref()),
            None,
        );
        let handle = self
            .session_handle_waiting_for_load(&arguments.session_id)
            .await
            .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
        let prompt_id = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("promptId"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let blocked_model_id = handle.model_route.snapshot().model_id;
        let blocked_prompt_response = || {
            acp::PromptResponse::new(acp::StopReason::EndTurn).meta(
                build_prompt_response_meta(PromptResponseMetaArgs {
                    session_id: &arguments.session_id.to_string(),
                    prompt_id: &prompt_id,
                    total_tokens: 0,
                    model_id: blocked_model_id.0.as_ref(),
                    last_turn_usage: None,
                    prompt_usage: None,
                    cancellation_category: None,
                    cancel_trigger: None,
                    structured_output: None,
                })
                .as_object()
                .cloned(),
            )
        };
        let catalog_transaction = self.model_reload_lock.lock().await;
        if self.models_manager.allowlist_excludes_all() {
            self.send_model_auto_switched(
                    &arguments.session_id,
                    &acp::ModelId::new(String::new()),
                    &acp::ModelId::new(String::new()),
                    "None of your models are allowed by allowed_models. \
                 Broaden it or remove it from your config, then restart.",
                )
                .await;
            return Ok(blocked_prompt_response());
        }
        let latched_model = self
            .model_unavailable_sessions
            .borrow()
            .get(arguments.session_id.0.as_ref())
            .cloned();
        let mut enqueued_recovery = None;
        if let Some(unavailable_model) = latched_model {
            let models = self.models_manager.models();
            let available = self.models_manager.available();
            let restore_model_id = selectable_catalog_key_for_persisted(
                    &models,
                    &available,
                    &unavailable_model,
                )
                .unwrap_or(unavailable_model.clone());
            if available.contains_key(&restore_model_id) {
                tracing::info!(
                    session_id = %arguments.session_id.0,
                    model_id = %restore_model_id.0,
                    "prompt: previously-unavailable model is back in the catalog; restoring it and unblocking the session"
                );
                ::diagnostics::unified_log::info(
                    "prompt: previously-unavailable model recovered, unblocking session",
                    Some(arguments.session_id.0.as_ref()),
                    Some(
                        serde_json::json!({
                        "model_id": restore_model_id.0.as_ref(),
                    }),
                    ),
                );
                enqueued_recovery = Some((
                    restore_model_id.clone(),
                    crate::agent::handlers::model_switch::enqueue(
                        self,
                        &catalog_transaction,
                        handle.clone(),
                        acp::SetSessionModelRequest::new(
                            arguments.session_id.clone(),
                            restore_model_id.clone(),
                        ),
                    ),
                ));
            } else {
                tracing::warn!(
                    session_id = %arguments.session_id.0,
                    unavailable_model = %unavailable_model.0,
                    available_count = available.len(),
                    available_keys = ?available.keys().take(10).collect::<Vec<_>>(),
                    "prompt blocked: session model unavailable since load and still missing from the catalog"
                );
                ::diagnostics::unified_log::warn(
                    "prompt blocked: model unavailable",
                    Some(arguments.session_id.0.as_ref()),
                    Some(
                        serde_json::json!({
                        "unavailable_model": unavailable_model.0.as_ref(),
                        "available_count": available.len(),
                    }),
                    ),
                );
                self.send_model_auto_switched(
                        &arguments.session_id,
                        &acp::ModelId::new(String::new()),
                        &acp::ModelId::new(String::new()),
                        "Your previous model is no longer available and could not \
                     be switched to a compatible model. Please start a new session.",
                    )
                    .await;
                return Ok(blocked_prompt_response());
            }
        }
        drop(catalog_transaction);
        if let Some((restore_model_id, enqueued)) = enqueued_recovery {
            let result = match enqueued {
                Ok(enqueued) => crate::agent::handlers::model_switch::finish(self, enqueued).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(_) => {
                    self.model_unavailable_sessions
                        .borrow_mut()
                        .remove(arguments.session_id.0.as_ref());
                }
                Err(error) => {
                    tracing::warn!(
                        session_id = %arguments.session_id.0,
                        model_id = %restore_model_id.0,
                        error = ?error,
                        "prompt: failed to restore previously-unavailable model; continuing with the session's current model"
                    );
                }
            }
        }
        let dispatch_lock = self.dispatch_lock(&arguments.session_id);
        let dispatch_guard = dispatch_lock.lock().await;
        let turn_number = self.allocate_turn_number(&arguments.session_id);
        tracing::Span::current().record("turn_number", turn_number);
        let (model_tx, model_rx) = oneshot::channel();
        let _ = handle
            .cmd_tx
            .send(crate::session::SessionCommand::GetCurrentModel {
                responds_to: model_tx,
            });
        let model = model_rx
            .await
            .unwrap_or_else(|_| self.models_manager.current_model_id().0.to_string());
        let verbatim = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("verbatim"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let (tx, rx) = oneshot::channel();
        let prompt_client_identifier = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("clientIdentifier"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let prompt_screen_mode = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("screenMode"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let json_schema = arguments
            .meta
            .as_ref()
            .and_then(|m| m.get("outputSchema"))
            .cloned();
        if json_schema.as_ref().is_some_and(|schema| !schema.is_object()) {
            return Err(
                acp::Error::invalid_params()
                    .data("outputSchema must be a JSON object describing a JSON Schema"),
            );
        }
        handle
            .cmd_tx
            .send(SessionCommand::QueuePrompt {
                prompt_id: prompt_id.clone(),
                prompt_blocks: arguments.prompt.clone(),
                origin: crate::session::PromptOrigin::User,
                turn_kind: crate::session::TurnKind::User,
                client_identifier: prompt_client_identifier,
                screen_mode: prompt_screen_mode,
                verbatim,
                json_schema,
                respond_to: tx,
                persist_ack: None,
            })
            .map_err(|e| {
                acp::Error::internal_error()
                    .data(format!("failed to dispatch prompt to session: {e}"))
            })?;
        drop(dispatch_guard);
        self.push_roster_activity_delta(
            &arguments.session_id,
            crate::agent::roster::RosterActivity::Working,
        );
        let stop_result = rx
            .await
            .map_err(|_| {
                acp::Error::internal_error().data("session failed to respond")
            })?;
        let last_turn_usage_for_meta = handle
            .chat_state_handle
            .get_last_turn_usage()
            .await;
        if matches!(
            stop_result,
            Ok(crate::session::commands::PromptTurnOk {
                completion_kind: crate::session::commands::PromptCompletionKind::RemovedFromQueue,
                ..
            })
        ) {
            return Ok(
                acp::PromptResponse::new(acp::StopReason::Cancelled)
                    .meta(
                        build_prompt_response_meta(PromptResponseMetaArgs {
                                session_id: &arguments.session_id.to_string(),
                                prompt_id: &prompt_id,
                                total_tokens: 0,
                                model_id: &model,
                                last_turn_usage: None,
                                prompt_usage: None,
                                cancellation_category: None,
                                cancel_trigger: None,
                                structured_output: None,
                            })
                            .as_object()
                            .cloned(),
                    ),
            );
        }
        let cancel_trigger: Option<String> = stop_result
            .as_ref()
            .ok()
            .and_then(|ok| match &ok.completion_kind {
                crate::session::commands::PromptCompletionKind::Cancelled {
                    context: Some(ctx),
                    ..
                } => ctx.trigger.clone(),
                _ => None,
            });
        {
            let end_activity = if handle
                .pending_interactions
                .lock()
                .map(|g| !g.is_empty())
                .unwrap_or(false)
            {
                crate::agent::roster::RosterActivity::NeedsInput
            } else {
                crate::agent::roster::RosterActivity::Idle
            };
            self.push_roster_activity_delta(&arguments.session_id, end_activity);
        }
        match stop_result {
            Ok(turn_ok) => {
                let crate::session::commands::PromptTurnOk {
                    stop_reason,
                    total_tokens,
                    turn_snapshot: _,
                    completion_kind,
                    structured_output,
                    usage: prompt_usage,
                } = turn_ok;
                let cwd = handle.info.cwd.clone();
                let cmd_tx = handle.cmd_tx.clone();
                tokio::spawn(async move {
                    let head = workspace::session::git::get_current_commit(
                            std::path::Path::new(&cwd),
                        )
                        .await;
                    let branch = workspace::session::git::get_branch(
                            std::path::Path::new(&cwd),
                        )
                        .await;
                    let _ = cmd_tx.send(crate::session::SessionCommand::PersistGitHead {
                        commit: head,
                        branch,
                    });
                });
                let last_turn_usage = last_turn_usage_for_meta;
                let cancellation_category = match &completion_kind {
                    crate::session::commands::PromptCompletionKind::Cancelled {
                        category: Some(cat),
                        ..
                    } => Some(format!("{cat:?}")),
                    crate::session::commands::PromptCompletionKind::MaxTurnsReached {
                        ..
                    } => Some("max_turns_reached".to_string()),
                    crate::session::commands::PromptCompletionKind::StationarityEnded => {
                        Some("action_stationarity".to_string())
                    }
                    _ => None,
                };
                Ok(
                    acp::PromptResponse::new(stop_reason)
                        .meta(
                            build_prompt_response_meta(PromptResponseMetaArgs {
                                    session_id: &arguments.session_id.to_string(),
                                    prompt_id: &prompt_id,
                                    total_tokens,
                                    model_id: &model,
                                    last_turn_usage: last_turn_usage.as_ref(),
                                    prompt_usage,
                                    cancellation_category,
                                    cancel_trigger,
                                    structured_output,
                                })
                                .as_object()
                                .cloned(),
                        ),
                )
            }
            Err(err) => {
                let err = if crate::sampling::error::prompt_usage_from_error(&err)
                    .is_some()
                {
                    err
                } else {
                    let prompt_id = handle
                        .current_prompt_id
                        .lock()
                        .ok()
                        .and_then(|g| g.clone());
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let usage = if handle
                        .cmd_tx
                        .send(crate::session::commands::SessionCommand::ErrorPathUsageFallback {
                            prompt_id,
                            respond_to: tx,
                        })
                        .is_ok()
                    {
                        rx.await.ok().flatten()
                    } else {
                        None
                    };
                    crate::sampling::error::attach_prompt_usage(err, usage)
                };
                Err(err)
            }
        }
    }
    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        tracing::info!("Received cancel request {args:?}");
        let handle = self.session_handle_waiting_for_load(&args.session_id).await;
        let cancel_trigger = args
            .meta
            .as_ref()
            .and_then(|m| m.get("cancelTrigger"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        ::diagnostics::unified_log::info(
            "shell.cancel.received",
            Some(args.session_id.0.as_ref()),
            Some(
                serde_json::json!({
                "session_found": handle.is_some(),
                "trigger": cancel_trigger,
            }),
            ),
        );
        if let Some(handle) = handle {
            let cancel_subagents = args
                .meta
                .as_ref()
                .and_then(|m| m.get("cancelSubagents"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let rewind_if_pristine = args
                .meta
                .as_ref()
                .and_then(|m| m.get("rewindIfPristine"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // Explicit user "Pause goal" intent from the Goal interrupt panel;
            // absent for older clients and all programmatic cancels.
            let pause_goal = args
                .meta
                .as_ref()
                .and_then(|m| m.get("pauseGoal"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let dispatch_lock = self.dispatch_lock(&args.session_id);
            let _dispatch_guard = dispatch_lock.lock().await;
            let _ = handle
                .cmd_tx
                .send(SessionCommand::Cancel {
                    cancel_subagents,
                    kill_background_tasks: false,
                    rewind_if_pristine,
                    pause_goal,
                    trigger: cancel_trigger,
                });
        }
        Ok(())
    }
    async fn set_session_mode(
        &self,
        args: acp::SetSessionModeRequest,
    ) -> Result<acp::SetSessionModeResponse, acp::Error> {
        tracing::info!("Received set session mode request {args:?}");
        let handle = self
            .session_handle_waiting_for_load(&args.session_id)
            .await
            .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
        let (tx, rx) = oneshot::channel();
        handle
            .cmd_tx
            .send(SessionCommand::BehaviorChange {
                session_mode: args.mode_id,
                responds_to: tx,
            })
            .map_err(|_| acp::Error::internal_error().data("session actor closed"))?;
        let outcome = rx
            .await
            .map_err(|_| {
                acp::Error::internal_error().data("response to set session failed")
            })??;
        Ok(acp::SetSessionModeResponse::new().meta(outcome.response_meta()))
    }
    async fn set_session_model(
        &self,
        args: acp::SetSessionModelRequest,
    ) -> Result<acp::SetSessionModelResponse, acp::Error> {
        // A reconnect load registers itself before taking the catalog lock.
        // Resolve that race first so this request never holds the lock while
        // waiting for the load that must acquire it.
        let handle = self
            .control_session_handle_waiting_for_load(&args.session_id)
            .await
            .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
        let catalog_transaction = self.model_reload_lock.lock().await;
        let workflow_pinned = handle.workflow_run_id.is_some();
        if !workflow_pinned {
            let model = self.resolve_model_id(&args.model_id)?;
            if !model.info.user_selectable {
                return Err(
                    acp::Error::invalid_params()
                        .data("This model isn't allowed by your allowed_models setting."),
                );
            }
        }
        if args
            .meta
            .as_ref()
            .is_some_and(|meta| meta.contains_key(REASONING_EFFORT_META_KEY))
        {
            let Some(effort) = sampling_types::parse_reasoning_effort_meta(args.meta.as_ref()) else {
                return Err(acp::Error::invalid_params()
                    .data("reasoningEffort must be a canonical string offered by the selected model"));
            };
            if !workflow_pinned
                && !self
                .models_manager
                .model_offers_reasoning_effort(args.model_id.0.as_ref(), effort)
            {
                return Err(acp::Error::invalid_params().data(format!(
                    "reasoning effort `{effort}` is not offered by model `{}`",
                    args.model_id.0
                )));
            }
        }
        let session_id = args.session_id.clone();
        let enqueued = crate::agent::handlers::model_switch::enqueue(
            self,
            &catalog_transaction,
            handle,
            args,
        )?;
        drop(catalog_transaction);
        let res = crate::agent::handlers::model_switch::finish(self, enqueued).await;
        if res.is_ok()
            && let Some(unavailable) = self
                .model_unavailable_sessions
                .borrow_mut()
                .remove(session_id.0.as_ref())
        {
            tracing::info!(
                session_id = %session_id.0,
                previously_unavailable_model = %unavailable.0,
                "set_session_model: user model switch cleared the model-unavailable block"
            );
        }
        res
    }
    #[tracing::instrument(
        name = "agent.ext_method",
        skip_all,
        fields(method = %args.method)
    )]
    async fn ext_method(
        &self,
        args: acp::ExtRequest,
    ) -> Result<acp::ExtResponse, acp::Error> {
        let request_meta = serde_json::from_str::<serde_json::Value>(args.params.get())
            .ok()
            .and_then(|v| v.get("_meta").cloned());
        tracing::info!("Received extension method call: method={}", args.method);
        #[allow(unused_mut)]
        let mut backend_no_bridge_err: Option<acp::Error> = None;
        let method = args.method.clone();
        let result = match method.as_ref() {
            "grow/session/info" | "grow/session/set_agent" | "grow/session/close" | "grow/session/list"
            | "grow/sessions/list" => {
                crate::agent::handlers::session::handle(self, &args).await
            }
            "grow/session/updates" => {
                crate::extensions::session_updates::handle(&args, &self.gateway).await
            }
            "grow/session/state" => {
                crate::extensions::session_state::handle_state(&args).await
            }
            "grow/session/import" => {
                crate::extensions::session_state::handle_import(&args).await
            }
            "grow/session/search" => {
                crate::extensions::session_search::handle(&args).await
            }
            "grow/session/resolve_local_for_worktree_resume" => {
                let ops = self.resolve_workspace_ops()?;
                crate::extensions::worktree::handle(self, &ops, &args).await
            }
            "grow/session/rename" | "grow/session/delete"
            | "grow/session/update_mcp_servers" | "grow/session/fork"
            | "grow/internal/reload_mcp_catalog" | "grow/internal/reload_skills"
            | "grow/internal/reload_workflows" | "grow/internal/reload_models"
            | "grow/internal/reload_announcements"
            | "grow/plugins/reload" | "grow/commands/list" | "grow/commands/execute"
            | "grow/queue/prompt_status" => {
                crate::extensions::session_admin::handle(self, &args).await
            }
            "grow/session/repair" => crate::extensions::repair::handle(self, &args).await,
            "grow/session/usage" => crate::extensions::usage::handle(self, &args).await,
            "grow/memory/flush" | "grow/memory/rewrite" => {
                crate::extensions::memory::handle(self, &args).await
            }
            "grow/skills/refresh-baseline" => {
                self.refresh_skill_baseline_for_all_sessions();
                crate::extensions::to_ext_response(
                    Ok(serde_json::json!({"ok": true})),
                )
            }
            "grow/steer" => crate::extensions::interject::handle(self, &args).await,
            "grow/btw" => {
                crate::extensions::feedback::handle(self, &args).await
            }
            "grow/recap" => crate::extensions::recap::handle(self, &args).await,
            "grow/prompt_history" => {
                crate::extensions::prompt_history::handle(self, &args).await
            }
            "grow/suggest" => crate::extensions::suggest::handle(self, &args).await,
            "grow/suggestPrompt" => crate::extensions::suggest::handle(self, &args).await,
            s if s.starts_with("grow/git/worktree/") => {
                let ops = self.resolve_workspace_ops()?;
                crate::extensions::worktree::handle(self, &ops, &args).await
            }
            s if s.starts_with("grow/git/") => {
                let ops = self.resolve_workspace_ops()?;
                crate::extensions::git::handle(self, &ops, &args).await
            }
            s if s.starts_with("grow/compact_conversation") => {
                crate::extensions::memory::handle(self, &args).await
            }
            s if s.starts_with("grow/plugins/") => {
                crate::extensions::plugins::handle(self, &args).await
            }
            s if s.starts_with("grow/marketplace/") => {
                crate::extensions::marketplace::handle(self, &args).await
            }
            s if s.starts_with("grow/hooks/") => {
                crate::extensions::hooks::handle(self, &args).await
            }
            s if s.starts_with("grow/hunk-tracker/") => {
                let ops = self.resolve_workspace_ops()?;
                crate::extensions::hunk_tracker::handle(self, &ops, &args).await
            }
            s if s.starts_with("grow/pr/") => {
                crate::extensions::pr::handle(self, &args).await
            }
            s if s.starts_with(crate::extensions::mcp::mcp_methods::PREFIX) => {
                crate::extensions::mcp::handle(self, &args).await
            }
            s if s.starts_with("grow/task/") => {
                crate::extensions::task::handle(self, &args).await
            }
            s if s.starts_with("grow/scheduler/") => {
                crate::extensions::task::handle_scheduler(self, &args).await
            }
            s if s.starts_with("grow/subagent/") => {
                crate::extensions::task::handle_subagent(self, &args).await
            }
            s if s.starts_with("grow/terminal/") => {
                crate::extensions::terminal::handle(self, &args).await
            }
            s if crate::extensions::fs::is_fs_method(s) => {
                crate::extensions::fs::handle(self, &args).await
            }
            s if s.starts_with("grow/search/") => {
                crate::extensions::search::handle(self, &args).await
            }
            s if s.starts_with("grow/bundle/") => {
                crate::extensions::bundle::handle(self, &args).await
            }
            s if s.starts_with("grow/code/") => {
                let ops = self.resolve_workspace_ops()?;
                crate::extensions::code_nav::handle(self, &ops, &args).await
            }
            s if s.starts_with("grow/skills/") || s == "grow/workflows/list" => {
                crate::extensions::skills::handle(
                    self,
                    &args,
                    self.plugin_registry_handle.snapshot().as_deref(),
                )
                    .await
            }
            s if s.starts_with("grow/review") => {
                crate::extensions::feedback::handle(self, &args).await
            }
            s if s.starts_with("grow/debug/") => {
                crate::extensions::debug::handle(self, &args).await
            }
            s if s.starts_with("grow/rewind") => {
                crate::extensions::rewind::handle(self, &args).await
            }
            other => {
                Err(
                    acp::Error::method_not_found()
                        .data(format!("unknown ACP extension method: {other}")),
                )
            }
        };
        if let Some(err) = backend_no_bridge_err
            && matches!(&result, Err(e) if e.code == acp::Error::method_not_found().code)
        {
            return Err(err);
        }
        result
    }
    async fn ext_notification(
        &self,
        args: acp::ExtNotification,
    ) -> Result<(), acp::Error> {
        tracing::info!("Received extension notification: method={}", args.method);
        if args.method.as_ref() == "grow/permission_mode_changed"
            && let Ok(params) = serde_json::from_str::<
                serde_json::Value,
            >(args.params.get())
        {
            let session_id = params.get("sessionId").and_then(|v| v.as_str());
            let mode = params
                .get("permissionMode")
                .and_then(|v| v.as_str())
                .and_then(|mode| match mode {
                    "ask" => Some(crate::util::config::PermissionMode::Ask),
                    "auto" => Some(crate::util::config::PermissionMode::Auto),
                    "always-approve" => {
                        Some(crate::util::config::PermissionMode::AlwaysApprove)
                    }
                    _ => None,
                });
            if let (Some(session_id), Some(mode)) = (session_id, mode) {
                let session_id = acp::SessionId::new(session_id);
                if let Some(handle) = self.sessions.borrow_mut().get_mut(&session_id) {
                    handle.permission_mode = mode;
                    let _ = handle
                        .cmd_tx
                        .send(crate::session::SessionCommand::SetPermissionMode { mode });
                }
            }
        }
        if args.method.as_ref() == "grow/permissions/reset" {
            let sessions = self.sessions.borrow();
            let updated = sessions
                .values()
                .filter(|h| {
                    h
                        .cmd_tx
                        .send(crate::session::SessionCommand::ResetPermissionState)
                        .is_ok()
                })
                .count();
            tracing::info!(
                target_sessions = updated,
                total_sessions = sessions.len(),
                "Permission state reset for matching sessions"
            );
        }
        if args.method.as_ref() == "grow/internal/evict_sessions" {
            self.handle_evict_sessions(&args.params).await;
        }
        if args.method.as_ref() == "grow/toggle_plan_mode"
            && let Ok(params) = serde_json::from_str::<
                serde_json::Value,
            >(args.params.get())
        {
            let session_id_str = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let handle = self
                .sessions
                .borrow()
                .values()
                .find(|s| s.info.id.0.as_ref() == session_id_str)
                .cloned();
            if let Some(handle) = handle {
                let is_engaged = handle.behavior.lock().is_plan();
                let next_mode_id = acp::SessionModeId::new(if is_engaged {
                    tools::types::BehaviorId::Normal.as_id()
                } else {
                    "plan"
                });
                let (tx, rx) = oneshot::channel();
                let _ = handle
                    .cmd_tx
                    .send(SessionCommand::BehaviorChange {
                        session_mode: next_mode_id.clone(),
                        responds_to: tx,
                    });
                if rx.await.is_err() {
                    tracing::warn!(
                        session_id = %session_id_str,
                        mode_id = %next_mode_id.0,
                        "toggle_plan_mode: session mode update failed"
                    );
                }
            } else {
                tracing::warn!(
                    session_id = %session_id_str,
                    "toggle_plan_mode: session not found"
                );
            }
        }
        if matches!(
            args.method.as_ref(),
            "grow/queue/remove"
                | "grow/queue/reorder"
                | "grow/queue/clear"
                | "grow/queue/edit"
                | "grow/queue/interject"
        )
            && let Ok(params) = serde_json::from_str::<
                serde_json::Value,
            >(args.params.get())
        {
            let session_id_str = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let owner = params
                .get("owner")
                .or_else(|| params.get("clientIdentifier"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let handle = self
                .sessions
                .borrow()
                .values()
                .find(|s| s.info.id.0.as_ref() == session_id_str)
                .cloned();
            if let Some(handle) = handle {
                let cmd = crate::agent::ext_parsers::parse_queue_edit_command(
                    args.method.as_ref(),
                    &params,
                    owner,
                );
                if let Some(cmd) = cmd && handle.cmd_tx.send(cmd).is_err() {
                    tracing::warn!(
                        session_id = %session_id_str,
                        method = %args.method,
                        "queue edit: failed to forward SessionCommand (session actor gone)"
                    );
                }
            } else {
                tracing::warn!(
                    session_id = %session_id_str,
                    method = %args.method,
                    "queue edit: session not found"
                );
            }
        }
        if args.method.as_ref() == "grow/terminal/pty/input"
            && let Ok(params) = serde_json::from_str::<
                serde_json::Value,
            >(args.params.get())
        {
            crate::extensions::terminal::handle_pty_input(&params).await;
        }
        if args.method.as_ref() == "_grow/session/update" {
            if let Ok(notification) = serde_json::from_str::<
                SessionNotification,
            >(args.params.get()) {
                tracing::info!(
                    "Storing Grow session notification: session_id={}",
                    notification.session_id.0
                );
                if let Some(handle) = self
                    .sessions
                    .borrow()
                    .get(&notification.session_id)
                {
                    let _ = handle
                        .cmd_tx
                        .send(crate::session::SessionCommand::GrowSessionNotification {
                            notification,
                        });
                } else {
                    tracing::warn!(
                        "Received Grow session notification for unknown session: {}",
                        notification.session_id.0
                    );
                }
            } else {
                tracing::warn!("Failed to parse Grow session notification params");
            }
        }
        if args.method.as_ref() == "grow/diagnostics/non_git_decision" {
            #[derive(serde::Deserialize)]
            struct NonGitDecisionParams {
                decision: String,
                session_id: String,
                #[serde(default)]
                client_version: Option<String>,
            }
            if let Ok(params) = serde_json::from_str::<
                NonGitDecisionParams,
            >(args.params.get()) {
                tracing::info!(
                    decision = %params.decision,
                    session_id = %params.session_id,
                    client_version = ?params.client_version,
                    "non_git_decision",
                );
                ::diagnostics::session_ctx::log_event(::diagnostics::events::NonGitDecisionEvent {
                    decision: params.decision,
                    session_id: params.session_id,
                    client_version: params.client_version,
                });
            } else {
                tracing::warn!("Failed to parse non_git_decision diagnostics params");
            }
        }
        if args.method.as_ref() == "grow/diagnostics/multi_agent_followup" {
            #[derive(serde::Deserialize)]
            struct MultiAgentFollowupParams {
                preferred_agent_label: char,
                preferred_agent_session_id: Option<String>,
                preferred_agent_model_id: Option<String>,
                /// (label, session_id, model_id)
                other_agents: Vec<(char, Option<String>, Option<String>)>,
            }
            if let Ok(params) = serde_json::from_str::<
                MultiAgentFollowupParams,
            >(args.params.get()) {
                tracing::info!(
                    "Logging multi-agent followup diagnostics: preferred_agent={}",
                    params.preferred_agent_label
                );
                let total_agents = 1 + params.other_agents.len();
                ::diagnostics::session_ctx::log_event(::diagnostics::events::MultiAgentFollowup {
                    preferred_agent_label: params.preferred_agent_label.to_string(),
                    preferred_agent_session_id: params.preferred_agent_session_id,
                    preferred_agent_model_id: params.preferred_agent_model_id,
                    other_agents: params
                        .other_agents
                        .into_iter()
                        .map(|(l, s, m)| ::diagnostics::events::AgentInfo {
                            label: l.to_string(),
                            session_id: s,
                            model_id: m,
                        })
                        .collect(),
                    total_agents,
                });
            } else {
                tracing::warn!("Failed to parse multi-agent followup diagnostics params");
            }
        }
        if args.method.as_ref() == "grow/diagnostics/multi_agent_apply" {
            #[derive(serde::Deserialize)]
            struct MultiAgentApplyParams {
                applied_agent_label: char,
                applied_agent_session_id: Option<String>,
                applied_agent_model_id: Option<String>,
                /// (label, session_id, model_id)
                discarded_agents: Vec<(char, Option<String>, Option<String>)>,
            }
            if let Ok(params) = serde_json::from_str::<
                MultiAgentApplyParams,
            >(args.params.get()) {
                tracing::info!(
                    "Logging multi-agent apply diagnostics: applied_agent={}",
                    params.applied_agent_label
                );
                let total_agents = 1 + params.discarded_agents.len();
                ::diagnostics::session_ctx::log_event(::diagnostics::events::MultiAgentApply {
                    applied_agent_label: params.applied_agent_label.to_string(),
                    applied_agent_session_id: params.applied_agent_session_id,
                    applied_agent_model_id: params.applied_agent_model_id,
                    discarded_agents: params
                        .discarded_agents
                        .into_iter()
                        .map(|(l, s, m)| ::diagnostics::events::AgentInfo {
                            label: l.to_string(),
                            session_id: s,
                            model_id: m,
                        })
                        .collect(),
                    total_agents,
                });
            } else {
                tracing::warn!("Failed to parse multi-agent apply diagnostics params");
            }
        }
        if args.method.as_ref() == "grow/diagnostics/multi_agent_discard" {
            #[derive(serde::Deserialize)]
            struct MultiAgentDiscardParams {
                /// (label, session_id, model_id)
                discarded_agents: Vec<(char, Option<String>, Option<String>)>,
            }
            if let Ok(params) = serde_json::from_str::<
                MultiAgentDiscardParams,
            >(args.params.get()) {
                tracing::info!(
                    "Logging multi-agent discard diagnostics: {} agents discarded",
                    params.discarded_agents.len()
                );
                let total = params.discarded_agents.len();
                ::diagnostics::session_ctx::log_event(::diagnostics::events::MultiAgentDiscard {
                    discarded_agents: params
                        .discarded_agents
                        .into_iter()
                        .map(|(l, s, m)| ::diagnostics::events::AgentInfo {
                            label: l.to_string(),
                            session_id: s,
                            model_id: m,
                        })
                        .collect(),
                    total_agents_discarded: total,
                });
            } else {
                tracing::warn!("Failed to parse multi-agent discard diagnostics params");
            }
        }
        if args.method.as_ref() == ::diagnostics::unified_log::LOG_METHOD
            && let Ok(params) = serde_json::from_str::<
                ::diagnostics::unified_log::LogNotificationParams,
            >(args.params.get())
        {
            ::diagnostics::unified_log::ingest_client_entries(
                params.src,
                &params.entries,
            );
        }
        Ok(())
    }
}
