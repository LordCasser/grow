#![cfg_attr(rustfmt, rustfmt::skip)]
#![allow(unused_imports)]
//! Inherent [`MvpAgent`] helpers (MCP/clients/gateway, settings/models, session ops, spawn).
//! Co-located child of `mvp_agent` (`use super::*`).
use super::*;
use crate::sampling::SamplingClient as OaiCompatClient;
use tools::implementations::grow_build::task::backend::SubagentBackend;
use tty_utils::ProcessScope;
impl MvpAgent {
    pub fn reload_skills_all_sessions(&self) -> usize {
        let session_ids: Vec<agent_client_protocol::SessionId> = self
            .sessions
            .borrow()
            .keys()
            .cloned()
            .collect();
        for sid in &session_ids {
            if let Some(handle) = self.sessions.borrow().get(sid).cloned() {
                let _ = handle.cmd_tx.send(SessionCommand::ReloadSkills);
            }
        }
        session_ids.len()
    }
    pub fn advertise_commands_all_sessions(&self) -> usize {
        let session_ids: Vec<agent_client_protocol::SessionId> = self
            .sessions
            .borrow()
            .keys()
            .cloned()
            .collect();
        for session_id in &session_ids {
            if let Some(handle) = self.sessions.borrow().get(session_id).cloned() {
                let _ = handle.cmd_tx.send(SessionCommand::AdvertiseCommands);
            }
        }
        session_ids.len()
    }
    pub(super) fn resolve_image_description_model(&self) -> Option<String> {
        self.cfg.borrow().image_description_model.clone()
    }
    pub(super) fn build_session_title_client(
        &self,
        primary: &SamplingConfig,
    ) -> Result<(OaiCompatClient, String), acp::Error> {
        let configured_model = self.cfg.borrow().session_title_model.clone();
        let models = self.models_manager.models();
        let alpha_test_key = self.cfg.borrow().endpoints.alpha_test_key.clone();
        let config = match configured_model {
            Some(model_id) => {
                let Some(mut cfg) = crate::agent::config::resolve_aux_model_sampling_config(
                    &model_id,
                    &models,
                    alpha_test_key,
                ) else {
                    tracing::warn!(
                        model_id,
                        "configured session-title model is unavailable; using the active session model"
                    );
                    return OaiCompatClient::new(primary.clone())
                        .map(|client| (client, primary.model.clone()))
                        .map_err(map_sampling_err_to_acp);
                };
                crate::agent::config::stamp_session_local_sampler_fields(
                    &mut cfg,
                    primary,
                    primary.max_retries,
                );
                cfg
            }
            None => primary.clone(),
        };
        let model = config.model.clone();
        let client = OaiCompatClient::new(config).map_err(map_sampling_err_to_acp)?;
        Ok((client, model))
    }
    /// Publish the current ACP auth method into the shared live handle so every
    /// running session's per-turn auth gate observes it on its next turn.
    pub(super) fn set_auth_method(&self, id: acp::AuthMethodId) {
        self.auth_method_id.store(Some(std::sync::Arc::new(id)));
    }
    /// Resolve the launch dir's project-scope trust verdict ONCE and return it
    /// with its path.
    ///
    /// Memoizes the single [`folder_trust::resolve_launch_dir_trust`] gather (see
    /// it for the dedup + TOCTOU contract) so the two one-shot init helpers
    /// (`ensure_plugin_registry` and `ensure_local_workspace_ops`) share it
    /// instead of each re-scanning. They share a single point-in-time verdict
    /// rather than two independent re-scans; the sub-millisecond, startup-only
    /// window between them is intentional (the cross-session TOCTOU re-scan is
    /// preserved per the contract).
    fn prime_launch_dir_trust(&self) -> (&std::path::Path, bool) {
        let trust = *self
            .launch_dir_trust
            .get_or_init(|| {
                let remote_settings = self.cfg.borrow().remote_settings.clone();
                folder_trust::resolve_launch_dir_trust(
                    &self.launch_cwd,
                    remote_settings.as_ref(),
                )
            });
        (&self.launch_cwd, trust)
    }
    /// Resolve folder trust and load launch-dir MCP configs after `initialize`
    /// returns. The walks are synchronous and expensive in large monorepos; they
    /// must not block the ACP response (embedding clients initialize immediately).
    pub(super) fn spawn_initialize_launch_mcp_setup(&self) {
        let cwd = self.launch_cwd.clone();
        let remote_settings = self.cfg.borrow().remote_settings.clone();
        let agent_mcp_state = self.agent_mcp_state.clone();
        tokio::task::spawn_local(async move {
            let local_mcp_servers = match tokio::task::spawn_blocking(move || {
                    let local = crate::util::config::load_mcp_servers(&cwd);
                    folder_trust::resolve_and_record(
                        &cwd,
                        remote_settings.as_ref(),
                        false,
                    );
                    folder_trust::filter_untrusted_project_mcp(&cwd, local)
                })
                .await
            {
                Ok(servers) => servers,
                Err(e) => {
                    tracing::warn!(error = %e, "initialize MCP setup task failed");
                    return;
                }
            };
            if !local_mcp_servers.is_empty() {
                agent_mcp_state.lock().await.update_configs(local_mcp_servers.clone());
            }
        });
    }
    pub fn agent_mcp_state(
        &self,
    ) -> std::sync::Arc<tokio::sync::Mutex<crate::session::mcp_servers::McpState>> {
        self.agent_mcp_state.clone()
    }
    /// Build the launch-dir plugin registry snapshot on first use.
    ///
    /// Boot-time discovery was deferred past ACP `initialize` (the cwd→git-root
    /// plus user/marketplace walks stalled embedding clients' first `initialize`),
    /// leaving `plugin_registry_handle` empty. That shared snapshot still backs
    /// the launch-dir plugin MCP/LSP merges read in `resolve_mcp_servers` and
    /// the session LSP build, so populate it lazily — off the `initialize`
    /// critical path — on the first session-creating call. Runs the discovery
    /// walk once; per-session `build_for_cwd` still re-resolves project-scoped
    /// plugins for each session's own cwd.
    pub(super) fn ensure_plugin_registry(&self) {
        if self.plugin_registry_initialized.replace(true) {
            return;
        }
        let (cwd, trusted) = self.prime_launch_dir_trust();
        let mut plugins = self.cfg.borrow().plugins.clone();
        let disk_config = plugins.to_discovery_config();
        let count = self
            .plugin_registry_handle
            .reload(Some(cwd), &disk_config, trusted, false);
        tracing::debug!(
            plugin_count = count,
            "lazily populated plugin registry snapshot"
        );
    }
    /// Merge configured MCP sources with client-provided servers.
    pub(super) fn resolve_mcp_servers(
        &self,
        client_servers: Vec<acp::McpServer>,
        cwd: &std::path::Path,
    ) -> Vec<acp::McpServer> {
        self.ensure_plugin_registry();
        crate::session::mcp_catalog::merge_mcp_servers(
            client_servers,
            cwd,
            self.plugin_registry_handle.snapshot().as_deref(),
        )
    }
    /// Set the memory configuration (called from TUI after config resolution).
    pub fn set_memory_config(&mut self, config: crate::config::MemoryConfig) {
        self.memory_config = if config.enabled { Some(config) } else { None };
    }
    /// Adopt the leader's [`AgentActivity`] so the auto-update checker sees
    /// the agent's live view of running turns/subagents and can flush
    /// sessions at shutdown.
    ///
    /// Must be called right after construction: entries registered on the
    /// constructor-created default instance are NOT migrated.
    pub fn set_activity(&mut self, activity: crate::agent::activity::AgentActivity) {
        self.activity = activity;
    }
    /// Send [`SessionCommand::Shutdown`] to every live session actor and wait
    /// up to `grace` for them to exit (SessionEnd hooks, memory save, etc.).
    ///
    /// Call on non-leader process quit **after** the cancel token fires but
    /// **before** dropping the agent / exiting the process, so session actors
    /// are not killed mid-hook. Mirrors the leader auto-update / relaunch
    /// flush path ([`crate::agent::activity::AgentActivity::flush_all_sessions`]).
    pub async fn flush_all_sessions(&self, grace: std::time::Duration) {
        self.activity.flush_all_sessions(grace).await;
    }
    /// Install the channel that fans new session cwds into the leader's
    /// `ConfigFileWatcher::watch_path`. Called once after
    /// the watcher is constructed in `agent/app.rs`. In simple /
    /// non-leader mode the channel is never wired and
    /// `notify_session_cwd_for_watch` is a no-op.
    pub fn set_config_watcher_path_tx(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<std::path::PathBuf>,
    ) {
        self.config_watcher_path_tx = Some(tx);
    }
    /// Best-effort fan-out of a new session's `cwd` to the leader's
    /// `ConfigFileWatcher` for dynamic non-recursive registration
    /// No-op if the channel was never installed
    /// (`set_config_watcher_path_tx` was not called — simple mode,
    /// tests) or if the receiver has been dropped. Watcher errors are
    /// logged inside the spawned task and do NOT propagate here.
    pub(crate) fn notify_session_cwd_for_watch(&self, cwd: &std::path::Path) {
        if let Some(tx) = self.config_watcher_path_tx.as_ref()
            && tx.send(cwd.to_path_buf()).is_err()
        {
            tracing::debug!(
                cwd = %cwd.display(),
                "config watcher path channel closed; session cwd not registered"
            );
        }
    }
    /// Pre-session command availability snapshot.
    ///
    /// Used by the `grow/commands/list` ext method and the
    /// `InitializeResponse._meta` path (`builtin_commands()`), both of
    /// which fire before any session exists. The eventual agent's toolset
    /// is unknown (depends on the model the user picks), so we fail-closed
    /// for runtime/tool-dependent gates (`/flush`, `/loop`, `/memory`,
    /// …) and let the session-scoped `available_commands_update` in
    /// `acp_session.rs` fill in the real per-model gating as soon as a
    /// session starts.
    ///
    /// otherwise it wouldn't appear in the slash menu until after the
    pub(crate) fn command_availability(
        &self,
    ) -> crate::session::slash_commands::CommandAvailability {
        crate::session::slash_commands::CommandAvailability {
            goal: self.cfg.borrow().resolve_goal().value,
            workflows: self.cfg.borrow().resolve_workflows().value,
            ..crate::session::slash_commands::CommandAvailability::default()
        }
    }
    /// Current client type as set by the most recent `initialize()` call.
    pub(crate) fn client_type(&self) -> ClientType {
        *self.client_type.borrow()
    }
    /// Most recently allocated turn number for `sid`, or `None` if the
    /// session has not started a turn yet.
    pub(crate) fn session_turn_number(&self, sid: &acp::SessionId) -> Option<u64> {
        self.retained_resources.borrow().get(sid).and_then(|d| d.turn_number)
    }
    pub(crate) fn allocate_turn_number(&self, session_id: &acp::SessionId) -> u64 {
        let turn = self.peek_turn_number(session_id);
        self.set_turn_number(session_id, turn.saturating_add(1));
        turn
    }
    /// Read a session's next turn number without advancing the counter.
    fn peek_turn_number(&self, session_id: &acp::SessionId) -> u64 {
        self.session_turn_number(session_id).unwrap_or(0u64)
    }
    /// Set a session's next turn number.
    pub(super) fn set_turn_number(&self, session_id: &acp::SessionId, next: u64) {
        self.retained_resources
            .borrow_mut()
            .entry(session_id.clone())
            .or_default()
            .turn_number = Some(next);
    }
    /// Shared plugin registry handle used by extensions for snapshot/reload.
    pub(crate) fn plugin_registry_handle(
        &self,
    ) -> &agent::plugins::SharedPluginRegistryHandle {
        &self.plugin_registry_handle
    }
    /// Resolved cli-chat-proxy base for session features (via
    /// `proxy_url`). Not for the deployment-config fetch.
    pub(crate) fn cli_chat_proxy_base_url(&self) -> String {
        self.cfg.borrow().endpoints.proxy_url()
    }
    pub(crate) fn alpha_test_key(&self) -> Option<String> {
        self.cfg.borrow().endpoints.alpha_test_key.clone()
    }
    /// Build the process-lifetime local `WorkspaceOps` on first use.
    ///
    /// Deferred past ACP wiring so `initialize` can respond before folder-trust
    /// scans and `WorkspaceHandle::new_minimal` run (same boot stall as plugin
    /// discovery for persistent ACP clients on Windows).
    fn ensure_local_workspace_ops(
        &self,
    ) -> Result<workspace::WorkspaceOps, acp::Error> {
        if let Some(ops) = self.workspace_ops.borrow().clone() {
            return Ok(ops);
        }
        let (cwd, project_lsp_trusted) = self.prime_launch_dir_trust();
        let ops = match workspace::handle::WorkspaceHandle::new_minimal(
            cwd.to_path_buf(),
            project_lsp_trusted,
        ) {
            Ok(handle) => workspace::WorkspaceOps::local(handle),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "failed to create local WorkspaceHandle"
                );
                return Err(
                    acp::Error::internal_error().data("workspace not initialized"),
                );
            }
        };
        *self.workspace_ops.borrow_mut() = Some(ops.clone());
        Ok(ops)
    }
    /// Resolve the workspace ops, returning `Err` if not yet initialized.
    ///
    /// Only `None` before the first lazy local build via
    /// [`Self::ensure_local_workspace_ops`]. Called at the `ext_method`
    /// dispatch boundary and in session spawn; extensions receive the
    /// resolved `&WorkspaceOps` directly.
    pub(crate) fn resolve_workspace_ops(
        &self,
    ) -> Result<workspace::WorkspaceOps, acp::Error> {
        let ops = self.ensure_local_workspace_ops()?;
        let handle = ops.workspace_handle();
        if !handle.has_client_ext_sink() {
            let gw = self.gateway.clone();
            handle
                .set_client_ext_sink(
                    std::sync::Arc::new(move |method: String, params: serde_json::Value| {
                        if let Ok(raw) = serde_json::value::to_raw_value(&params) {
                            gw.forward_fire_and_forget(
                                acp::ExtNotification::new(method, raw.into()),
                            );
                        }
                    }),
                );
        }
        Ok(ops)
    }
    pub(crate) fn deployment_key(&self) -> Option<String> {
        self.cfg.borrow().endpoints.deployment_key.clone()
    }
    /// Push the current, expiry-filtered local announcement configuration to
    /// connected clients. The notification is deliberately stateless: local
    /// config reload and client initialization both publish an authoritative
    /// replacement snapshot.
    pub(crate) fn emit_announcements(&self) {
        let announcements = announcements::filter_expired(
            self.cfg.borrow().announcements.clone(),
        );
        let payload = announcements::AnnouncementsUpdated {
            announcements: announcements.clone(),
        };
        let Ok(params) = serde_json::value::to_raw_value(&payload) else {
            return;
        };
        self.gateway.forward_fire_and_forget(
            acp::ExtNotification::new("grow/announcements/update", params.into()),
        );
        tracing::info!(
            count = announcements.len(),
            "pushing local announcements update to clients"
        );
    }
    pub(super) async fn send_model_auto_switched(
        &self,
        session_id: &acp::SessionId,
        previous: &acp::ModelId,
        new: &acp::ModelId,
        reason: &str,
    ) {
        let notification = crate::extensions::notification::SessionNotification {
            session_id: session_id.clone(),
            update: crate::extensions::notification::SessionUpdate::ModelAutoSwitched {
                previous_model_id: previous.0.to_string(),
                new_model_id: new.0.to_string(),
                reason: reason.to_string(),
            },
            meta: None,
        };
        if let Ok(params) = serde_json::value::to_raw_value(&notification) {
            let _ = self
                .gateway
                .ext_notification(
                    acp::ExtNotification::new("grow/session_notification", params.into()),
                )
                .await;
        }
    }
    /// Pure id → entry resolver (the `allowed_models` gate lives in `set_session_model`).
    pub(crate) fn resolve_model_id(
        &self,
        requested: &acp::ModelId,
    ) -> Result<ModelEntry, acp::Error> {
        let requested_str = requested.0.as_ref();
        let models = self.models_manager.models();
        let Some(entry) = models.get(requested_str) else {
            tracing::debug!(
                requested = %requested_str,
                model_count = models.len(),
                "resolve_model_id: unknown provider/model catalog id"
            );
            return Err(acp::Error::invalid_params().data("unknown model id"));
        };
        tracing::debug!(
            "resolve_model_id: matched catalog id: requested={} routing_model={}",
            requested_str,
            entry.info.model
        );
        Ok(entry.clone())
    }
    pub(crate) fn prepare_sampling_config_for_model(
        &self,
        model: &ModelEntry,
        origin_client: Option<crate::http::OriginClientInfo>,
    ) -> SamplingConfig {
        let credentials = resolve_credentials(model);
        let cfg = self.cfg.borrow();
        let alpha_test_key = cfg.endpoints.alpha_test_key.clone();
        drop(cfg);
        let mut config =
            crate::agent::config::sampling_config_for_model(model, credentials, alpha_test_key);
        config.origin_client = origin_client;
        config
    }
    /// Resolve sampling config for a model by ID, falling back to the global
    /// default on resolution failure. This ensures API-key auth routes to
    /// the public API (via resolve_credentials) instead of the global config's
    /// cli-chat-proxy base_url.
    pub(super) fn resolve_sampling_config_for_model(
        &self,
        model_id: &acp::ModelId,
        origin_client: Option<crate::http::OriginClientInfo>,
    ) -> SamplingConfig {
        if let Ok(model) = self.resolve_model_id(model_id) {
            self.prepare_sampling_config_for_model(&model, origin_client.clone())
        } else {
            let mut c = self.sampling_config.borrow().clone();
            c.origin_client = origin_client;
            c
        }
    }
    /// Build deploy-service config. The tool talks directly to the deployer service.
    pub(super) fn prepare_app_builder_deployer_config(
        &self,
    ) -> tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig {
        use tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig;
        AppBuilderDeployerConfig::Disabled
    }
    /// Returns `Err` with a user-facing message on invalid config; the caller at
    /// the process boundary prints it and exits.
    pub fn new(gateway: GatewaySender, cfg: &AgentConfig) -> Result<Self, String> {
        let (cfg, models_manager) = crate::agent::init::bootstrap(cfg)?;
        Ok(Self::with_models(gateway, &cfg, models_manager))
    }
    /// Prepare the web fetch configuration based on feature flags.
    ///
    /// Enabled gate: `GROW_WEB_FETCH` env > remote settings
    /// `web_fetch_enabled` > default (false).
    ///
    /// Params resolution (TOML > env > remote settings > default):
    /// - `proxy_endpoint`: `[toolset.web_fetch] proxy_endpoint` > `GROW_WEB_FETCH_PROXY` > remote settings > None
    /// - `allowed_domains`: `[toolset.web_fetch] allowed_domains` > remote settings > built-in defaults
    /// - `allow_local`: `[toolset.web_fetch] allow_local` > `GROW_WEB_FETCH_ALLOW_LOCAL` > false
    pub(super) fn prepare_web_fetch_config(
        &self,
    ) -> tools::implementations::grow_build::web_fetch::WebFetchConfig {
        use tools::implementations::grow_build::web_fetch::WebFetchConfig;
        let cfg = self.cfg.borrow();
        let remote = cfg.remote_settings.as_ref();
        let enabled = cfg.resolve_web_fetch();
        if !enabled.value {
            return WebFetchConfig::Disabled;
        }
        let context_window = Some(self.sampling_config.borrow().context_window);
        let params = cfg
            .toolset
            .web_fetch
            .resolve_params(
                remote.and_then(|s| s.web_fetch_proxy.as_deref()),
                remote.and_then(|s| s.web_fetch_allowed_domains.as_deref()),
                context_window,
            );
        if params.allowed_domains.as_ref().is_some_and(Vec::is_empty) {
            tracing::info!("web_fetch disabled: allowed_domains is explicitly empty");
            return WebFetchConfig::Disabled;
        }
        WebFetchConfig::Enabled { params }
    }
    /// Construct from pre-built components. Use when the caller needs the
    /// `ModelsManager` handle externally (e.g. `run_leader` wires it to the
    /// config watcher). Otherwise prefer [`Self::new`].
    pub fn with_models(
        gateway: GatewaySender,
        cfg: &AgentConfig,
        models_manager: crate::agent::models::ModelsManager,
    ) -> Self {
        models_manager.set_gateway(gateway.clone());
        let sampling_config = models_manager.sampling_config();
        let default_permission_mode = cfg.default_permission_mode;
        let config_root = crate::config::load_effective_config().ok();
        let empty_config = toml::Value::Table(toml::map::Map::new());
        let raw = config_root.as_ref().unwrap_or(&empty_config);
        let (worktree_type, wt_source) = crate::util::config::resolve_worktree_type(
            raw,
            cfg.remote_settings.as_ref(),
        );
        let restore_code = crate::util::config::resolve_restore_code(
            raw,
            cfg.remote_settings.as_ref(),
        );
        tracing::info!(
            worktree_type = ?worktree_type,
            source = wt_source,
            "WORKTREE_CONFIG_SHELL: resolved worktree type at agent startup"
        );
        let (subagent_event_tx, subagent_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let activity = crate::agent::activity::AgentActivity::default();
        Self {
            sessions: RefCell::new(HashMap::new()),
            activity,
            loading_sessions: RefCell::new(HashMap::new()),
            retained_resources: RefCell::new(HashMap::new()),
            session_threads: RefCell::new(HashMap::new()),
            resident_roster_titles: RefCell::new(HashMap::new()),
            initialize_request: OnceLock::new(),
            gateway,
            launch_cwd: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".")),
            launch_dir_trust: std::cell::OnceCell::new(),
            plugin_registry_handle: agent::plugins::SharedPluginRegistryHandle::new(
                None,
                cfg.plugins.cli_plugin_dirs.clone(),
            ),
            plugin_registry_initialized: std::cell::Cell::new(false),
            models_manager,
            cfg: RefCell::new(cfg.clone()),
            auth_method_id: crate::agent::auth_method::new_shared_auth_method_id(None),
            sampling_config: RefCell::new(sampling_config),
            client_type: RefCell::new(ClientType::default()),
            code_nav_enabled: std::cell::Cell::new(false),
            default_permission_mode,
            memory_config: None,
            config_watcher_path_tx: None,
            buffering_settings: RefCell::new(None),
            background_copy_context: BackgroundCopyContext::new(),
            codebase_indexes: Arc::new(
                parking_lot::Mutex::new(CodebaseIndexManager::new()),
            ),
            resident_resources: RefCell::new(HashMap::new()),
            worktree_type,
            restore_code,
            agent_mcp_state: std::sync::Arc::new(
                tokio::sync::Mutex::new(
                    crate::session::mcp_servers::McpState::new(vec![]),
                ),
            ),
            model_unavailable_sessions: RefCell::new(std::collections::HashMap::new()),
            subagent_event_tx,
            subagent_event_rx: RefCell::new(Some(subagent_event_rx)),
            subagent_presentation: RefCell::new(
                crate::agent::subagent::SubagentPresentation::new(),
            ),
            monitor_event_buffer: tools::implementations::grow_build::monitor::types::MonitorEventBuffer::default(),
            bundle_sync_in_flight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            workspace_ops: RefCell::new(None),
            session_live_state: RefCell::new(HashMap::new()),
            supervisor_started: std::cell::Cell::new(false),
            #[cfg(test)]
            roster_delta_spy: RefCell::new(Vec::new()),
            #[cfg(test)]
            supervisor_spawn_count: std::cell::Cell::new(0),
        }
    }
    /// Handle `grow/internal/evict_sessions` — the leader server tells us a
    /// client disconnected and these sessions lost their IPC owner.
    ///
    /// **This is the no-evict keystone.** A disconnect must
    /// NOT destroy a session. The behavior is now *detach + keep-resident +
    /// idle-unload*:
    ///
    /// - **Sessions with live work stay resident.** We do NOT send `Shutdown`
    ///   and do NOT drop the `SessionHandle`, so the actor, its pending
    ///   permission oneshots, and its `KillOnDrop` tool subprocesses all
    ///   survive. The route/driver detach is groundwork for PR-3 (the
    ///   driver/subscriber maps don't exist yet), so for now we only mark the
    ///   live state.
    /// - **Fully idle sessions are unloaded to disk** to bound memory (the
    ///   `sessions`/`session_threads` maps are uncapped). This preserves the
    ///   legacy unload path — `Shutdown` the actor, drop the `SessionHandle`,
    ///   but KEEP the `SessionThread` so `drain_old_session_thread` can drain it
    ///   on reconnect — and crucially does **not** finalize the cloud replica
    ///   (the session remains resumable via `session/load`).
    ///
    /// The "live work" check is the coarse PR-2 stub (`session_has_live_work`);
    /// the full `SessionActivity` signal lands in PR-4.
    pub(super) async fn handle_evict_sessions(
        &self,
        params: &serde_json::value::RawValue,
    ) {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct EvictParams {
            session_ids: Vec<String>,
        }
        let Ok(p) = serde_json::from_str::<EvictParams>(params.get()) else {
            tracing::warn!("Failed to parse evict_sessions params");
            return;
        };
        if p.session_ids.is_empty() {
            return;
        }
        tracing::info!(
            count = p.session_ids.len(),
            sessions = ?p.session_ids,
            "Client disconnected; detaching sessions (no-evict keystone)"
        );
        let checks = p
            .session_ids
            .iter()
            .map(|sid| {
                let id = acp::SessionId::new(sid.clone());
                async move {
                    let busy = self.session_has_live_work(&id).await;
                    (id, busy)
                }
            });
        let resolved = futures::future::join_all(checks).await;
        let mut kept_resident: usize = 0;
        let mut unloaded: usize = 0;
        for (id, busy) in resolved {
            if busy {
                self.set_session_live_state(&id, SessionLiveState::Working);
                kept_resident += 1;
                tracing::info!(
                    session_id = %id.0,
                    "kept session resident across client disconnect (live work)"
                );
                continue;
            }
            self.request_session_shutdown(&id);
            if self.take_session(&id).is_some() {
                self.resident_resources.borrow_mut().remove(&id);
                self.set_session_live_state(&id, SessionLiveState::Dormant);
                unloaded += 1;
                tracing::debug!(session_id = %id.0, "idle session unloaded to disk on disconnect");
            }
        }
        tracing::info!(kept_resident, unloaded, "client-disconnect detach complete");
        self.sweep_dead_sessions();
    }
    /// Wait for an old session thread to finish before reloading the same session.
    ///
    /// When a client disconnects and a session is *idle*, `handle_evict_sessions`
    /// unloads it: sends `Shutdown`, drops the `SessionHandle`, and keeps the
    /// `SessionThread`. (Sessions with live work stay fully resident and skip
    /// this path.) If the client reconnects and loads the same session, we must
    /// wait for the old actor to finish flushing to disk before replaying
    /// `updates.jsonl`.
    ///
    /// Uses async polling (never blocks the `LocalSet` runtime) with a 5s deadline
    /// to handle slow shutdowns (e.g., embedding API timeouts).
    pub(super) async fn drain_old_session_thread(&self, session_id: &acp::SessionId) {
        let thread = self.session_threads.borrow_mut().remove(session_id);
        let Some(thread) = thread else { return };
        if thread.is_finished() {
            return;
        }
        tracing::info!(
            session_id = %session_id.0,
            "Waiting for old session thread to finish before reload"
        );
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if thread.is_finished() {
                tracing::debug!(
                    session_id = %session_id.0,
                    "Old session thread finished cleanly"
                );
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    session_id = %session_id.0,
                    "Old session thread still running after 5s — proceeding with replay. \
                     Session data may be incomplete if the old actor is still writing."
                );
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    /// Mark a `session/load` as in flight for `session_id`.
    ///
    /// Returns an RAII guard; while it is alive,
    /// [`Self::wait_for_in_flight_session_load`] blocks racing session-scoped
    /// requests for the same session. Dropping the guard (every exit path of
    /// `load_session`, success or error) removes the marker and wakes all
    /// waiters via watch-channel closure.
    pub(super) fn begin_session_load(
        &self,
        session_id: &acp::SessionId,
    ) -> SessionLoadGuard<'_> {
        let (tx, rx) = tokio::sync::watch::channel(false);
        self.loading_sessions.borrow_mut().insert(session_id.clone(), rx.clone());
        SessionLoadGuard {
            agent: self,
            session_id: session_id.clone(),
            rx,
            _tx: tx,
        }
    }
    /// Session lookup that tolerates an in-flight `session/load`.
    ///
    /// THE chokepoint for the post-leader-crash error class: every
    /// user-facing session-scoped handler (`prompt`, `set_session_model`,
    /// `set_session_mode`, `interject`, ...) resolves its handle through
    /// this instead of a bare `sessions` lookup, so a request racing the
    /// reconnect-replayed `session/load` waits for the session to land
    /// rather than failing with "unknown session id" / "session not found".
    ///
    /// Returns `None` only when the session is genuinely absent — no load in
    /// flight (or the load failed / timed out), exactly the cases where the
    /// legacy error is correct.
    pub(crate) async fn session_handle_waiting_for_load(
        &self,
        session_id: &acp::SessionId,
    ) -> Option<crate::session::SessionHandle> {
        let existing = self.sessions.borrow().get(session_id).cloned();
        if existing.is_some() {
            return existing;
        }
        self.wait_for_in_flight_session_load(session_id).await;
        self.sessions.borrow().get(session_id).cloned()
    }
    /// If a `session/load` for `session_id` is in flight, wait (bounded) for
    /// it to finish. Returns immediately when no load is in flight.
    ///
    /// This closes the load-vs-request race after a leader restart: clients
    /// replay `session/load` on reconnect, and a `session/prompt` arriving
    /// right behind it must wait for the session to land in `self.sessions`
    /// instead of failing with "unknown session id". The wait wakes when the
    /// load's [`SessionLoadGuard`] drops (success or failure) and re-checks;
    /// a failed load still surfaces the original error to the caller.
    pub(crate) async fn wait_for_in_flight_session_load(
        &self,
        session_id: &acp::SessionId,
    ) {
        const LOAD_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(
            60,
        );
        let deadline = tokio::time::Instant::now() + LOAD_WAIT_TIMEOUT;
        loop {
            if self.sessions.borrow().contains_key(session_id) {
                return;
            }
            let rx = self.loading_sessions.borrow().get(session_id).cloned();
            let Some(mut rx) = rx else { return };
            let now = tokio::time::Instant::now();
            if now >= deadline {
                tracing::warn!(
                    session_id = %session_id.0,
                    "timed out waiting for in-flight session/load"
                );
                return;
            }
            let _ = tokio::time::timeout(deadline - now, rx.changed()).await;
        }
    }
    /// Returns the default permission mode for new sessions.
    pub fn default_permission_mode(&self) -> crate::util::config::PermissionMode {
        self.default_permission_mode
    }
    /// Returns the background copy context for managing background file copy tasks.
    pub fn background_copy_context(&self) -> BackgroundCopyContext {
        self.background_copy_context.clone()
    }
    /// Move a foreground bash command to background.
    /// Routes through the session's tool bridge to unblock the agent loop.
    pub async fn background_foreground_command(
        &self,
        session_id: &str,
        tool_call_id: &str,
    ) -> bool {
        let sid = acp::SessionId::new(session_id);
        if let Some(handle) = self.get_session_handle(&sid) {
            handle.background_foreground_command(tool_call_id).await
        } else {
            false
        }
    }
    /// Kill a background task by task_id.
    /// Routes through the session's tool bridge to the TerminalBackend.
    pub async fn kill_background_task(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<tools::types::KillOutcome, String> {
        let sid = acp::SessionId::new(session_id);
        if let Some(handle) = self.get_session_handle(&sid) {
            handle.kill_background_task(task_id).await
        } else {
            Err("session not found".to_string())
        }
    }
    pub async fn delete_scheduled_task(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<bool, String> {
        let sid = acp::SessionId::new(session_id);
        if let Some(handle) = self.get_session_handle(&sid) {
            handle.delete_scheduled_task(task_id).await
        } else {
            Err("session not found".to_string())
        }
    }
    /// Cancel a subagent by id, returning a typed outcome that backs the pager's
    /// `grow/subagent/cancel`. Active/pending → cancelled (a finish follows);
    /// already-finished → its terminal status; unknown id → `NotFound`.
    pub async fn cancel_subagent(
        &self,
        subagent_id: &str,
    ) -> tools::implementations::grow_build::task::types::SubagentCancelOutcome {
        tools::implementations::grow_build::task::backend::ChannelBackend::new(
                self.subagent_event_tx.clone(),
            )
            .cancel(subagent_id)
            .await
    }
    pub(crate) async fn list_running_subagents(
        &self,
        parent_session_id: &str,
    ) -> Vec<
        tools::implementations::grow_build::task::types::SubagentInspection,
    > {
        tools::implementations::grow_build::task::backend::ChannelBackend::new(
                self.subagent_event_tx.clone(),
            )
            .list_running(parent_session_id)
            .await
    }
    pub(crate) async fn inspect_subagent(
        &self,
        subagent_id: &str,
    ) -> Option<
        tools::implementations::grow_build::task::types::SubagentInspection,
    > {
        tools::implementations::grow_build::task::backend::ChannelBackend::new(
                self.subagent_event_tx.clone(),
            )
            .inspect(subagent_id)
            .await
    }
    pub(crate) async fn query_subagent(
        &self,
        subagent_id: &str,
        block: bool,
        timeout_ms: Option<u64>,
    ) -> Option<
        tools::implementations::grow_build::task::types::SubagentSnapshot,
    > {
        tools::implementations::grow_build::task::backend::ChannelBackend::new(
                self.subagent_event_tx.clone(),
            )
            .query(subagent_id, block, timeout_ms)
            .await
    }
    /// List all background tasks for a session.
    /// Routes through the session's tool bridge to the TerminalBackend.
    pub async fn list_tasks(
        &self,
        session_id: &str,
    ) -> Option<Vec<tools::types::TaskSnapshot>> {
        let sid = acp::SessionId::new(session_id);
        if let Some(handle) = self.get_session_handle(&sid) {
            handle.list_tasks().await
        } else {
            None
        }
    }
    /// Flush a session's persistence buffer with a 5-second timeout.
    ///
    /// Sends `FlushComplete` to the session actor, which chains through to
    /// `FlushAndAck` on the persistence actor — a true sync barrier that only
    /// resolves after all queued writes (chat messages, updates) hit disk.
    ///
    /// Returns `Ok(())` on success, `Err(reason)` on timeout or channel failure.
    pub(crate) async fn flush_session(
        &self,
        session_id: &acp::SessionId,
    ) -> Result<(), &'static str> {
        let cmd_tx = self.sessions.borrow().get(session_id).map(|h| h.cmd_tx.clone());
        let Some(cmd_tx) = cmd_tx else {
            return Err("session not found");
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        if cmd_tx
            .send(SessionCommand::FlushComplete {
                respond_to: tx,
            })
            .is_err()
        {
            return Err("send failed");
        }
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err("channel closed"),
            Err(_) => Err("timeout"),
        }
    }
    /// Get a session's cwd by session_id.
    /// Returns None if the session is not found.
    pub fn get_session_cwd(&self, session_id: &acp::SessionId) -> Option<PathBuf> {
        let sessions = self.sessions.borrow();
        sessions.get(session_id).map(|handle| PathBuf::from(&handle.info.cwd))
    }
    /// Get a session handle by session_id.
    /// Returns None if the session is not found.
    pub fn get_session_handle(
        &self,
        session_id: &acp::SessionId,
    ) -> Option<crate::session::SessionHandle> {
        let sessions = self.sessions.borrow();
        sessions.get(session_id).cloned()
    }
    /// Get hooks list for a session (for `grow/hooks/list` extension).
    pub async fn list_hooks(
        &self,
        session_id: &acp::SessionId,
    ) -> Option<extension_types::HooksListResponse> {
        let handle = self.get_session_handle(session_id)?;
        handle.get_hooks_list().await
    }
    /// Execute a hooks management action (for `grow/hooks/action`).
    pub async fn execute_hooks_action(
        &self,
        session_id: &acp::SessionId,
        action: extension_types::HooksAction,
    ) -> Option<extension_types::ActionOutcome> {
        let handle = self.get_session_handle(session_id)?;
        handle.execute_hooks_action(action).await
    }
    /// Execute a plugins management action (for `grow/plugins/action`).
    pub async fn execute_plugins_action(
        &self,
        session_id: &acp::SessionId,
        action: extension_types::PluginsAction,
    ) -> Option<extension_types::ActionOutcome> {
        let is_reload = matches!(action, extension_types::PluginsAction::Reload);
        let handle = self.get_session_handle(session_id)?;
        let outcome = handle.execute_plugins_action(action).await;
        let succeeded = matches!(
            outcome.as_ref().map(|o| &o.status),
            Some(extension_types::OutcomeStatus::Success)
        );
        if is_reload && succeeded {
            self.broadcast_plugin_registry_to_sessions(Some(session_id));
        }
        outcome
    }
    /// Get a snapshot of the shared plugin registry (for `grow/plugins/list`).
    pub fn plugin_registry_snapshot(
        &self,
    ) -> Option<std::sync::Arc<agent::plugins::PluginRegistry>> {
        self.plugin_registry_handle.snapshot()
    }
    /// Run content search at agent level.
    /// This allows content search to work with just a cwd, without requiring a session.
    /// Resolve client version: prefer the value from the initialize request _meta,
    /// fall back to the agent's own version (VERSION_WITH_COMMIT set by the TUI launcher).
    pub(super) fn client_version(&self) -> Option<String> {
        self.initialize_request
            .get()
            .and_then(|req| req.meta.as_ref())
            .and_then(|m| m.get("clientVersion"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.cfg.borrow().client_version.clone())
    }
    pub(super) fn origin_client_info_from_meta(
        &self,
        meta: Option<&acp::Meta>,
    ) -> Option<crate::http::OriginClientInfo> {
        crate::http::merge_origin_client_info(
                crate::http::origin_client_info_from_meta(meta),
                crate::http::origin_client_info_from_meta(
                        self.initialize_request.get().and_then(|req| req.meta.as_ref()),
                    )
                    .map(|mut origin| {
                        if origin.version.is_none() {
                            origin.version = self.client_version();
                        }
                        origin
                    }),
            )
            .map(|mut origin| {
                if origin.version.is_none() {
                    origin.version = self.client_version();
                }
                origin
            })
    }
    /// Returns the model state for a given session (or the agent default).
    ///
    /// When `session_id` is `Some`, looks up the session's per-session model.
    /// Falls back to `current_model_id` (startup default) when no session is
    /// found or `session_id` is `None` (e.g., during `initialize` before any
    /// session exists).
    pub fn model_state(
        &self,
        session_id: Option<&acp::SessionId>,
    ) -> acp::SessionModelState {
        let model_id = lookup_session_model(
            &self.sessions.borrow(),
            session_id,
            &self.models_manager.current_model_id(),
        );
        let mut available_models: Vec<acp::ModelInfo> = self
            .models_manager
            .available()
            .values()
            .cloned()
            .collect();
        let override_effort = session_id
            .and_then(|sid| self.sessions.borrow().get(sid).map(|h| h.reasoning_effort))
            .flatten()
            .or_else(|| {
                self.models_manager
                    .model_default_reasoning_effort(model_id.0.as_ref())
            });
        if let Some(override_effort) = override_effort
            && let Some(info) = available_models
                .iter_mut()
                .find(|info| info.model_id == model_id)
            && parse_reasoning_efforts_meta(info.meta.as_ref()).is_some()
        {
            let mut map = info.meta.clone().unwrap_or_default();
            map.insert(
                REASONING_EFFORT_META_KEY.to_string(),
                reasoning_effort_meta_value(override_effort),
            );
            info.meta = Some(map);
        }
        acp::SessionModelState::new(model_id, available_models)
    }
    pub(super) fn session_config_options(
        &self,
        session_id: Option<&acp::SessionId>,
        state: &acp::SessionModelState,
    ) -> Vec<session_config::SessionConfigOption> {
        let model_id = state.current_model_id.clone();
        let effort_options: Vec<ReasoningEffortOption> = self
            .models_manager
            .model_reasoning_efforts(model_id.0.as_ref());
        let supports_effort = !effort_options.is_empty();
        let current_effort = if supports_effort {
            session_id
                .and_then(|sid| {
                    self.sessions.borrow().get(sid).map(|h| h.reasoning_effort)
                })
                .flatten()
                .or_else(|| {
                    self
                        .models_manager
                        .model_default_reasoning_effort(model_id.0.as_ref())
                })
        } else {
            None
        };
        session_config::build_session_config_options(
            &state.available_models,
            &model_id,
            &effort_options,
            current_effort,
        )
    }
    /// Insert the per-session `_meta` keys (`grow/sessionConfig` and
    /// `grow/sessionDetail`) shared by
    /// `new_session` and `load_session`. Keeping both response paths on this one
    /// builder stops them drifting.
    pub(super) fn insert_session_config_meta(
        &self,
        meta: &mut serde_json::Map<String, serde_json::Value>,
        session_id: &acp::SessionId,
        cwd: String,
        title: Option<String>,
        model_state: &acp::SessionModelState,
    ) {
        let config_options = self.session_config_options(Some(session_id), model_state);
        let detail = session_config::GrowSessionDetail::build(
            session_id.0.to_string(),
            cwd,
            model_state.current_model_id.0.to_string(),
            title,
        );
        meta.insert(
            "grow/sessionConfig".to_string(),
            serde_json::json!({ "options": config_options }),
        );
        meta.insert("grow/sessionDetail".to_string(), serde_json::json!(detail));
    }
    /// Warn when no model has an explicit BYOK credential. Per-model
    /// resolution remains deferred to session creation.
    pub(super) fn seed_client_config_auth_if_available(&self) {
        if !self
            .models_manager
            .models()
            .values()
            .any(|m| m.has_own_credentials())
        {
            tracing::warn!("No BYOK credentials found: configure model api_key/env_key/auth_provider");
        }
    }
    /// Resolve the agent definition for a session.
    ///
    /// Resumed sessions first restore their persisted Agent name. New sessions
    /// then resolve an explicit ACP/CLI/config/env selection before using the
    /// global default. Model selection is intentionally independent.
    pub fn resolve_agent_definition(
        cwd: &std::path::Path,
        agent_profile_path: Option<&std::path::Path>,
        agent_config: &config::AgentSelectionConfig,
        acp_agent_profile: Option<agent::AgentDefinition>,
        persisted_agent_name: Option<&str>,
    ) -> agent::AgentDefinition {
        use agent::AgentDefinition;
        if let Some(name) = persisted_agent_name {
            if let Some(definition) = agent::discovery::by_name_in_cwd(name, cwd) {
                return definition;
            }
            tracing::warn!(
                agent_name = %name,
                "persisted Agent is unavailable; falling back to the global default"
            );
        }
        if let Some(def) = acp_agent_profile {
            tracing::info!(
                agent_name = %def.name,
                "Using agent profile from ACP _meta.agentProfile"
            );
            return def;
        }
        if let Some(path) = agent_profile_path {
            match AgentDefinition::from_file(path) {
                Ok(def) => return def,
                Err(e) => {
                    tracing::error!(
                        path = %path.display(),
                        error = %e,
                        "Failed to load agent profile from --agent-profile path"
                    );
                    eprintln!(
                        "error: failed to load agent profile '{}': {}",
                        path.display(),
                        e
                    );
                    crate::instrumentation::finalize_and_exit(1);
                }
            }
        }
        if let Some(ref path) = agent_config.definition {
            match AgentDefinition::from_file(path) {
                Ok(def) => {
                    tracing::info!(
                        agent_name = %def.name,
                        path = %path.display(),
                        "Using agent definition from config.toml [agent] definition"
                    );
                    return def;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to load agent definition from config.toml [agent] definition, \
                         falling through to next source"
                    );
                }
            }
        }
        if let Some(ref name) = agent_config.name {
            tracing::info!(
                agent_name = %name,
                "Resolving agent definition from config.toml [agent] name"
            );
            if let Some(def) = agent::discovery::by_name_in_cwd(name, cwd) {
                return def;
            }
            tracing::warn!(
                agent_name = %name,
                "Agent '{}' not found via discovery, falling through to next source",
                name
            );
        }
        let agent_name = std::env::var("GROW_AGENT").ok();
        match agent_name.as_deref() {
            Some("browser-use") | Some("browser_use") => AgentDefinition::browser_use(),
            Some("grow-build-concise") | Some("grow_build_concise") => {
                AgentDefinition::grow_build_concise()
            }
            Some(path) if std::path::Path::new(path).is_absolute() => {
                match AgentDefinition::from_file(path) {
                    Ok(def) => def,
                    Err(e) => {
                        tracing::warn!(
                            path = path,
                            error = %e,
                            "Failed to load agent definition from file, falling back to default"
                        );
                        AgentDefinition::default_grow_build()
                    }
                }
            }
            Some(name) => {
                agent::discovery::by_name_in_cwd(name, cwd)
                    .unwrap_or_else(AgentDefinition::default_grow_build)
            }
            None => AgentDefinition::default_grow_build(),
        }
    }
    /// Extract per-client terminal/fs capabilities from request `_meta`
    /// (injected by the leader). Falls back to the shared `init` OnceCell.
    pub(super) fn resolve_client_io_caps(
        meta: Option<&acp::Meta>,
        init: &acp::InitializeRequest,
    ) -> (bool, bool, bool) {
        let terminal = meta
            .and_then(|m| m.get("clientTerminal"))
            .and_then(|v| v.as_bool())
            .unwrap_or(init.client_capabilities.terminal);
        let fs_read = meta
            .and_then(|m| m.get("clientFsRead"))
            .and_then(|v| v.as_bool())
            .unwrap_or(init.client_capabilities.fs.read_text_file);
        let fs_write = meta
            .and_then(|m| m.get("clientFsWrite"))
            .and_then(|v| v.as_bool())
            .unwrap_or(init.client_capabilities.fs.write_text_file);
        (terminal, fs_read, fs_write)
    }
    /// Spawn and register a session actor given a session id and session parameters.
    ///
    /// Parameters are bundled in [`SessionSpawnOptions`] (named fields) rather than
    /// passed positionally: there are too many same-typed args (`bool`s,
    /// `Option<…>`s) for positional calls to be transposition-safe.
    pub(super) async fn spawn_and_register_session(
        &self,
        init: &acp::InitializeRequest,
        spec: SessionSpawnOptions<'_>,
    ) -> Result<(), acp::Error> {
        let SessionSpawnOptions {
            session_info,
            cwd,
            mcp_servers,
            initial_client_mcp_servers,
            mcp_meta_config_map,
            persistence,
            session_title_route,
            timeline_bootstrap,
            rewind_points_file_path,
            origin_client: _origin_client,
            client_code_nav_enabled,
            client_terminal,
            client_fs_read,
            client_fs_write,
            preloaded_envrc,
            persisted_signals,
            persisted_behavior,
            persisted_goal_mode,
            persisted_control_revision,
            persisted_workflow_runs,
            persisted_announcement_state,
            session_meta,
            persisted_agent_name,
            session_model_id,
            session_permission_mode,
            prompt_display_cwd,
        } = spec;
        let _timer = crate::instrumentation_timer!("session.spawn_and_register");
        let spawn_remote_settings = self.cfg.borrow().remote_settings.clone();
        folder_trust::resolve_and_record(
            cwd.as_path(),
            spawn_remote_settings.as_ref(),
            false,
        );
        let use_acp_fs = client_fs_read && client_fs_write;
        let fs_notify_config = init
            .client_capabilities
            .meta
            .as_ref()
            .and_then(|m| m.get("grow/fs_notify"))
            .and_then(|v| {
                use crate::session::{ClientFsConfig, ClientFsMode};
                use fsnotify::FsConfig;
                if v.as_bool() == Some(true) {
                    return Some(ClientFsConfig::default());
                }
                let obj = v.as_object()?;
                if obj.get("enabled").and_then(|e| e.as_bool()) == Some(false) {
                    return None;
                }
                let mode = if obj.get("index").and_then(|i| i.as_bool()) == Some(true) {
                    ClientFsMode::Index
                } else {
                    ClientFsMode::Events
                };
                let mut fs = FsConfig::default();
                if let Some(ms) = obj.get("debounce_ms").and_then(|v| v.as_u64()) {
                    fs.debounce_ms = ms;
                }
                if let Some(patterns) = obj.get("ignore").and_then(|v| v.as_array()) {
                    fs.ignore_patterns = patterns
                        .iter()
                        .filter_map(|p| p.as_str().map(String::from))
                        .collect();
                }
                Some(ClientFsConfig { fs, mode })
            });
        let fs: Arc<dyn workspace::file_system::AsyncFileSystem> = if use_acp_fs {
            let mut acp_fs = AcpSessionFs::new(
                cwd.to_path_buf(),
                session_info.id.clone(),
                self.gateway.clone(),
            );
            if let Some(ref display) = prompt_display_cwd {
                acp_fs = acp_fs.with_display_cwd(std::path::PathBuf::from(display));
            }
            Arc::new(acp_fs)
        } else {
            Arc::new(LocalFs::new(cwd.to_path_buf()))
        };
        let gateway_enabled = std::sync::Arc::new(
            std::sync::atomic::AtomicBool::new(true),
        );
        let terminal: std::sync::Arc<dyn crate::terminal::AsyncTerminalRunner> = if client_terminal {
            std::sync::Arc::new(AcpTerminalRunner {
                gateway: self.gateway.clone(),
                session_id: session_info.id.clone(),
            })
        } else {
            let notifier: std::sync::Arc<
                dyn crate::terminal::SessionNotificationSender,
            > = std::sync::Arc::new(
                crate::terminal::GatedNotifier::new(
                    std::sync::Arc::new(self.gateway.clone()),
                    gateway_enabled.clone(),
                ),
            );
            std::sync::Arc::new(TerminalRunner::new(notifier, session_info.id.clone()))
        };
        let load_envrc = self.cfg.borrow().session.load_envrc.unwrap_or(true);
        let mut startup_hints = init
            .meta
            .as_ref()
            .and_then(|m| m.get("startupHints"))
            .and_then(|v| {
                serde_json::from_value::<crate::session::StartupHints>(v.clone()).ok()
            })
            .unwrap_or_default();
        startup_hints.subagent_permission_mode =
            Some(self.cfg.borrow().subagent_permission_mode);
        let hunk_plan = plan_hunk_tracking(
            init
                .client_capabilities
                .meta
                .as_ref()
                .and_then(|m| m.get("grow/hunkTracker"))
                .and_then(|v| v.get("mode"))
                .and_then(|v| v.as_str()),
        );
        let incremental_bash_output = init
            .client_capabilities
            .meta
            .as_ref()
            .and_then(|m| m.get("grow/incrementalBashOutput"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let no_color = init
            .client_capabilities
            .meta
            .as_ref()
            .and_then(|m| m.get("grow/bashOutputNoColor"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let hunk_tracking_enabled = hunk_plan.enabled();
        let hunk_tracker_handle = match hunk_plan.actor_mode {
            Some(mode) => {
                let cancel = CancellationToken::new();
                let (hunk_event_tx, mut hunk_event_rx) = tokio::sync::mpsc::unbounded_channel();
                let handle = HunkTrackerActor::spawn(
                    session_info.id.0.to_string(),
                    cwd.as_path().to_path_buf(),
                    hunk_event_tx,
                    mode,
                    cancel.clone(),
                );
                tokio::spawn(async move {
                    while hunk_event_rx.recv().await.is_some() {}
                });
                handle
            }
            None => hunk_tracker::HunkTrackerHandle::noop(),
        };
        let project_env_trusted = folder_trust::project_scope_allowed(cwd.as_path());
        let mut session_env = std::collections::HashMap::new();
        let envrc = match preloaded_envrc {
            Some(env) => env,
            None => {
                workspace::envrc::load_envrc_or_empty_when_trusted(
                    cwd.as_path(),
                    load_envrc && project_env_trusted,
                )
            }
        };
        session_env.extend(envrc);
        if no_color {
            session_env.extend(crate::terminal::no_color_env());
        } else {
            session_env.extend(crate::terminal::color_env());
        }
        let mut tool_ctx = ToolContext::with_preloaded_env(
                cwd.clone(),
                Some(self.gateway.clone()),
                Some(session_info.id.clone()),
                fs,
                terminal,
                hunk_tracker_handle,
                session_env,
            )
            .with_hunk_tracking_enabled(hunk_tracking_enabled);
        tool_ctx.process_scope = Some(ProcessScope::new());
        let workspace_ops = self
            .resolve_workspace_ops()
            .map_err(|_| {
                acp::Error::internal_error()
                    .data(
                        "Local workspace initialization failed; cannot create session. \
                 Check that a Tokio runtime is available.",
                    )
            })?;
        tool_ctx.subagent_event_tx = Some(self.subagent_event_tx.clone());
        tool_ctx.is_turn_active = Some(
            self.subagent_presentation.borrow().turn_active_flag(),
        );
        tool_ctx.monitor_event_buffer = Some(self.monitor_event_buffer.clone());
        tool_ctx.subagent_depth = 0;
        tool_ctx.auto_wake_enabled = self.cfg.borrow().auto_wake_enabled;
        let support_permission = self.cfg.borrow().features.support_permission;
        let diagnostics_enabled = true;
        let origin_client = self.origin_client_info_from_meta(init.meta.as_ref());
        let sampling_config = self
            .resolve_sampling_config_for_model(&session_model_id, origin_client.clone());
        if self.auth_method_id.load().is_none() {
            return Err(acp::Error::auth_required().data("no auth method id provided"));
        }
        let auth_method_id = std::sync::Arc::clone(&self.auth_method_id);
        tracing::info!(
            session_id = %session_info.id.0,
            ?startup_hints,
            "startup hints"
        );
        let auto_compact_threshold_percent = {
            let cfg = self.cfg.borrow();
            let models = self.models_manager.models();
            let model = config::find_model_by_catalog_id(&models, &session_model_id.0);
            crate::util::config::resolve_auto_compact_threshold_percent(
                &cfg,
                &session_model_id.0,
                model.map(|e| &e.info),
            )
        };
        let permission_prompt_timeout = self
            .cfg
            .borrow()
            .session
            .permission_prompt_timeout(startup_hints.non_interactive);
        let system_prompt_label = {
            let cfg = self.cfg.borrow();
            let models = self.models_manager.models();
            let model = config::find_model_by_catalog_id(&models, &session_model_id.0);
            crate::util::config::resolve_system_prompt_label(
                &cfg,
                &session_model_id.0,
                model.map(|e| &e.info),
            )
        };
        let compaction_verbatim_input = self
            .cfg
            .borrow()
            .resolve_compaction_verbatim_input();
        let compaction_tool_choice = self.cfg.borrow().resolve_compaction_tool_choice();
        let compaction_pre_prune = self.cfg.borrow().resolve_compaction_pre_prune();
        let compaction_pre_prune_token_budget = self
            .cfg
            .borrow()
            .resolve_compaction_pre_prune_token_budget();
        let auto_update = self.cfg.borrow().cli.auto_update;
        let client_type = *self.client_type.borrow();
        let buffering_settings = self.buffering_settings.borrow().clone();
        let skills = self.cfg.borrow().skills.clone();
        let acp_agent_profile = parse_agent_profile_from_meta(session_meta);
        let mut agent_definition = {
            let cfg = self.cfg.borrow();
            Self::resolve_agent_definition(
                cwd.as_path(),
                cfg.agent_profile_path.as_deref(),
                &cfg.agent,
                acp_agent_profile,
                persisted_agent_name,
            )
        };
        {
            let cfg = self.cfg.borrow();
            let overrides = &cfg.cli_agent_overrides;
            overrides.apply_to_definition(&mut agent_definition);
            if overrides.has_definition_overrides() {
                tracing::debug!(
                    agent = %agent_definition.name,
                    tools = ?overrides.tools,
                    disallowed = ?overrides.disallowed_tools,
                    "cli agent overrides applied"
                );
            }
        }
        let max_turns = {
            let cfg = self.cfg.borrow();
            cfg.cli_agent_overrides
                .max_turns
                .or(agent_definition.max_turns)
                .map(|v| v as usize)
        };
        {
            let cfg = self.cfg.borrow();
            let effective = cfg
                .toolset
                .resolve_file_toolset(cfg.remote_settings.as_ref());
            if effective != crate::tools::FileToolset::Standard {
                let file_tools = effective
                    .tool_configs(&cfg.toolset.hashline)
                    .map_err(|e| {
                        acp::Error::invalid_params()
                            .data(format!("invalid [toolset.hashline] config: {e}"))
                    })?;
                agent_definition.override_file_tools(file_tools);
            }
        }
        let lsp_tools_enabled = self.cfg.borrow().resolve_lsp_tools().value;
        if lsp_tools_enabled && tool_ctx.lsp.is_none() {
            let snapshot = self.plugin_registry_handle.snapshot();
            let active: Vec<_> = snapshot
                .iter()
                .flat_map(|reg| reg.active_plugins())
                .collect();
            let (plugin_lsp_paths, plugin_names): (Vec<std::path::PathBuf>, Vec<&str>) = active
                .iter()
                .filter_map(|p| {
                    p.lsp_config_path.clone().map(|path| (path, p.name.as_str()))
                })
                .unzip();
            let (
                plugin_inline_lsp,
                inline_names,
            ): (Vec<&serde_json::Value>, Vec<&str>) = active
                .iter()
                .filter_map(|p| {
                    p.inline_lsp_servers.as_ref().map(|v| (v, p.name.as_str()))
                })
                .unzip();
            let sourced = tools::implementations::lsp::config::load_servers_with_plugins_sourced(
                tool_ctx.cwd.as_path(),
                &plugin_lsp_paths,
                &plugin_inline_lsp,
                &plugin_names,
                &inline_names,
            );
            let servers = folder_trust::filter_untrusted_project_lsp(
                tool_ctx.cwd.as_path(),
                sourced,
            );
            tool_ctx.lsp_server_names = servers.keys().cloned().collect();
            if servers.is_empty() {
                let user_path = tools::util::grow_home::grow_home()
                    .join("lsp.json");
                let project_path = tool_ctx.cwd.as_path().join(".grow").join("lsp.json");
                tracing::debug!(
                    cwd = %tool_ctx.cwd,
                    user_lsp_path = %user_path.display(),
                    project_lsp_path = %project_path.display(),
                    "LSP tools enabled, but no language servers are configured"
                );
            } else {
                use tools::implementations::lsp::{
                    LspBackend, LspBackendAdapter, LspManager,
                };
                let mgr = std::sync::Arc::new(
                    tokio::sync::Mutex::new(
                        LspManager::new(
                                servers,
                                tool_ctx.cwd.as_path().to_path_buf(),
                                true,
                                tools::notification::ToolNotificationHandle::noop(),
                            )
                            .with_process_scope(tool_ctx.process_scope.clone()),
                    ),
                );
                let adapter = std::sync::Arc::new(LspBackendAdapter::new(mgr));
                adapter.ensure_started_background();
                tool_ctx.lsp = Some(adapter as std::sync::Arc<dyn LspBackend>);
            }
        }
        let inference_idle_timeout_secs = {
            let models = self.models_manager.models();
            let cfg = self.cfg.borrow();
            resolve_inference_idle_timeout_secs(
                &models,
                &sampling_config.model,
                cfg.remote_settings.as_ref(),
            )
        };
        let model_max_retries = self
            .models_manager
            .models()
            .values()
            .find(|entry| entry.info.model == sampling_config.model)
            .and_then(|entry| entry.info.max_retries);
        let origin_client = self.origin_client_info_from_meta(init.meta.as_ref());
        let app_builder_deployer_config = self.prepare_app_builder_deployer_config();
        let web_fetch_config = self.prepare_web_fetch_config();
        let write_file_enabled = self.cfg.borrow().resolve_write_file().value;
        let goal_enabled = self.cfg.borrow().resolve_goal().value;
        let background_workflows_enabled = self.cfg.borrow().resolve_workflows().value;
        let subagents_enabled = self.cfg.borrow().subagents_enabled;
        let subagents_max_depth = self.cfg.borrow().subagents_max_depth;
        let subagent_classifier_input = self.cfg.borrow().subagent_classifier_input;
        let ask_user_question_enabled = parse_ask_user_question_from_meta(session_meta)
            .unwrap_or_else(|| self.cfg.borrow().resolve_ask_user_question().value);
        let client_hooks = crate::extensions::hooks::parse_client_hooks(session_meta);
        let todo_gate = self.cfg.borrow().todo_gate;
        let remote_settings_for_spawn = self.cfg.borrow().remote_settings.clone();
        let laziness_debug_log_for_spawn = self.cfg.borrow().laziness_debug_log.clone();
        let respect_gitignore = self.cfg.borrow().respect_gitignore;
        let path_not_found_hints = self.cfg.borrow().path_not_found_hints;
        let subagent_toggle = self.cfg.borrow().subagent_toggle.clone();
        let handle_display_cwd = prompt_display_cwd.clone();
        let bash_params_json = {
            let cfg = self.cfg.borrow();
            let remote_auto_bg = cfg
                .remote_settings
                .as_ref()
                .and_then(|r| r.auto_background_on_timeout);
            let remote_allow_background_operator = cfg
                .remote_settings
                .as_ref()
                .and_then(|r| r.allow_background_operator);
            cfg.toolset
                .bash
                .to_bash_params_json(remote_auto_bg, remote_allow_background_operator)
        };
        let ask_user_question_params_json = {
            let cfg = self.cfg.borrow();
            let params = crate::util::config::resolve_ask_user_question_params_from_disk(
                cfg.remote_settings.as_ref(),
            );
            match serde_json::to_value(params) {
                Ok(serde_json::Value::Object(map)) => Some(map),
                _ => None,
            }
        };
        let tool_params_json = crate::session::agent_rebuild::ResolvedToolParamsJson {
            bash: Some(bash_params_json),
            ask_user_question: ask_user_question_params_json,
        };
        let is_new_session = timeline_bootstrap.is_fresh();
        let init_meta = self
            .initialize_request
            .get()
            .and_then(|init| init.meta.as_ref());
        let (mut handle, agent_system_prompt, session_thread) = {
            let _timer = crate::instrumentation_timer!("session.spawn_actor_call");
            let credentials = chat_state::Credentials {
                api_key: sampling_config.api_key.clone(),
                alpha_test_key: self.alpha_test_key(),
            };
            let agent_hook_registry_override = agent_definition
                .hooks
                .as_ref()
                .and_then(|hooks_config| {
                    let hooks_val = hooks_config.as_value();
                    let (specs, errors) = ::hooks::config::parse_hooks_from_value_with_dir(
                        &hooks_val,
                        &format!(
                        "{}{}",
                        ::hooks::config::AGENT_HOOK_PREFIX,
                        agent_definition.name
                    ),
                        std::path::Path::new(&session_info.cwd),
                    );
                    for e in &errors {
                        tracing::warn!(agent = %agent_definition.name, error = ?e, "agent hook parse error");
                    }
                    if specs.is_empty() {
                        return None;
                    }
                    let cwd = std::path::Path::new(&session_info.cwd);
                    let hooks_trusted = folder_trust::project_scope_allowed(cwd);
                    let git_root = workspace::session::git::find_git_root_from_path(
                            cwd,
                        )
                        .ok();
                    let (disk_registry, disk_errors) = crate::util::hooks::discover_hooks(
                        git_root.as_deref(),
                        hooks_trusted,
                    );
                    for e in &disk_errors {
                        tracing::warn!(error = ?e, "hook loading error");
                    }
                    let mut merged = disk_registry;
                    if folder_trust::agent_inline_hooks_allowed(
                        agent_definition.scope,
                        || hooks_trusted,
                    ) {
                        merged.append_specs(specs);
                    }
                    Some(std::sync::Arc::new(merged))
                });
            let initial_reasoning_effort =
                is_new_session.then_some(sampling_config.reasoning_effort);
            let _ = persistence
                .tx
                .send(crate::session::persistence::PersistenceMsg::CurrentModel {
                    model_id: session_model_id.clone(),
                    agent_name: Some(agent_definition.name.clone()),
                    reasoning_effort: initial_reasoning_effort,
                });
            let acp_mcp_servers = crate::session::acp_mcp::parse_acp_mcp_servers(
                session_meta,
            );
            let git_head_changed = init
                .client_capabilities
                .meta
                .as_ref()
                .and_then(|m| m.get("grow/gitHeadChanged"))
                .and_then(|v| v.as_bool());
            let session_cwd = std::path::Path::new(&session_info.cwd);
            let fs_watch_caps = crate::session::fs_watch::FsWatchCapabilities::resolve(crate::session::fs_watch::CapabilityInputs {
                client_notify: fs_notify_config.is_some(),
                hunk_tracking: hunk_plan.enabled(),
                code_nav: client_code_nav_enabled,
                git_head_changed,
            });
            spawn_session_on_thread(
                    session_info.clone(),
                    crate::session::persistence::session_dir(&session_info),
                    self.gateway.clone(),
                    sampling_config,
                    credentials,
                    auth_method_id,
                    tool_ctx,
                    mcp_servers,
                    initial_client_mcp_servers,
                    mcp_meta_config_map,
                    None,
                    acp_mcp_servers,
                    support_permission,
                    auto_update,
                    persistence,
                    session_title_route,
                    timeline_bootstrap,
                    rewind_points_file_path,
                    fs_notify_config,
                    startup_hints,
                    client_type,
                    permission_prompt_timeout,
                    auto_compact_threshold_percent,
                    system_prompt_label,
                    compaction_verbatim_input,
                    compaction_tool_choice,
                    compaction_pre_prune,
                    compaction_pre_prune_token_budget,
                    buffering_settings,
                    origin_client.clone(),
                    self.codebase_indexes.clone(),
                    client_code_nav_enabled,
                    fs_watch_caps,
                    client_terminal,
                    client_fs_read && client_fs_write,
                    gateway_enabled,
                    agent_definition,
                    skills,
                    None,
                    incremental_bash_output,
                    persisted_signals,
                    persisted_behavior,
                    persisted_goal_mode,
                    persisted_control_revision,
                    persisted_workflow_runs,
                    persisted_announcement_state,
                    self.memory_config.clone(),
                    session_model_id,
                    session_permission_mode,
                    origin_client.as_ref().map(|o| o.product.clone()),
                    inference_idle_timeout_secs,
                    model_max_retries,
                    web_fetch_config,
                    app_builder_deployer_config,
                    write_file_enabled,
                    goal_enabled,
                    background_workflows_enabled,
                    subagents_enabled,
                    subagents_max_depth,
                    subagent_classifier_input,
                    ask_user_question_enabled,
                    client_hooks,
                    prompt_display_cwd,
                    subagent_toggle,
                    agent::prompt::context::PromptAudience::Primary,
                    respect_gitignore,
                    path_not_found_hints,
                    tool_params_json,
                    {
                        let disk_cfg = crate::config::resolve_effective_plugins_config(
                                session_cwd,
                            )
                            .to_discovery_config();
                        self.plugin_registry_handle
                            .refresh_and_build_for_cwd(
                                session_cwd,
                                &disk_cfg,
                                &parse_session_plugin_dirs(session_meta),
                                folder_trust::project_scope_allowed(session_cwd),
                            )
                    },
                    Some(self.plugin_registry_handle.clone()),
                    self.models_manager.clone(),
                    None,
                    None,
                    self.resolve_image_description_model(),
                    agent_hook_registry_override,
                    workspace_ops.clone(),
                    {
                        let cfg = self.cfg.borrow();
                        cfg.cli_agent_overrides.permission_rules.clone()
                    },
                    todo_gate,
                    remote_settings_for_spawn,
                    laziness_debug_log_for_spawn,
                    None,
                    None,
                    max_turns,
                )
                .await?
        };
        self.session_threads
            .borrow_mut()
            .insert(session_info.id.clone(), session_thread);
        tracing::debug!(session_id = %session_info.id.0, "spawn_session_on_thread complete");
        self.set_session_live_state(&session_info.id, SessionLiveState::IdleResident);
        self.ensure_session_supervisor();
        self.push_roster_delta_upserted(&session_info.id);
        if is_new_session {
            let _timer = crate::instrumentation_timer!("session.system_prompt_inject");
            let system_prompt = build_spawn_system_prompt(
                session_meta,
                init_meta,
                &agent_system_prompt,
            );
            tracing::debug!(
                session_id = %session_info.id.0,
                "built system prompt"
            );
            let _ = handle
                .cmd_tx
                .send(SessionCommand::Initialize {
                    system_prompt,
                });
            tracing::debug!(session_id = %session_info.id.0, "enqueued SessionCommand::Initialize");
        }
        let _ = handle.cmd_tx.send(SessionCommand::AdvertiseCommands);
        if handle_display_cwd.is_some() {
            handle.display_cwd = handle_display_cwd;
        }
        let source = if is_new_session { "new" } else { "load" };
        let _ = handle
            .cmd_tx
            .send(SessionCommand::DispatchSessionStartHook {
                source: source.to_string(),
            });
        self.notify_session_cwd_for_watch(std::path::Path::new(&session_info.cwd));
        self.activity.register_session(&session_info.id.0, &handle);
        if let Some(old) = self
            .sessions
            .borrow_mut()
            .insert(session_info.id.clone(), handle)
            && let Some(scope) = &old.tool_context.process_scope
        {
            scope.kill_all();
        }
        Ok(())
    }
}
