//! [`WorkspaceHandle`] -- public handle to a workspace instance.
use fastrace::future::FutureExt as _;
use fastrace::local::LocalSpan;
use hunk_tracker::{HunkTrackerActor, HunkTrackerHandle, TrackingMode};
use prometheus::{IntCounterVec, register_int_counter_vec};
use std::path::PathBuf;
use std::sync::Arc;
/// Tripwire, expected 0 in production. `path="swap"`: a toolset swap found
/// the outgoing toolset's `Terminal` resource pointing at a backend other
/// than the session-owned one — a resolve path bypassed the session-owned
/// backend, and that backend's background tasks die with the old toolset.
/// Non-zero means background tasks were (or are about to be) killed by a
/// toolset swap: page the owning team. (`path="actor"` — actor-loop
/// channel-closure detection — is not emitted yet.)
pub(crate) static WORKSPACE_TERMINAL_BACKEND_ORPHANED_TOTAL: std::sync::LazyLock<IntCounterVec> =
    std::sync::LazyLock::new(|| {
        register_int_counter_vec!(
            "grow_workspace_terminal_backend_orphaned_total",
            "Terminal backends detected orphaned from their session, by detection path \
             (tripwire, expected 0)",
            &["path"]
        )
        .unwrap()
    });
use crate::config::{DEFAULT_EVENT_BUFFER_CAPACITY, HookSourceConfig, WorkspaceConfig};
use crate::error::{WorkspaceError, WorkspaceResult};
use crate::session::swap_policy::{
    DeferReason, SessionSnapshot, SwapAction, SwapDecision, SwapPolicy, record_swap_decision,
    record_toolset_swap,
};
use crate::session::tool_config::resolve_session_toolset;
use crate::session::{WorkspaceSession, WorkspaceShared};
use crate::workspace_ops::{
    GetFileEntry, GetFileResult, GetFilesRes, PutFileEntry, PutFileResult, PutFilesRes,
};

/// Zero-init this module's metric families. See [`crate::init_metrics`].
pub(crate) fn init_metrics() {
    WORKSPACE_TERMINAL_BACKEND_ORPHANED_TOTAL
        .with_label_values(&["swap"])
        .inc_by(0);
}
/// What [`WorkspaceHandle::resolve_and_swap_session_toolset`] actually did —
/// so no caller can mistake a deliberate skip for an installed swap (the
/// skip leaves toolset AND fingerprint untouched).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a skip means the config was NOT applied; callers must not report success"]
pub(crate) enum SwapOutcome {
    /// Toolset re-resolved and installed; fingerprint updated.
    Swapped,
    /// Identical fingerprint ([`SwapDecision::Reuse`]): the live toolset
    /// already reflects the config, nothing resolved or changed.
    Reused,
    /// Externally-owned (local-bind) toolset: rebuild skipped, nothing
    /// changed. See `toolset_terminal_is_session_owned`.
    SkippedExternallyOwned,
}
/// Public handle to a workspace instance. Owns shared state (sessions, tool
/// configuration, event bus) and session lifecycle.
#[derive(Clone)]
pub struct WorkspaceHandle {
    pub(crate) shared: Arc<WorkspaceShared>,
}
impl WorkspaceHandle {
    /// Construct a handle with zero sessions.
    ///
    /// Sessions are created explicitly via [`Self::create_session`] or
    /// session creation. There is no implicit "main" session —
    /// callers create their first
    /// session after construction.
    ///
    /// # Panics
    /// Requires a Tokio runtime to be entered (for broadcast channel).
    pub fn new(config: WorkspaceConfig) -> WorkspaceResult<Self> {
        Self::build(config, ephemeral_workspace_home())
    }
    pub(crate) fn build(
        config: WorkspaceConfig,
        workspace_home: std::path::PathBuf,
    ) -> WorkspaceResult<Self> {
        let sessions = std::collections::HashMap::new();
        let local_registry = tool_runtime::LocalRegistry::new();
        let capacity = if config.event_buffer_capacity == 0 {
            DEFAULT_EVENT_BUFFER_CAPACITY
        } else {
            config.event_buffer_capacity
        };
        let (events, _drop_rx) = tokio::sync::broadcast::channel(capacity);
        let (hook_registry, hook_load_errors) = {
            use hooks::discovery::{HookSource, load_hooks_from_sources};
            fn to_hook_source(s: &HookSourceConfig) -> HookSource<'_> {
                match s {
                    HookSourceConfig::HookFile(p) => HookSource::HookFile(p.as_path()),
                    HookSourceConfig::Directory(p) => HookSource::Directory(p.as_path()),
                }
            }
            let global_refs: Vec<HookSource<'_>> = config
                .hook_global_sources
                .iter()
                .map(to_hook_source)
                .collect();
            let project_refs: Vec<HookSource<'_>> = config
                .hook_project_sources
                .iter()
                .map(to_hook_source)
                .collect();
            let (registry, errors) = load_hooks_from_sources(&global_refs, &project_refs);
            for err in &errors {
                tracing::warn!(error = %err, "hook discovery error (non-fatal)");
            }
            tracing::info!(
                hook_count = registry.len(),
                error_count = errors.len(),
                "hook discovery complete"
            );
            (registry, errors)
        };
        let lsp: Option<Arc<dyn tools::implementations::lsp::LspBackend>> = {
            let sourced = tools::implementations::lsp::config::load_servers_with_plugins_sourced(
                &config.root_cwd,
                &[],
                &[],
                &[],
                &[],
            );
            let servers = tools::implementations::lsp::config::filter_project_lsp_when_untrusted(
                sourced,
                config.project_lsp_trusted,
            );
            if servers.is_empty() {
                None
            } else {
                use tools::implementations::lsp::{LspBackend, LspBackendAdapter, LspManager};
                let mgr = Arc::new(tokio::sync::Mutex::new(LspManager::new(
                    servers,
                    config.root_cwd.clone(),
                    true,
                    tools::notification::ToolNotificationHandle::noop(),
                )));
                let adapter = Arc::new(LspBackendAdapter::new(mgr));
                adapter.ensure_started_background();
                Some(adapter)
            }
        };
        let activity_tracker = Arc::new(
            crate::activity::ActivityTracker::with_prune_window(
                config.status_config.session_idle_prune,
            )
            .with_idle_ignores_background(config.status_config.idle_ignores_background),
        );
        let shared = WorkspaceShared {
            default_tool_config: config.default_tool_config,
            root_cwd: config.root_cwd.clone(),
            sessions: parking_lot::RwLock::new(sessions),
            session_factory: config.session_factory,
            events,
            respect_gitignore: config.respect_gitignore,
            memory_config: config.memory_config,
            hook_registry: Arc::new(parking_lot::RwLock::new(hook_registry)),
            hook_load_errors,
            skills_config: config.skills_config,
            plugin_discovery_config: config.plugin_discovery_config,
            client_ext_sink: arc_swap::ArcSwap::new(Arc::new(None)),
            local_registry,
            activity_tracker,
            status_config: config.status_config,
            fuzzy_searches: Arc::new(tokio::sync::Mutex::new(
                crate::file_system::FuzzySearchManager::new(std::time::Duration::from_secs(300)),
            )),
            lsp,
            codebase_indexes: Arc::new(parking_lot::Mutex::new(
                crate::file_system::CodebaseIndexManager::new(),
            )),
            workspace_home,
            #[cfg(test)]
            post_resolve_test_hook: parking_lot::Mutex::new(None),
            client_fs_hash_memo: Default::default(),
        };
        Ok(Self {
            shared: Arc::new(shared),
        })
    }
    #[allow(dead_code)]
    pub fn shared(&self) -> &Arc<WorkspaceShared> {
        &self.shared
    }
    pub fn activity_tracker(&self) -> &std::sync::Arc<crate::activity::ActivityTracker> {
        &self.shared.activity_tracker
    }
    /// Get the workspace root directory.
    pub(crate) fn root_cwd(&self) -> crate::error::WorkspaceResult<PathBuf> {
        Ok(self.shared.root_cwd.clone())
    }
    /// Create a new top-level session from the workspace's default config.
    ///
    /// This creates a fresh session with the workspace's `root_cwd`. Both the
    /// TUI and server use this as the
    /// primary session creation path.
    ///
    /// Returns the newly created session, or an error if a session with
    /// the given ID already exists.
    pub fn create_session(
        &self,
        session_id: impl Into<String>,
    ) -> WorkspaceResult<Arc<WorkspaceSession>> {
        self.create_session_with_cwd(session_id, None)
    }
    /// Create a session with an optional CWD override, using the workspace
    /// default toolset.
    pub fn create_session_with_cwd(
        &self,
        session_id: impl Into<String>,
        cwd: Option<std::path::PathBuf>,
    ) -> WorkspaceResult<Arc<WorkspaceSession>> {
        self.create_session_with_config(session_id, cwd, None, None, false)
    }
    /// Create a session with an optional CWD override, per-session toolset, and
    /// `tool_config: None` uses the default.
    pub fn create_session_with_config(
        &self,
        session_id: impl Into<String>,
        cwd: Option<std::path::PathBuf>,
        tool_config: Option<tools::registry::types::ToolServerConfig>,
        viewer_ctx: Option<tool_runtime::WorkspaceViewerContext>,
        system_notifications: bool,
    ) -> WorkspaceResult<Arc<WorkspaceSession>> {
        let session_id = session_id.into();
        let session_cwd = cwd.unwrap_or_else(|| self.shared.root_cwd.clone());
        let (hunk_event_tx, _hunk_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let hunk_cancel = tokio_util::sync::CancellationToken::new();
        let hunk_tracker = HunkTrackerActor::spawn(
            session_id.clone(),
            session_cwd.clone(),
            hunk_event_tx,
            TrackingMode::AllDirty,
            hunk_cancel.clone(),
        );
        let result = self.create_session_with_tracker_inner(
            session_id,
            session_cwd,
            hunk_tracker,
            Some(hunk_cancel.clone()),
            tool_config,
            viewer_ctx,
            system_notifications,
        );
        if result.is_err() {
            hunk_cancel.cancel();
        }
        result
    }
    /// Create a session that reuses an existing hunk tracker (already rooted at
    /// `cwd`) instead of spawning a new one, so the workspace session and the
    /// agent share a single per-session tracker. `tool_config: None` uses the default.
    pub fn create_session_with_tracker(
        &self,
        session_id: impl Into<String>,
        cwd: std::path::PathBuf,
        hunk_tracker: HunkTrackerHandle,
        tool_config: Option<tools::registry::types::ToolServerConfig>,
    ) -> WorkspaceResult<Arc<WorkspaceSession>> {
        self.create_session_with_tracker_and_viewer_ctx(
            session_id,
            cwd,
            hunk_tracker,
            tool_config,
            None,
            false,
        )
    }
    /// Variant of [`create_session_with_tracker`](Self::create_session_with_tracker)
    /// that carries a session-bind viewer context. The tracker is externally
    /// owned, so the session stores no cancel token for it.
    pub fn create_session_with_tracker_and_viewer_ctx(
        &self,
        session_id: impl Into<String>,
        cwd: std::path::PathBuf,
        hunk_tracker: HunkTrackerHandle,
        tool_config: Option<tools::registry::types::ToolServerConfig>,
        viewer_ctx: Option<tool_runtime::WorkspaceViewerContext>,
        system_notifications: bool,
    ) -> WorkspaceResult<Arc<WorkspaceSession>> {
        self.create_session_with_tracker_inner(
            session_id,
            cwd,
            hunk_tracker,
            None,
            tool_config,
            viewer_ctx,
            system_notifications,
        )
    }
    /// Shared creation body. `hunk_tracker_cancel` is `Some` only for
    /// workspace-spawned trackers, whose actor lifetime the session then
    /// owns; externally owned trackers pass `None`.
    #[allow(clippy::too_many_arguments)]
    fn create_session_with_tracker_inner(
        &self,
        session_id: impl Into<String>,
        cwd: std::path::PathBuf,
        hunk_tracker: HunkTrackerHandle,
        hunk_tracker_cancel: Option<tokio_util::sync::CancellationToken>,
        tool_config: Option<tools::registry::types::ToolServerConfig>,
        viewer_ctx: Option<tool_runtime::WorkspaceViewerContext>,
        system_notifications: bool,
    ) -> WorkspaceResult<Arc<WorkspaceSession>> {
        let session_id = session_id.into();
        if session_id.is_empty() {
            return Err(WorkspaceError::EmptyAgentId);
        }
        let mut sessions = self.shared.sessions.write();
        if sessions.contains_key(&session_id) {
            return Err(WorkspaceError::SessionAlreadyExists(session_id));
        }
        let session_env = Arc::new(std::collections::HashMap::new());
        let config = tool_config.unwrap_or_else(|| self.shared.default_tool_config.clone());
        let system_notify_channel =
            system_notifications.then(tools::notification::types::ToolNotificationHandle::channel);
        let system_notify_handle = system_notify_channel.as_ref().map(|(h, _)| h.clone());
        let (effective, toolset, terminal_backend) = {
            let _span = LocalSpan::enter_with_local_parent("tool_server.toolset_resolve")
                .with_property(|| ("session_id", session_id.clone()));
            resolve_session_toolset(
                config,
                cwd.clone(),
                session_env.clone(),
                &session_id,
                self.shared.session_factory.as_ref(),
                Some(self.shared.local_registry.clone()),
                self.shared.lsp.clone(),
                viewer_ctx.clone(),
                system_notify_handle,
            )
        }?;
        let session = Arc::new(WorkspaceSession::new(
            session_id.clone(),
            cwd,
            session_env,
            Arc::new(effective),
            toolset,
            terminal_backend,
            hunk_tracker,
            hunk_tracker_cancel,
            viewer_ctx,
            system_notifications,
            system_notify_channel,
        ));
        tracing::info!(session_id = %session_id, "create_session: new session created");
        sessions.insert(session_id, session.clone());
        record_toolset_swap(
            &self.shared.activity_tracker,
            "create",
            session.session_id(),
        );
        Ok(session)
    }
    /// Update a session's tool config after checking caller ownership.
    /// Swap gating (retryable `TurnActive`, stale heal): [`SwapPolicy::evaluate`].
    pub(crate) async fn update_tool_config(
        &self,
        caller_session_id: &str,
        session_id: &str,
        new_config: tools::registry::types::ToolServerConfig,
    ) -> crate::error::WorkspaceResult<()> {
        let session = self
            .session(session_id)
            .ok_or_else(|| crate::error::WorkspaceError::SessionNotFound(session_id.to_owned()))?;
        if caller_session_id != session_id {
            return Err(crate::error::WorkspaceError::Unauthorized {
                caller: caller_session_id.to_owned(),
                target: session_id.to_owned(),
            });
        }
        match self
            .resolve_and_swap_session_toolset(&session, new_config)
            .await?
        {
            SwapOutcome::Swapped | SwapOutcome::Reused => Ok(()),
            SwapOutcome::SkippedExternallyOwned => Err(
                crate::error::WorkspaceError::ToolsetExternallyOwned(session_id.to_owned()),
            ),
        }
    }
    /// Re-resolve `new_config` against the session's frozen bind-time inputs
    /// and atomically swap its toolset (`ToolsChanged`). Update-RPC entry:
    /// gated by [`SwapPolicy::evaluate`], twice (entry + post-resolve).
    pub(crate) async fn resolve_and_swap_session_toolset(
        &self,
        session: &Arc<crate::session::WorkspaceSession>,
        new_config: tools::registry::types::ToolServerConfig,
    ) -> crate::error::WorkspaceResult<SwapOutcome> {
        let _update_guard = session.update_lock.lock().await;
        let session_id = session.session_id();
        let new_fingerprint = serde_json::to_value(&new_config).ok();
        let snapshot = SessionSnapshot::capture(
            session,
            &self.shared.activity_tracker,
            new_fingerprint.as_ref(),
        )
        .await;
        match SwapPolicy::evaluate(&snapshot) {
            SwapDecision::Reuse => {
                tracing::debug!(
                    session_id = %session_id,
                    "toolset config identical to the stored bind fingerprint — \
                     reused untouched"
                );
                Ok(SwapOutcome::Reused)
            }
            SwapDecision::Skip(reason) => {
                record_swap_decision(
                    &self.shared.activity_tracker,
                    session_id,
                    SwapAction::Skipped(reason),
                );
                tracing::warn!(
                    session_id = %session_id,
                    "toolset swap skipped: toolset terminal backend is externally \
                     owned (local bind)"
                );
                Ok(SwapOutcome::SkippedExternallyOwned)
            }
            SwapDecision::Defer(reason) => {
                record_swap_decision(
                    &self.shared.activity_tracker,
                    session_id,
                    SwapAction::Deferred(reason),
                );
                tracing::info!(
                    session_id = %session_id,
                    "toolset mutation rejected: turn active — retry at the turn boundary"
                );
                Err(crate::error::WorkspaceError::TurnActive(
                    session_id.to_owned(),
                ))
            }
            SwapDecision::Apply => {
                self.resolve_and_swap_session_toolset_locked(session, new_config, new_fingerprint)
                    .await
            }
        }
    }
    /// The [`SwapDecision::Apply`] arm: resolve `new_config` (whose
    /// fingerprint `new_fingerprint` must be) and install it. Callers hold
    /// `update_lock` and evaluated [`SwapPolicy`] to `Apply` under that hold.
    async fn resolve_and_swap_session_toolset_locked(
        &self,
        session: &Arc<crate::session::WorkspaceSession>,
        new_config: tools::registry::types::ToolServerConfig,
        new_fingerprint: Option<serde_json::Value>,
    ) -> crate::error::WorkspaceResult<SwapOutcome> {
        let session_id = session.session_id().to_owned();
        let cwd = session.cwd().to_path_buf();
        let session_env = session.session_env().clone();
        let factory = self.shared.session_factory.clone();
        let lr = self.shared.local_registry.clone();
        let lsp = self.shared.lsp.clone();
        let sid = session_id.to_owned();
        let viewer_ctx = session.viewer_ctx().cloned();
        let notification_handle = session.system_notify_handle();
        let terminal_backend = session.terminal_backend().clone();
        let resolve_result = tokio::task::spawn_blocking(move || {
            crate::session::tool_config::resolve_session_toolset_rebuild(
                new_config,
                cwd,
                session_env,
                &sid,
                factory.as_ref(),
                Some(lr),
                lsp,
                viewer_ctx,
                notification_handle,
                terminal_backend,
            )
        })
        .await
        .map_err(|e| crate::error::WorkspaceError::JoinError(e.to_string()))?;
        let (effective, new_toolset) = resolve_result?;
        #[cfg(test)]
        if let Some(hook) = self.shared.post_resolve_test_hook.lock().as_ref() {
            hook();
        }
        let snapshot = SessionSnapshot::capture(
            session,
            &self.shared.activity_tracker,
            new_fingerprint.as_ref(),
        )
        .await;
        match SwapPolicy::evaluate(&snapshot) {
            SwapDecision::Apply => {}
            SwapDecision::Reuse => {
                tracing::debug!(
                    session_id = %session_id,
                    "resolved toolset discarded post-resolve: a concurrent \
                     bind installed the identical fingerprint during the \
                     re-resolve"
                );
                return Ok(SwapOutcome::Reused);
            }
            SwapDecision::Skip(reason) => {
                record_swap_decision(
                    &self.shared.activity_tracker,
                    &session_id,
                    SwapAction::Skipped(reason),
                );
                tracing::warn!(
                    session_id = %session_id,
                    "toolset swap skipped: toolset terminal backend is externally \
                     owned (local bind)"
                );
                return Ok(SwapOutcome::SkippedExternallyOwned);
            }
            SwapDecision::Defer(reason) => {
                let reason = match reason {
                    DeferReason::TurnActive => DeferReason::TurnActiveLate,
                    other => other,
                };
                record_swap_decision(
                    &self.shared.activity_tracker,
                    &session_id,
                    SwapAction::Deferred(reason),
                );
                tracing::info!(
                    session_id = %session_id,
                    "toolset mutation rejected post-resolve: a turn started during \
                     the re-resolve — resolved toolset discarded; retry at the \
                     turn boundary"
                );
                return Err(crate::error::WorkspaceError::TurnActive(session_id));
            }
        }
        session
            .replace_carrying_browser_service(Arc::new(effective), new_toolset)
            .await;
        session.set_tool_config_fingerprint(new_fingerprint);
        session.clear_stale_resolve();
        record_swap_decision(
            &self.shared.activity_tracker,
            &session_id,
            SwapAction::Applied,
        );
        let _ = self
            .shared
            .events
            .send(workspace_types::WorkspaceEvent::ToolsChanged {
                session_id: session_id.to_owned(),
            });
        Ok(SwapOutcome::Swapped)
    }
    pub async fn on_before_turn(
        &self,
        session_id: &str,
        payload: &tool_protocol::turn_hook::BeforeTurnPayload,
    ) {
        self.shared
            .activity_tracker
            .turn_started(session_id, payload.turn_number);
        tracing::debug!(
            session = %session_id,
            turn = payload.turn_number,
            model = %payload.model_id,
            "workspace: before_turn processed"
        );
    }
    /// Fire-and-forget `after_turn` hook path for in-process clients.
    pub async fn on_after_turn(
        &self,
        session_id: &str,
        payload: &tool_protocol::turn_hook::AfterTurnPayload,
    ) {
        let _ = self.process_after_turn(session_id, payload).await;
    }
    async fn process_after_turn(
        &self,
        session_id: &str,
        payload: &tool_protocol::turn_hook::AfterTurnPayload,
    ) {
        self.shared.activity_tracker.turn_completed(
            session_id,
            payload.turn_number,
            payload.duration_ms,
        );
        tracing::debug!(
            session = %session_id,
            turn = payload.turn_number,
            outcome = ?payload.outcome,
            "workspace: after_turn processed"
        );
    }
    /// Answer a request/response `turn_hook` (sampler/shell → workspace).
    ///
    /// Both phases run the same turn-boundary work as their fire-and-forget
    /// hook counterparts (the server-side sampler signals turns ONLY through
    /// this request channel): `Before` drives [`Self::on_before_turn`]
    /// and answers with a no-op reply
    /// (injections are not computed yet); `After` runs the turn-end work.
    ///
    /// Each phase must be signalled through exactly ONE channel per client —
    /// fire-and-forget hook or request — otherwise its work runs twice.
    pub async fn compute_turn_injections(
        &self,
        session_id: &str,
        request: &tool_protocol::turn_hook::TurnHookRequest,
    ) -> tool_protocol::turn_hook::HookReply {
        use tool_protocol::turn_hook::{HookReply, TurnHookRequest};
        match request {
            TurnHookRequest::Before(payload) => {
                self.on_before_turn(session_id, payload).await;
                HookReply::default()
            }
            TurnHookRequest::After(payload) => {
                self.process_after_turn(session_id, payload).await;
                HookReply::default()
            }
            _ => HookReply::default(),
        }
    }
    /// Bookkeeping for a cancelled in-flight tool call: marks it as
    /// completed in the activity tracker. Does **not** abort execution
    /// of the tool — that requires `CancellationToken` plumbing (future work).
    pub fn cancel_tool_call(&self, session_id: &str, call_id: &str) {
        self.shared
            .activity_tracker
            .tool_call_completed(call_id, Some(session_id));
        tracing::info!(%session_id, %call_id, "cancel_tool_call: marked as completed");
    }
    /// Cancel all in-flight tool calls for a session. Called when a
    /// session-wide Cancel hook arrives (no specific `call_id`).
    pub fn cancel_all_tool_calls(&self, session_id: &str) {
        let count = self
            .shared
            .activity_tracker
            .cancel_all_session_calls(session_id);
        tracing::info!(%session_id, count, "cancel_all_tool_calls: marked all as completed");
    }
    /// Clean up workspace state for a session that has ended.
    /// Does **not** drop the session — that is handled by the server's
    /// `unbind_session` lifecycle.
    pub fn on_session_ended(&self, session_id: &str) {
        self.shared.activity_tracker.session_ended(session_id);
        tracing::info!(%session_id, "session_ended cleanup completed");
    }
    /// Returns a cloned snapshot of the hook registry, disconnected
    /// from the workspace's live state.
    ///
    /// The registry is loaded once at workspace construction from the
    /// global and project sources in `WorkspaceConfig`; mid-session
    /// reloads (e.g. plugin hook appending) mutate the live registry
    /// in place via the `RwLock` on `WorkspaceShared`. The returned
    /// clone is not affected by subsequent mutations.
    pub fn hook_registry(&self) -> hooks::discovery::HookRegistry {
        self.shared.hook_registry.read().clone()
    }
    /// Non-fatal errors from the initial hook discovery pass at
    /// workspace construction time.
    ///
    /// Empty when all hook files parsed cleanly. Not updated on
    /// mid-session hook mutations (e.g. plugin hook appending).
    pub fn hook_load_errors(&self) -> &[hooks::error::HookError] {
        &self.shared.hook_load_errors
    }
    /// Canonicalize the workspace root directory.
    /// Called once per batch and passed to `resolve_service_path` for each file.
    pub(crate) async fn canonical_root(&self) -> WorkspaceResult<PathBuf> {
        Self::canonicalize_root_dir(&self.root_cwd()?).await
    }
    /// Canonicalize a confinement root directory.
    async fn canonicalize_root_dir(root: &std::path::Path) -> WorkspaceResult<PathBuf> {
        #[allow(clippy::disallowed_methods)]
        let canonical = tokio::fs::canonicalize(root).await.map_err(|e| {
            WorkspaceError::Operation(format!("failed to canonicalize workspace root: {e}"))
        })?;
        Ok(dunce::simplified(&canonical).to_path_buf())
    }
    /// Resolve a caller-provided path safely. Accepts a path relative to the
    /// workspace root, or an absolute path that resolves within the root;
    /// either form is confined to the root (paths that escape are rejected).
    /// Two-layer defense: textual normalization + symlink containment.
    ///
    /// # TOCTOU caveat
    /// The symlink check is point-in-time. If a symlink is created between
    /// resolution and I/O, containment is not guaranteed. Defense-in-depth
    /// (e.g., `O_NOFOLLOW`, mount namespaces) would be needed for hostile
    /// workspace environments, which is out of scope for this service-level API.
    pub(crate) async fn resolve_service_path(
        &self,
        req_path: &str,
        canonical_root: &std::path::Path,
    ) -> WorkspaceResult<PathBuf> {
        let root = self.root_cwd()?;
        Self::resolve_path_within_root(req_path, &root, canonical_root).await
    }
    /// Core of [`Self::resolve_service_path`], parameterized over the
    /// confinement root (see [`Self::confine_to_root`]).
    async fn resolve_path_within_root(
        req_path: &str,
        root: &std::path::Path,
        canonical_root: &std::path::Path,
    ) -> WorkspaceResult<PathBuf> {
        use std::path::{Component, Path};
        if req_path.is_empty() {
            return Err(WorkspaceError::Operation("empty path not allowed".into()));
        }
        let path = Path::new(req_path);
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        let mut components = Vec::new();
        for component in joined.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !components.is_empty()
                        && !matches!(components.last(), Some(Component::RootDir))
                    {
                        components.pop();
                    }
                }
                c => components.push(c),
            }
        }
        let normalized: PathBuf = components.into_iter().collect();
        if !normalized.starts_with(root) && !normalized.starts_with(canonical_root) {
            return Err(WorkspaceError::Operation(format!(
                "path escapes workspace root: {req_path}"
            )));
        }
        const MAX_SYMLINK_HOPS: usize = 40;
        let mut symlink_hops = 0usize;
        let mut check_path = normalized.clone();
        loop {
            #[allow(clippy::disallowed_methods)]
            match tokio::fs::canonicalize(&check_path).await {
                Ok(canonical) => {
                    let canonical = dunce::simplified(&canonical).to_path_buf();
                    if !canonical.starts_with(canonical_root) {
                        return Err(WorkspaceError::Operation(format!(
                            "path resolves outside workspace root (symlink escape): {req_path}"
                        )));
                    }
                    break;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::NotFound
                        || e.kind() == std::io::ErrorKind::NotADirectory =>
                {
                    if let Ok(md) = tokio::fs::symlink_metadata(&check_path).await
                        && md.file_type().is_symlink()
                    {
                        if symlink_hops >= MAX_SYMLINK_HOPS {
                            return Err(WorkspaceError::Operation(format!(
                                "path resolves outside workspace root (unresolved symlink chain): {req_path}"
                            )));
                        }
                        let Ok(target) = tokio::fs::read_link(&check_path).await else {
                            return Err(WorkspaceError::Operation(format!(
                                "failed to resolve symlink for containment: {req_path}"
                            )));
                        };
                        symlink_hops += 1;
                        check_path = if target.is_absolute() {
                            target
                        } else {
                            check_path
                                .parent()
                                .map(|p| p.join(&target))
                                .unwrap_or(target)
                        };
                        continue;
                    }
                    match check_path.parent() {
                        Some(parent) if parent != check_path => {
                            check_path = parent.to_path_buf();
                        }
                        _ => {
                            tracing::warn!(
                                "symlink containment: parent chain exhausted without canonicalize for {req_path}"
                            );
                            break;
                        }
                    }
                }
                Err(e) => {
                    return Err(WorkspaceError::Operation(format!(
                        "failed to verify path containment: {e}"
                    )));
                }
            }
        }
        Ok(normalized)
    }
    /// Resolve an already-local path for filesystem extension operations.
    /// Local sessions are not tenant boundaries, so absolute paths remain
    /// available to the agent and no walk root is imposed.
    pub async fn confine_to_workspace_root(
        &self,
        path: &std::path::Path,
    ) -> WorkspaceResult<(PathBuf, Option<PathBuf>)> {
        Ok((path.to_path_buf(), None))
    }
    /// Local equivalent of [`Self::confine_to_workspace_root`] for a session cwd.
    pub async fn confine_to_root(
        &self,
        path: &std::path::Path,
        _root: &std::path::Path,
    ) -> WorkspaceResult<(PathBuf, Option<PathBuf>)> {
        Ok((path.to_path_buf(), None))
    }
    /// Write files to the workspace filesystem (service-level, no hunk tracking).
    ///
    /// Files are written sequentially. If file N fails, files 1..N-1 are
    /// already on disk and will NOT be rolled back. Callers must inspect
    /// per-file results in the response to detect partial failures.
    pub async fn put_files(&self, files: Vec<PutFileEntry>) -> WorkspaceResult<PutFilesRes> {
        let canonical_root = self.canonical_root().await?;
        let mut results = Vec::with_capacity(files.len());
        for entry in files {
            let result = self.put_single_file(&entry, &canonical_root).await;
            results.push(result);
        }
        Ok(PutFilesRes { results })
    }
    async fn put_single_file(
        &self,
        entry: &PutFileEntry,
        canonical_root: &std::path::Path,
    ) -> PutFileResult {
        let resolved = match self.resolve_service_path(&entry.path, canonical_root).await {
            Ok(p) => p,
            Err(e) => {
                return PutFileResult {
                    path: entry.path.clone(),
                    ok: false,
                    error: Some(e.to_string()),
                    hash: None,
                };
            }
        };
        if entry.create_dirs
            && let Some(parent) = resolved.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return PutFileResult {
                path: entry.path.clone(),
                ok: false,
                error: Some(format!("failed to create directories: {e}")),
                hash: None,
            };
        }
        let write_result = if entry.append {
            use tokio::io::AsyncWriteExt;
            async {
                let mut f = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&resolved)
                    .await?;
                f.write_all(entry.content.as_bytes()).await?;
                f.flush().await
            }
            .await
        } else {
            tokio::fs::write(&resolved, entry.content.as_bytes()).await
        };
        match write_result {
            Ok(()) => {
                let hash = sha256_hex(entry.content.as_bytes());
                PutFileResult {
                    path: entry.path.clone(),
                    ok: true,
                    error: None,
                    hash: Some(hash),
                }
            }
            Err(e) => PutFileResult {
                path: entry.path.clone(),
                ok: false,
                error: Some(e.to_string()),
                hash: None,
            },
        }
    }
    /// Read files from the workspace filesystem with optional cache
    /// validation and byte-range support.
    ///
    /// Files are read sequentially. Each result includes:
    /// - `exists`: whether the file exists on disk.
    /// - `content`: file content (full or requested byte range as UTF-8).
    /// - `hash`: SHA-256 hex digest of the **full** file content.
    /// - `matched`: true if `if_none_match` matched the current hash.
    /// - `size`: total file size in bytes.
    pub async fn get_files(&self, files: Vec<GetFileEntry>) -> WorkspaceResult<GetFilesRes> {
        let canonical_root = self.canonical_root().await?;
        let mut results = Vec::with_capacity(files.len());
        for entry in files {
            let result = self.get_single_file(&entry, &canonical_root).await;
            results.push(result);
        }
        Ok(GetFilesRes { results })
    }
    async fn get_single_file(
        &self,
        entry: &GetFileEntry,
        canonical_root: &std::path::Path,
    ) -> GetFileResult {
        let resolved = match self.resolve_service_path(&entry.path, canonical_root).await {
            Ok(p) => p,
            Err(e) => {
                return GetFileResult {
                    path: entry.path.clone(),
                    exists: false,
                    content: None,
                    hash: None,
                    matched: false,
                    size: None,
                    error: Some(e.to_string()),
                };
            }
        };
        let is_chunked = entry.offset.is_some() || entry.length.is_some();
        let metadata = match tokio::fs::metadata(&resolved).await {
            Ok(m) => m,
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::NotADirectory =>
            {
                return GetFileResult {
                    path: entry.path.clone(),
                    exists: false,
                    content: None,
                    hash: None,
                    matched: false,
                    size: None,
                    error: None,
                };
            }
            Err(e) => {
                return GetFileResult {
                    path: entry.path.clone(),
                    exists: true,
                    content: None,
                    hash: None,
                    matched: false,
                    size: None,
                    error: Some(e.to_string()),
                };
            }
        };
        let file_size = metadata.len();
        if is_chunked {
            let req_offset = entry.offset.unwrap_or(0);
            let req_length = entry.length.unwrap_or(file_size.saturating_sub(req_offset));
            let read_result = stream_hash_and_range(&resolved, req_offset, req_length).await;
            match read_result {
                Ok((hash, chunk_bytes, _streamed)) => {
                    if let Some(ref etag) = entry.if_none_match
                        && *etag == hash
                    {
                        return GetFileResult {
                            path: entry.path.clone(),
                            exists: true,
                            content: None,
                            hash: Some(hash),
                            matched: true,
                            size: Some(file_size),
                            error: None,
                        };
                    }
                    match String::from_utf8(chunk_bytes) {
                        Ok(content) => GetFileResult {
                            path: entry.path.clone(),
                            exists: true,
                            content: Some(content),
                            hash: Some(hash),
                            matched: false,
                            size: Some(file_size),
                            error: None,
                        },
                        Err(e) => GetFileResult {
                            path: entry.path.clone(),
                            exists: true,
                            content: None,
                            hash: Some(hash),
                            matched: false,
                            size: Some(file_size),
                            error: Some(format!("not valid UTF-8 in range: {e}")),
                        },
                    }
                }
                Err(e) => GetFileResult {
                    path: entry.path.clone(),
                    exists: true,
                    content: None,
                    hash: None,
                    matched: false,
                    size: Some(file_size),
                    error: Some(e.to_string()),
                },
            }
        } else {
            match tokio::fs::read(&resolved).await {
                Ok(bytes) => {
                    let hash = sha256_hex(&bytes);
                    if let Some(ref etag) = entry.if_none_match
                        && *etag == hash
                    {
                        return GetFileResult {
                            path: entry.path.clone(),
                            exists: true,
                            content: None,
                            hash: Some(hash),
                            matched: true,
                            size: Some(file_size),
                            error: None,
                        };
                    }
                    match String::from_utf8(bytes) {
                        Ok(content) => GetFileResult {
                            path: entry.path.clone(),
                            exists: true,
                            content: Some(content),
                            hash: Some(hash),
                            matched: false,
                            size: Some(file_size),
                            error: None,
                        },
                        Err(e) => GetFileResult {
                            path: entry.path.clone(),
                            exists: true,
                            content: None,
                            hash: Some(hash),
                            matched: false,
                            size: Some(file_size),
                            error: Some(format!("not valid UTF-8: {e}")),
                        },
                    }
                }
                Err(e) => GetFileResult {
                    path: entry.path.clone(),
                    exists: true,
                    content: None,
                    hash: None,
                    matched: false,
                    size: Some(file_size),
                    error: Some(e.to_string()),
                },
            }
        }
    }
    /// Open a fuzzy file search index rooted at the workspace cwd.
    pub async fn fuzzy_open(
        &self,
        root: Option<&std::path::Path>,
        request_id: Option<String>,
        hidden: bool,
        session_id: Option<String>,
        target_client_id: crate::file_system::TargetClientId,
    ) -> String {
        let search_root = root.unwrap_or(&self.shared.root_cwd);
        let mut manager = self.shared.fuzzy_searches.lock().await;
        manager.open(
            search_root,
            request_id,
            hidden,
            session_id,
            target_client_id,
        )
    }
    /// Routing info (session id + target client) stored for a search at open
    /// time, read by the notification driver to address status updates.
    pub async fn fuzzy_routing(
        &self,
        search_id: &str,
    ) -> (Option<String>, crate::file_system::TargetClientId) {
        let manager = self.shared.fuzzy_searches.lock().await;
        (
            manager.get_session_id(search_id),
            manager.get_target_client_id(search_id),
        )
    }
    /// Run one poll tick for an active fuzzy search. Returns the next batch of
    /// results (paths absolutized against the search root) or a signal to keep
    /// polling / stop. Drives the `grow/search/fuzzy/status` notification loop.
    pub async fn fuzzy_poll(
        &self,
        search_id: &str,
        min_generation: usize,
        has_query: bool,
        query_version: usize,
        limit: usize,
    ) -> crate::file_system::FuzzyPollOutcome {
        use crate::file_system::FuzzyPollOutcome;
        let mut manager = self.shared.fuzzy_searches.lock().await;
        if !manager.is_current_query(search_id, query_version) {
            return FuzzyPollOutcome::Stale;
        }
        let root = manager.get_root(search_id);
        match manager.get_results_filtered(search_id, min_generation, has_query) {
            None => {
                if manager.get_results(search_id).is_none() {
                    FuzzyPollOutcome::Closed
                } else {
                    FuzzyPollOutcome::Pending
                }
            }
            Some(mut data) => {
                data.matches.truncate(limit);
                if let Some(root) = &root {
                    for m in &mut data.matches {
                        let path_str = m.path.to_string();
                        if !path_str.starts_with('/') {
                            m.path = root.join(&path_str).to_string_lossy().into_owned().into();
                        }
                    }
                }
                FuzzyPollOutcome::Update(data)
            }
        }
    }
    /// Update the query for an active fuzzy search.
    /// Returns (min_generation, has_query, query_version) if the search exists.
    pub async fn fuzzy_change(
        &self,
        search_id: &str,
        query: &str,
        dirs_only: bool,
    ) -> Option<(usize, bool, usize)> {
        let mut manager = self.shared.fuzzy_searches.lock().await;
        manager.change(search_id, query, dirs_only)
    }
    /// Get fuzzy search results.
    pub async fn fuzzy_get_results(
        &self,
        search_id: &str,
    ) -> Option<crate::file_system::FuzzySearchData> {
        let mut manager = self.shared.fuzzy_searches.lock().await;
        manager.get_results(search_id)
    }
    /// Close a fuzzy search.
    pub async fn fuzzy_close(&self, search_id: &str) -> bool {
        let mut manager = self.shared.fuzzy_searches.lock().await;
        manager.close(search_id)
    }
    /// Install the sink used to deliver workspace-originated ext-notifications
    /// to the local client gateway.
    pub fn set_client_ext_sink(&self, sink: crate::session::ClientExtSink) {
        self.shared.client_ext_sink.store(Arc::new(Some(sink)));
    }
    /// Whether a client ext-notification sink has been installed.
    pub fn has_client_ext_sink(&self) -> bool {
        self.shared.client_ext_sink.load().is_some()
    }
    /// Deliver an ext-notification to the client via the installed sink.
    /// No-op when no sink is set.
    pub fn emit_client_ext(&self, method: String, params: serde_json::Value) {
        if let Some(sink) = self.shared.client_ext_sink.load_full().as_ref() {
            sink(method, params);
        }
    }
    /// Drive the `grow/search/fuzzy/status` stream for an active search: poll
    /// until done / closed / superseded, emitting each new result batch to the
    /// client through the ext-notification sink. Co-located with the manager so
    /// polling remains in-process.
    pub async fn run_fuzzy_notifications(
        &self,
        search_id: String,
        min_generation: usize,
        has_query: bool,
        query_version: usize,
        limit: usize,
    ) {
        use crate::file_system::FuzzyPollOutcome;
        use std::time::Duration;
        use tokio::time::interval;
        let (session_id, target_client_id) = self.fuzzy_routing(&search_id).await;
        let context_id = session_id.unwrap_or_else(|| "agent".to_string());
        let mut poll_interval = interval(Duration::from_millis(25));
        let mut last_generation: Option<usize> = None;
        let max_polls = 400;
        poll_interval.tick().await;
        for _ in 0..max_polls {
            poll_interval.tick().await;
            let data = match self
                .fuzzy_poll(&search_id, min_generation, has_query, query_version, limit)
                .await
            {
                FuzzyPollOutcome::Stale | FuzzyPollOutcome::Closed => break,
                FuzzyPollOutcome::Pending => continue,
                FuzzyPollOutcome::Update(data) => data,
            };
            if last_generation == Some(data.generation) {
                if data.done {
                    break;
                }
                continue;
            }
            last_generation = Some(data.generation);
            let mut params = serde_json::json!({
                "sessionId": context_id.as_str(),
                "searchId": search_id.as_str(),
                "matches": serde_json::to_value(&data.matches).unwrap_or_default(),
                "total": data.total,
                "done": data.done,
                "generation": data.generation,
            });
            if !target_client_id.is_none() {
                params["_meta"] = serde_json::json!({
                    "targetClientId": serde_json::to_value(&target_client_id).unwrap_or_default(),
                });
            }
            self.emit_client_ext("grow/search/fuzzy/status".to_string(), params);
            if data.done {
                break;
            }
        }
    }
    /// Run a content search (ripgrep) and return results.
    /// Run a streaming content (ripgrep) search rooted at `cwd`, emitting each
    /// batch as `grow/search/content/status` via the client sink, and returning
    /// the final result. Co-located with the sink so it streams in both modes.
    pub async fn run_content_search(
        &self,
        cwd: std::path::PathBuf,
        context_id: String,
        params: crate::file_system::ContentSearchParams,
    ) -> crate::error::WorkspaceResult<crate::file_system::ContentSearchData> {
        let handle = self.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        crate::file_system::content_search_streaming(&cwd, &params, cancel, move |batch| {
            let params = serde_json::json!({
                "sessionId": context_id.as_str(),
                "files": serde_json::to_value(&batch.files).unwrap_or_default(),
                "totalMatches": batch.total_matches,
                "totalFiles": batch.total_files,
                "done": batch.done,
                "truncated": batch.truncated,
            });
            handle.emit_client_ext("grow/search/content/status".to_string(), params);
        })
        .await
        .map_err(|e| WorkspaceError::Operation(e.to_string()))
    }
    pub fn get_or_create_codebase_index(
        &self,
        cwd: std::path::PathBuf,
    ) -> (Arc<codebase_graph::IndexManagerHandle>, bool) {
        self.shared.codebase_indexes.lock().get_or_create(cwd)
    }
    pub fn get_codebase_index(
        &self,
        cwd: &std::path::Path,
    ) -> Option<Arc<codebase_graph::IndexManagerHandle>> {
        self.shared.codebase_indexes.lock().get(cwd)
    }
    fn spawn_codebase_index_event_forwarder(&self) -> tokio::task::JoinHandle<()> {
        let shared = self.shared.clone();
        let root_cwd = self.shared.root_cwd.clone();
        let index_root =
            crate::session::git::find_git_root_from_path(&root_cwd).unwrap_or(root_cwd);
        tokio::spawn(async move {
            let mut rx = shared.events.subscribe();
            loop {
                match rx.recv().await {
                    Ok(workspace_types::WorkspaceEvent::FsChanged { ref path, kind }) => {
                        if let Some(idx) = shared.codebase_indexes.lock().get(&index_root) {
                            let event =
                                crate::fs_notify::ws_event_to_codebase_graph_event(path, kind);
                            if let Err(e) = idx.send_event(event) {
                                tracing::debug!(error = %e, "codebase graph: fs event forward failed");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "codebase index event forwarder lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            tracing::debug!("codebase index event forwarder exited");
        })
    }
    /// Post-creation session setup (browser service seeding, etc.).
    ///
    /// When the optional browser backend is enabled, seeds a fresh per-session `BrowserService`
    /// into the toolset unless one is already present (idempotent — safe
    /// against double-finalize on concurrent on-demand session creation).
    /// Toolset rebuilds carry the handle forward via
    /// [`WorkspaceSession::replace_carrying_browser_service`](crate::session::WorkspaceSession::replace_carrying_browser_service).
    ///
    /// Holds the session's `update_lock` for the whole read-check-insert so
    /// it cannot interleave with a concurrent toolset rebuild (which swaps
    /// in a fresh `FinalizedToolset` under the same lock) — otherwise the
    /// seed could land in a just-replaced, stale toolset and the live one
    /// would miss the browser service.
    ///
    pub(crate) async fn finalize_session_setup(&self, session: &crate::session::WorkspaceSession) {
        let _update_guard = session.update_lock.lock().await;
    }
    /// Look up an existing session.
    pub(crate) fn session(&self, session_id: &str) -> Option<Arc<WorkspaceSession>> {
        self.shared.sessions.read().get(session_id).cloned()
    }
    /// IDs of all sessions currently bound to this workspace.
    pub fn session_ids(&self) -> Vec<String> {
        self.shared.sessions.read().keys().cloned().collect()
    }
    pub fn session_count(&self) -> usize {
        self.shared.sessions.read().len()
    }
    /// Remove a session.
    pub fn drop_session(&self, caller_session_id: &str, session_id: &str) -> WorkspaceResult<()> {
        if caller_session_id != session_id {
            return Err(WorkspaceError::Unauthorized {
                caller: caller_session_id.to_owned(),
                target: session_id.to_owned(),
            });
        }
        let mut sessions = self.shared.sessions.write();
        let Some(session) = sessions.remove(session_id) else {
            return Err(WorkspaceError::SessionNotFound(session_id.to_owned()));
        };
        drop(sessions);
        session.abort_system_notify_forwarder();
        session.shutdown_terminal_backend();
        session.cancel_hunk_tracker();
        Ok(())
    }
}
/// Apply a tool notification to the ActivityTracker background-task count.
/// `started` must precede `completed`, else the unknown `completed` no-ops and
/// strands the count.
pub(crate) fn apply_background_task_notification(
    tracker: &crate::activity::ActivityTracker,
    notification: &tools::notification::types::ToolNotification,
) {
    use tools::notification::types::ToolNotification;
    match notification {
        ToolNotification::BashExecutionBackgrounded(bg) => {
            tracker.background_task_started(&bg.task_id);
        }
        ToolNotification::TaskCompleted(snap) => {
            tracker.background_task_completed(&snap.task_id);
        }
        _ => {}
    }
}
/// Feed local tool notifications into the activity tracker.
pub(crate) async fn run_activity_feed(
    tracker: Arc<crate::activity::ActivityTracker>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<tools::notification::types::ToolNotification>,
) {
    while let Some(notification) = rx.recv().await {
        apply_background_task_notification(&tracker, &notification);
    }
}
/// Compute SHA-256 hex digest.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(data))
}
/// Stream a file once: SHA-256 over every byte while capturing the
/// `[offset, offset + length)` overlap. Returns
/// `(hash_hex, range_bytes, total_streamed_bytes)`.
///
/// Shared by [`WorkspaceHandle::get_files`]' chunked reads and the
/// `file_system::client_fs` ops so the overlap arithmetic lives in one
/// place.
pub(crate) async fn stream_hash_and_range(
    path: &std::path::Path,
    offset: u64,
    length: u64,
) -> std::io::Result<(String, Vec<u8>, u64)> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;
    let req_end = offset.saturating_add(length);
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut chunk = Vec::new();
    let mut pos: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        let start = pos.max(offset);
        let end = (pos + n as u64).min(req_end);
        if start < end {
            let local_start = (start - pos) as usize;
            let local_end = (end - pos) as usize;
            chunk.extend_from_slice(&buf[local_start..local_end]);
        }
        pos += n as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), chunk, pos))
}
/// Resolve `$GROW_WORKSPACE_HOME` — the workspace-owned on-disk state root.
///
/// Precedence:
/// 1. `$GROW_WORKSPACE_HOME` (operator override).
/// 2. `<grow_home>/workspace`, where `<grow_home>` honours `$GROW_HOME` and
///    otherwise falls back to `~/.grow` (see [`config::grow_home`]).
pub fn resolve_workspace_home() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("GROW_WORKSPACE_HOME")
        && !p.trim().is_empty()
    {
        return std::path::PathBuf::from(p);
    }
    config::grow_home().join("workspace")
}
/// Watchdog for awaiting enqueue outcomes when answering an `After` turn
/// hook. MUST undercut the requester's 10s hook deadline or the reply (and
/// its ack) arrives after the requester gave up. Default 8s; override via
/// `GROW_WORKSPACE_AFTER_TURN_WATCHDOG_MS` (malformed values fall back).
fn after_turn_watchdog() -> std::time::Duration {
    const DEFAULT_MS: u64 = 8_000;
    let ms = std::env::var("GROW_WORKSPACE_AFTER_TURN_WATCHDOG_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MS);
    std::time::Duration::from_millis(ms)
}
/// Per-process ephemeral workspace home for test and local handles.
fn ephemeral_workspace_home() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("grow-workspace-ephemeral-{}", std::process::id()))
}
impl WorkspaceHandle {
    /// Minimal local handle. Requires a Tokio runtime.
    ///
    pub fn new_minimal(
        cwd: std::path::PathBuf,
        project_lsp_trusted: bool,
    ) -> WorkspaceResult<Self> {
        use crate::session::tool_config::WorkspaceSessionContextFactory;
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: tools::registry::types::ToolServerConfig { tools: vec![] },
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: Arc::new(WorkspaceSessionContextFactory::new()),
            hook_global_sources: vec![],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted,
        };
        Self::build(config, ephemeral_workspace_home())
    }
}
#[cfg(any(test, feature = "test-support"))]
impl WorkspaceHandle {
    fn test_config(
        root_cwd: std::path::PathBuf,
        factory: std::sync::Arc<
            crate::session::tool_config::test_support::TestSessionContextFactory,
        >,
    ) -> crate::config::WorkspaceConfig {
        use crate::config::{DEFAULT_EVENT_BUFFER_CAPACITY, WorkspaceConfig};
        use crate::session::tool_config::test_support::baseline_config;
        WorkspaceConfig {
            root_cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        }
    }
    /// Test handle backed by a temp dir. Zero sessions; `TempDir` kept alive via `Arc`.
    pub fn for_test() -> Self {
        use crate::session::tool_config::test_support::TestSessionContextFactory;
        let factory = std::sync::Arc::new(TestSessionContextFactory::new());
        let root_cwd = factory.temp.path().to_path_buf();
        Self::new(Self::test_config(root_cwd, factory))
            .expect("test workspace handle construction must succeed")
    }
    /// Like [`Self::for_test`] but rooted at `root` (must exist on disk).
    pub fn for_test_in(root: &std::path::Path) -> Self {
        use crate::session::tool_config::test_support::TestSessionContextFactory;
        let factory = std::sync::Arc::new(TestSessionContextFactory::new());
        Self::new(Self::test_config(root.to_path_buf(), factory))
            .expect("test workspace handle construction must succeed")
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::{DEFAULT_EVENT_BUFFER_CAPACITY, WorkspaceConfig};
    use crate::error::WorkspaceError;
    use crate::session::tool_config::resolve_session_toolset;
    use crate::session::tool_config::test_support::{
        TestSessionContextFactory, baseline_config, tc,
    };
    use std::sync::Arc;
    use tools::registry::types::ToolServerConfig;
    use tools::types::tool::ToolKind;
    use workspace_types::WorkspaceEvent;
    /// Create a test workspace handle with a "main" session pre-created.
    pub(crate) fn make_handle() -> WorkspaceHandle {
        make_handle_inner(Default::default())
    }
    fn make_handle_inner(status_config: crate::StatusConfig) -> WorkspaceHandle {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config,
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::build(config, ephemeral_workspace_home())
            .expect("handle construction should succeed");
        handle
            .create_session("main")
            .expect("create main session should succeed");
        handle
    }
    /// Client names advertised by a session's current toolset.
    fn session_tool_names(session: &Arc<crate::session::WorkspaceSession>) -> Vec<String> {
        session
            .toolset()
            .tool_definitions()
            .iter()
            .map(|d| d.function.name.clone())
            .collect()
    }
    fn swap_rejected_count(reason: &str, trigger: &str) -> u64 {
        crate::session::swap_policy::WORKSPACE_TOOLSET_SWAP_REJECTED_TOTAL
            .with_label_values(&[reason, trigger])
            .get()
    }
    /// The RPC path rejects a mid-turn config change with the retryable
    /// `TurnActive` error (counted); the retry at the turn boundary succeeds.
    #[tokio::test]
    async fn update_tool_config_rejects_mid_turn_then_succeeds_at_boundary() {
        let rejected_before = swap_rejected_count("turn_active", "update_tool_config");
        let handle = make_handle();
        handle.activity_tracker().turn_started("main", 1);
        let cfg = explicit_cfg("renamed_read");
        let err = handle
            .update_tool_config("main", "main", cfg.clone())
            .await
            .expect_err("a mid-turn config change must be rejected");
        assert!(
            matches!(err, WorkspaceError::TurnActive(ref s) if s == "main"),
            "got {err:?}"
        );
        assert!(
            swap_rejected_count("turn_active", "update_tool_config") > rejected_before,
            "the rejection must be counted"
        );
        let session = handle.session("main").expect("main session exists");
        assert!(
            session_tool_names(&session)
                .iter()
                .all(|n| n != "renamed_read"),
            "the rejected config must not take effect"
        );
        handle.activity_tracker().turn_completed("main", 1, 0);
        handle
            .update_tool_config("main", "main", cfg)
            .await
            .expect("the retry at the turn boundary must succeed");
        let session = handle.session("main").expect("main session exists");
        assert_eq!(
            session_tool_names(&session),
            vec!["renamed_read".to_owned()]
        );
    }
    /// TOCTOU lock: a turn that starts DURING the re-resolve (after the
    /// entry check passed) must still abort the install — the resolved
    /// toolset is discarded, the fingerprint stays unchanged, and the
    /// rejection is counted under `reason="turn_active_late"`. The retry at
    /// the turn boundary then succeeds.
    #[tokio::test]
    async fn update_tool_config_rejects_turn_started_during_resolve() {
        let late_rejected_before = swap_rejected_count("turn_active_late", "update_tool_config");
        let handle = make_handle();
        let session = handle.session("main").expect("main session exists");
        let toolset_before = session.toolset();
        let hook_handle = handle.clone();
        *handle.shared.post_resolve_test_hook.lock() = Some(Box::new(move || {
            hook_handle.activity_tracker().turn_started("main", 7);
        }));
        let cfg = explicit_cfg("late_read");
        let err = handle
            .update_tool_config("main", "main", cfg.clone())
            .await
            .expect_err("a turn starting mid-resolve must abort the install");
        assert!(
            matches!(err, WorkspaceError::TurnActive(ref s) if s == "main"),
            "got {err:?}"
        );
        assert!(
            swap_rejected_count("turn_active_late", "update_tool_config") > late_rejected_before,
            "the post-resolve rejection must be counted distinctly"
        );
        let session = handle.session("main").expect("main session exists");
        assert!(
            Arc::ptr_eq(&session.toolset(), &toolset_before),
            "the resolved toolset must be discarded, not installed"
        );
        assert!(
            session.tool_config_matches(None),
            "the unapplied config's fingerprint must NOT be recorded"
        );
        *handle.shared.post_resolve_test_hook.lock() = None;
        handle.activity_tracker().turn_completed("main", 7, 0);
        handle
            .update_tool_config("main", "main", cfg)
            .await
            .expect("the retry at the turn boundary must succeed");
        let session = handle.session("main").expect("main session exists");
        assert_eq!(session_tool_names(&session), vec!["late_read".to_owned()]);
    }
    /// Re-applying the session's current config mid-turn stays allowed
    /// (matching fingerprint), so hot-reload re-applies keep working
    /// during turns.
    #[tokio::test]
    async fn update_tool_config_reapply_of_current_config_allowed_mid_turn() {
        let handle = make_handle();
        let cfg = explicit_cfg("renamed_read");
        let session = handle
            .create_session_with_config("hot", None, Some(cfg.clone()), None, false)
            .expect("create session");
        session.set_tool_config_fingerprint(serde_json::to_value(&cfg).ok());
        handle.activity_tracker().turn_started("hot", 1);
        handle
            .update_tool_config("hot", "hot", cfg)
            .await
            .expect("an identical-config re-apply must not be turn_active-rejected");
    }
    #[tokio::test]
    async fn update_tool_config_identical_reapply_repairs_stale_resolve() {
        let handle = make_handle();
        let cfg = explicit_cfg("renamed_read");
        let session = handle
            .create_session_with_config("stale", None, Some(cfg.clone()), None, false)
            .expect("create session");
        session.set_tool_config_fingerprint(serde_json::to_value(&cfg).ok());
        let toolset_before = session.toolset();
        handle
            .update_tool_config("stale", "stale", cfg.clone())
            .await
            .expect("an identical re-apply must succeed");
        assert!(
            Arc::ptr_eq(&session.toolset(), &toolset_before),
            "without the stale marker the identical re-apply must not rebuild"
        );
        session.mark_stale_resolve();
        let rejected_before = swap_rejected_count("turn_active", "update_tool_config");
        handle.activity_tracker().turn_started("stale", 1);
        let err = handle
            .update_tool_config("stale", "stale", cfg.clone())
            .await
            .expect_err("a mid-turn recovery re-apply must be rejected");
        assert!(
            matches!(err, WorkspaceError::TurnActive(ref s) if s == "stale"),
            "got {err:?}"
        );
        assert!(
            swap_rejected_count("turn_active", "update_tool_config") > rejected_before,
            "the rejected recovery must be counted"
        );
        assert!(
            session.stale_resolve(),
            "the rejected recovery must keep the stale marker"
        );
        assert!(
            Arc::ptr_eq(&session.toolset(), &toolset_before),
            "the rejected recovery must not install"
        );
        handle.activity_tracker().turn_completed("stale", 1, 0);
        handle
            .update_tool_config("stale", "stale", cfg.clone())
            .await
            .expect("the boundary retry must repair the stale toolset");
        let session = handle.session("stale").expect("session exists");
        assert!(
            !Arc::ptr_eq(&session.toolset(), &toolset_before),
            "the recovery re-apply must install a freshly resolved toolset"
        );
        assert!(
            !session.stale_resolve(),
            "a successful install must clear the stale marker"
        );
        assert!(
            session.tool_config_matches(serde_json::to_value(&cfg).ok().as_ref()),
            "the stored fingerprint must be unchanged by the identical recovery"
        );
    }
    /// The `Terminal` resource of a session's current toolset.
    async fn toolset_terminal(
        toolset: &Arc<tools::registry::types::FinalizedToolset>,
    ) -> Arc<dyn tools::computer::types::TerminalBackend> {
        let res = toolset.resources.lock().await;
        res.get::<tools::types::resources::Terminal>()
            .map(|t| t.0.clone())
            .expect("toolset must carry a Terminal resource")
    }
    fn orphaned_swap_count() -> u64 {
        WORKSPACE_TERMINAL_BACKEND_ORPHANED_TOTAL
            .with_label_values(&["swap"])
            .get()
    }
    fn explicit_cfg(name_override: &str) -> ToolServerConfig {
        let mut renamed = tc("Grow:read_file", Some(ToolKind::Read));
        renamed.name_override = Some(name_override.to_owned());
        ToolServerConfig {
            tools: vec![renamed],
        }
    }
    /// Background-capable toolset (execute + task-output + kill), the shape
    /// the restart-recovery and RPC-survival tests resolve.
    pub(crate) fn background_capable_cfg() -> ToolServerConfig {
        ToolServerConfig {
            tools: vec![
                tc("Grow:read_file", Some(ToolKind::Read)),
                tc("Grow:run_terminal_cmd", Some(ToolKind::Execute)),
                tc("Grow:get_task_output", Some(ToolKind::BackgroundTaskAction)),
                tc("Grow:kill_task", Some(ToolKind::KillTaskAction)),
            ],
        }
    }
    /// A minimal bash-kind [`TerminalRunRequest`] for `command`, writing
    /// output under `out_dir`.
    ///
    /// [`TerminalRunRequest`]: tools::computer::types::TerminalRunRequest
    pub(crate) fn terminal_run_request(
        command: &str,
        out_dir: &std::path::Path,
        tool_call_id: &str,
    ) -> tools::computer::types::TerminalRunRequest {
        tools::computer::types::TerminalRunRequest {
            command: command.to_string(),
            working_directory: out_dir.to_path_buf(),
            env: std::collections::HashMap::new(),
            timeout: std::time::Duration::from_secs(60),
            output_byte_limit: 4096,
            output_file: out_dir.join(format!("{tool_call_id}.out")),
            notification_handle: tools::notification::ToolNotificationHandle::noop(),
            tool_call_id: tool_call_id.to_string(),
            display_command: None,
            auto_background_on_timeout: false,
            foreground_block_budget: None,
            kind: tools::computer::types::TaskKind::Bash,
            owner_session_id: None,
            goal_id: None,
            description: None,
        }
    }
    /// Start a `sleep 30` background task on `session`'s owned backend and
    /// return its handle. Shared by the swap-survival, update-survival, and
    /// restart tests.
    pub(crate) async fn start_background_sleep(
        session: &Arc<crate::session::WorkspaceSession>,
        out_dir: &std::path::Path,
        tool_call_id: &str,
    ) -> tools::computer::types::BackgroundHandle {
        session
            .terminal_backend()
            .run_background(terminal_run_request("sleep 30", out_dir, tool_call_id))
            .await
            .expect("start background task")
    }
    /// A local-bound session (external toolset installed via
    /// `bind_local_session`: the toolset keeps the shell's backend, the
    /// session-owned backend is an idle decoy) must reject Workspace-owned
    /// rebuilds rather than detach tools from the shell's live task table.
    #[tokio::test]
    async fn local_bound_session_rejects_workspace_rebuild() {
        let orphaned_before = orphaned_swap_count();
        let handle = make_handle();
        let donor = handle
            .create_session_with_config(
                "donor",
                None,
                Some(explicit_cfg("read_donor")),
                None,
                false,
            )
            .expect("create donor session");
        let local = handle
            .create_session_with_config(
                "local",
                None,
                Some(explicit_cfg("read_local")),
                None,
                false,
            )
            .expect("create local session");
        let external_toolset = donor.toolset();
        local.replace(local.effective_tool_config(), external_toolset.clone());
        assert!(
            !local.toolset_terminal_is_session_owned().await,
            "precondition: the installed toolset's Terminal must be external"
        );
        let local = handle.session("local").expect("local session still exists");
        assert!(
            Arc::ptr_eq(&local.toolset(), &external_toolset),
            "the local-bound session's toolset must remain externally owned"
        );
        assert!(
            Arc::ptr_eq(
                &toolset_terminal(&local.toolset()).await,
                donor.terminal_backend()
            ),
            "the external (shell) backend must still ride the toolset"
        );
        assert_eq!(
            orphaned_swap_count(),
            orphaned_before,
            "the skip must not fire the orphaned-backend tripwire"
        );
        let outcome = handle
            .resolve_and_swap_session_toolset(&local, explicit_cfg("read_new"))
            .await
            .expect("the skip is not an internal error at the choke point");
        assert_eq!(outcome, SwapOutcome::SkippedExternallyOwned);
        assert!(
            Arc::ptr_eq(&local.toolset(), &external_toolset),
            "the choke point must not swap an externally-owned toolset"
        );
        assert_eq!(orphaned_swap_count(), orphaned_before);
        let err = handle
            .update_tool_config("local", "local", explicit_cfg("read_new"))
            .await
            .expect_err("update_tool_config must refuse an externally-owned toolset");
        assert!(
            matches!(err, crate::error::WorkspaceError::ToolsetExternallyOwned(ref s) if s == "local"),
            "expected ToolsetExternallyOwned, got: {err:?}"
        );
        assert!(
            Arc::ptr_eq(&local.toolset(), &external_toolset),
            "the refused update must leave the toolset untouched"
        );
        let fp_local = serde_json::to_value(explicit_cfg("read_local")).ok();
        local.set_tool_config_fingerprint(fp_local.clone());
        handle
            .update_tool_config("local", "local", explicit_cfg("read_local"))
            .await
            .expect("an identical config on an externally-owned toolset is a no-op success");
        assert!(
            Arc::ptr_eq(&local.toolset(), &external_toolset),
            "the identical no-op must leave the externally-owned toolset untouched"
        );
        assert!(
            local.tool_config_matches(fp_local.as_ref()),
            "the identical no-op must leave the stored fingerprint untouched"
        );
        assert_eq!(orphaned_swap_count(), orphaned_before);
    }
    /// A background task started before a toolset swap must still be
    /// queryable through the NEW toolset's `Terminal` resource — the
    /// swap ⇒ empty task table + SIGKILL incident class.
    #[tokio::test]
    async fn background_task_survives_toolset_swap() {
        let orphaned_before = orphaned_swap_count();
        let handle = make_handle();
        let cfg_a = explicit_cfg("read_a");
        let session = handle
            .create_session_with_config("bg", None, Some(cfg_a.clone()), None, false)
            .expect("create session");
        session.set_tool_config_fingerprint(serde_json::to_value(&cfg_a).ok());
        let out_dir = tempfile::tempdir().expect("temp dir");
        let bg = start_background_sleep(&session, out_dir.path(), "bg-task").await;
        let cfg_b = explicit_cfg("read_b");
        handle
            .update_tool_config("bg", "bg", cfg_b)
            .await
            .expect("tool config update succeeds");
        let rebound = handle.session("bg").expect("session exists");
        let new_terminal = toolset_terminal(&rebound.toolset()).await;
        let task = new_terminal
            .get_task(&bg.task_id)
            .await
            .expect("the task table must survive the toolset swap");
        assert!(
            !task.completed,
            "the task's process must still be running after the swap"
        );
        assert_eq!(
            orphaned_swap_count(),
            orphaned_before,
            "the orphaned-backend tripwire must stay 0"
        );
        new_terminal.kill_task(&bg.task_id).await;
    }
    /// Test factory whose sessions own a PERSISTENT-shell backend (the
    /// production factory shape). The plain [`TestSessionContextFactory`]
    /// builds a non-persistent backend, which tracks no shell cwd — hence
    /// this wrapper for the shell-state-survival test.
    struct PersistentShellFactory {
        inner: TestSessionContextFactory,
    }
    impl crate::config::SessionContextFactory for PersistentShellFactory {
        fn build_session_context(
            &self,
            session_id: &str,
            cwd: std::path::PathBuf,
            session_env: Arc<std::collections::HashMap<String, String>>,
            backend: Arc<dyn tools::computer::types::TerminalBackend>,
        ) -> tools::registry::types::SessionContext {
            self.inner
                .build_session_context(session_id, cwd, session_env, backend)
        }
        fn build_terminal_backend(&self) -> crate::config::SessionTerminalBackend {
            crate::config::SessionTerminalBackend::local(
                tools::computer::local::LocalTerminalBackend::with_persistent_shell(),
            )
        }
        fn registry_builder(&self) -> tools::registry::types::ToolRegistryBuilder {
            self.inner.registry_builder()
        }
    }
    /// [`make_handle`] shape around a [`PersistentShellFactory`]; no
    /// pre-created session.
    fn make_persistent_shell_handle() -> WorkspaceHandle {
        let factory = Arc::new(PersistentShellFactory {
            inner: TestSessionContextFactory::new(),
        });
        let root_cwd = factory.inner.temp.path().to_path_buf();
        let config = WorkspaceConfig {
            root_cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        WorkspaceHandle::build(config, ephemeral_workspace_home())
            .expect("handle construction should succeed")
    }
    /// The persistent shell's state (a model-issued `cd`) survives a
    /// `Reresolved` toolset swap, because the shell lives inside the
    /// session-owned backend — the isolation-matrix #3 "persistent-shell
    /// cwd preserved" sub-assert, on the production backend shape
    /// (`with_persistent_shell`). Unix-only, like the persistent shell.
    #[cfg(unix)]
    #[tokio::test]
    async fn reresolved_swap_preserves_persistent_shell_cwd() {
        let handle = make_persistent_shell_handle();
        let root = handle.root_cwd().expect("root cwd");
        let cfg_a = explicit_cfg("read_a");
        let session = handle
            .create_session_with_config("shell-swap", None, Some(cfg_a.clone()), None, false)
            .expect("create session");
        session.set_tool_config_fingerprint(serde_json::to_value(&cfg_a).ok());
        std::fs::create_dir_all(root.join("swap_kept_dir")).expect("create subdir");
        let result = session
            .terminal_backend()
            .run(terminal_run_request("cd swap_kept_dir", &root, "shell-cd"))
            .await
            .expect("cd through the persistent shell");
        assert_eq!(
            result.exit_code,
            Some(0),
            "cd must succeed: {}",
            result.combined_output
        );
        let cwd_before = session
            .terminal_backend()
            .get_shell_cwd()
            .await
            .expect("the persistent shell must track a cwd after a command");
        assert_eq!(
            cwd_before.file_name().and_then(|n| n.to_str()),
            Some("swap_kept_dir"),
            "the shell must have entered the subdir: {}",
            cwd_before.display()
        );
        let cfg_b = explicit_cfg("read_b");
        handle
            .update_tool_config("shell-swap", "shell-swap", cfg_b)
            .await
            .expect("tool config update succeeds");
        let rebound = handle.session("shell-swap").expect("session exists");
        let cwd_after = toolset_terminal(&rebound.toolset())
            .await
            .get_shell_cwd()
            .await
            .expect("the swapped-in toolset's terminal must still track the shell cwd");
        assert_eq!(
            cwd_after, cwd_before,
            "the persistent shell's cwd must survive the toolset swap"
        );
    }
    /// Poll `backend` with a trivial command until its actor refuses it —
    /// proving an explicit shutdown, since callers still hold live `Arc`s.
    /// Shared by session teardown tests.
    pub(crate) async fn assert_backend_stops(
        backend: &Arc<dyn tools::computer::types::TerminalBackend>,
    ) {
        let out_dir = tempfile::tempdir().expect("temp dir");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let request = terminal_run_request("true", out_dir.path(), "probe");
            if backend.run(request).await.is_err() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "backend actor must stop after an explicit shutdown even with live Arcs"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    /// `drop_session` shuts the backend down explicitly: the actor stops even
    /// while other `Arc`s to the backend are still alive (teardown must not
    /// depend on the last toolset `Arc` dropping).
    #[tokio::test]
    async fn drop_session_shuts_down_terminal_backend_explicitly() {
        let handle = make_handle();
        let session = handle
            .create_session_with_config("doomed", None, None, None, false)
            .expect("create session");
        let retained_backend = session.terminal_backend().clone();
        let retained_toolset = session.toolset();
        drop(session);
        handle.drop_session("doomed", "doomed").expect("drop");
        assert_backend_stops(&retained_backend).await;
        drop(retained_toolset);
    }
    async fn assert_hunk_tracker_stops(tracker: &hunk_tracker::HunkTrackerHandle) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !tracker.is_closed() {
            assert!(
                std::time::Instant::now() < deadline,
                "hunk-tracker actor must stop within the deadline despite live \
                 handle clones"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    /// `drop_session` cancels the workspace-spawned hunk-tracker actor even
    /// while a leaked `HunkTrackerHandle` clone keeps its command channel
    /// open. Rationale on `cancel_hunk_tracker`.
    #[tokio::test]
    async fn drop_session_cancels_workspace_spawned_hunk_tracker() {
        let handle = make_handle();
        let session = handle
            .create_session_with_config("doomed-ht", None, None, None, false)
            .expect("create session");
        let leaked_tracker = session.hunk_tracker().clone();
        assert!(
            !leaked_tracker.is_closed(),
            "precondition: the actor is alive while the session exists"
        );
        drop(session);
        handle.drop_session("doomed-ht", "doomed-ht").expect("drop");
        assert_hunk_tracker_stops(&leaked_tracker).await;
    }
    /// The inverse guarantee: a tracker bound via `create_session_with_tracker`
    /// is externally owned, so `drop_session` must NOT cancel it. The agent
    /// shares such trackers with the workspace session.
    #[tokio::test]
    async fn drop_session_leaves_externally_owned_hunk_tracker_alive() {
        let handle = make_handle();
        let cwd = handle.shared.root_cwd.clone();
        let (hunk_event_tx, _hunk_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let owner_cancel = tokio_util::sync::CancellationToken::new();
        let tracker = HunkTrackerActor::spawn(
            "external-ht".to_string(),
            cwd.clone(),
            hunk_event_tx,
            TrackingMode::AllDirty,
            owner_cancel.clone(),
        );
        let session = handle
            .create_session_with_tracker("external-ht", cwd, tracker.clone(), None)
            .expect("create session");
        assert!(
            !tracker.is_closed(),
            "precondition: the actor is alive while the session exists"
        );
        drop(session);
        handle
            .drop_session("external-ht", "external-ht")
            .expect("drop");
        let _ = tracker.get_all_hunks().await;
        assert!(
            !tracker.is_closed(),
            "drop_session must not cancel an externally owned hunk tracker"
        );
        owner_cancel.cancel();
        assert_hunk_tracker_stops(&tracker).await;
    }
    /// Isolation matrix #5: a workspace process restart loses tasks (they are
    /// process state — physics), and what's pinned here is the recovery UX:
    /// the same session id recreates cleanly on the fresh process, the task
    /// table starts empty (loss is visible, not silent), and `get_task_output`
    /// for the lost id returns the informative not-found message.
    #[tokio::test]
    async fn restarted_workspace_recreates_session_and_reports_lost_task() {
        let handle_a = make_handle();
        let session_a = handle_a
            .create_session_with_config("reborn", None, Some(background_capable_cfg()), None, false)
            .expect("create session");
        let out_dir = tempfile::tempdir().expect("temp dir");
        let bg = start_background_sleep(&session_a, out_dir.path(), "restart-bg").await;
        assert!(
            session_a
                .terminal_backend()
                .get_task(&bg.task_id)
                .await
                .is_some(),
            "precondition: the task exists in the first process"
        );
        let handle_b = make_handle();
        let session_b = handle_b
            .create_session_with_config("reborn", None, Some(background_capable_cfg()), None, false)
            .expect("the session must recreate cleanly after a restart");
        assert!(
            session_b.terminal_backend().list_tasks().await.is_empty(),
            "precondition: a fresh handle must start with an empty task table"
        );
        let result = session_b
            .toolset()
            .call(
                "get_task_output",
                serde_json::json!({"task_ids": [bg.task_id.clone()]}),
                "restart-probe",
                None,
            )
            .await
            .expect("get_task_output must answer, not error");
        let tools::types::output::ToolOutput::TaskOutput(
            tool_types::TaskOutputOutput::TaskNotFound(msg),
        ) = &result.output
        else {
            panic!("expected TaskNotFound, got: {:?}", result.output);
        };
        assert!(
            msg.contains(&format!("Task {} not found", bg.task_id)),
            "the message must name the lost task id: {msg}"
        );
        assert!(
            msg.contains("No background tasks or subagents exist in this session"),
            "the message must say the restarted session has no tasks: {msg}"
        );
        session_a.terminal_backend().kill_task(&bg.task_id).await;
    }
    /// The client ext-notification sink is invoked with the emitted method +
    /// params, and is no-op until installed.
    #[tokio::test]
    async fn client_ext_sink_receives_emitted_notification() {
        let handle = make_handle();
        assert!(!handle.has_client_ext_sink());
        handle.emit_client_ext("grow/noop".to_string(), serde_json::json!({}));
        let captured = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink_captured = captured.clone();
        handle.set_client_ext_sink(Arc::new(move |method, params| {
            sink_captured.lock().push((method, params));
        }));
        assert!(handle.has_client_ext_sink());
        handle.emit_client_ext(
            "grow/search/fuzzy/status".to_string(),
            serde_json::json!({"a": 1}),
        );
        let got = captured.lock();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "grow/search/fuzzy/status");
        assert_eq!(got[0].1, serde_json::json!({"a": 1}));
    }
    /// End-to-end local streaming: open + change a fuzzy search over real files,
    /// run the notification driver, and assert a correctly-shaped
    /// `grow/search/fuzzy/status` is delivered through the sink with the match.
    #[tokio::test]
    async fn fuzzy_change_streams_status_through_sink() {
        use crate::file_system::TargetClientId;
        let handle = make_handle();
        let cwd = handle.root_cwd().unwrap();
        std::fs::write(cwd.join("alpha_widget.rs"), b"").unwrap();
        std::fs::write(cwd.join("beta_gadget.rs"), b"").unwrap();
        let captured = Arc::new(parking_lot::Mutex::new(Vec::<serde_json::Value>::new()));
        let sink_captured = captured.clone();
        handle.set_client_ext_sink(Arc::new(move |method, params| {
            if method == "grow/search/fuzzy/status" {
                sink_captured.lock().push(params);
            }
        }));
        let search_id = handle
            .fuzzy_open(
                Some(cwd.as_path()),
                None,
                false,
                Some("sess-1".into()),
                TargetClientId::None,
            )
            .await;
        let (min_gen, has_query, query_version) = handle
            .fuzzy_change(&search_id, "alpha_widget", false)
            .await
            .expect("search should exist");
        handle
            .run_fuzzy_notifications(search_id.clone(), min_gen, has_query, query_version, 50)
            .await;
        let got = captured.lock();
        assert!(
            !got.is_empty(),
            "expected at least one fuzzy status notification"
        );
        let last = got.last().unwrap();
        assert_eq!(last["sessionId"], "sess-1");
        assert_eq!(last["searchId"], serde_json::json!(search_id));
        let matches = last["matches"].as_array().expect("matches array");
        assert!(
            matches.iter().any(|m| m["path"]
                .as_str()
                .is_some_and(|p| p.contains("alpha_widget"))),
            "expected alpha_widget in matches, got: {last}"
        );
    }
    /// `WorkspaceHandle::new` (the test/default path, not `connect_local_workspace`)
    /// must use an ephemeral temp `workspace_home` — never the real
    /// `$GROW_WORKSPACE_HOME`; `new` stays runtime-light and never touches
    /// persistent workspace state.
    #[tokio::test]
    async fn new_defaults_to_ephemeral_home() {
        let handle = make_handle();
        let shared = handle.shared();
        let home = shared.workspace_home();
        assert!(
            home.starts_with(std::env::temp_dir()),
            "default workspace_home must live under the temp dir, got {}",
            home.display()
        );
        assert_ne!(
            home,
            resolve_workspace_home(),
            "default construction must NOT use the real $GROW_WORKSPACE_HOME"
        );
    }
    #[tokio::test]
    async fn shared_accessors_round_trip() {
        let handle = make_handle();
        assert!(handle.shared().root_cwd().to_str().is_some());
        assert!(!handle.shared().respect_gitignore());
        assert!(handle.shared().memory_config().is_none());
        assert!(!handle.shared().default_tool_config().tools.is_empty());
    }
    #[tokio::test]
    async fn hook_registry_empty_when_no_sources() {
        let handle = make_handle();
        let registry = handle.hook_registry();
        assert!(registry.is_empty(), "no sources => empty registry");
        assert!(
            handle.hook_load_errors().is_empty(),
            "no sources => no errors"
        );
    }
    #[tokio::test]
    async fn hook_registry_loads_from_hook_file() {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let settings_path = cwd.join("claude_settings.json");
        std::fs::write(
            &settings_path,
            r#"{"hooks":{"pre_tool_use":[{"hooks":[{"type":"command","command":"echo ok"}]}]}}"#,
        )
        .expect("write settings");
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![HookSourceConfig::HookFile(settings_path)],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::new(config).expect("ok");
        let registry = handle.hook_registry();
        assert!(!registry.is_empty(), "hook file should yield hooks");
        assert!(handle.hook_load_errors().is_empty());
    }
    #[tokio::test]
    async fn hook_registry_loads_from_directory() {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let hooks_dir = cwd.join("hooks");
        std::fs::create_dir_all(&hooks_dir).expect("mkdir");
        std::fs::write(
            hooks_dir.join("my_hook.json"),
            r#"{"hooks":{"session_start":[{"hooks":[{"type":"command","command":"echo hi"}]}]}}"#,
        )
        .expect("write hook file");
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![],
            hook_project_sources: vec![HookSourceConfig::Directory(hooks_dir)],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::new(config).expect("ok");
        let registry = handle.hook_registry();
        assert!(!registry.is_empty(), "directory source should yield hooks");
    }
    #[tokio::test]
    async fn hook_registry_snapshot_is_disconnected() {
        let handle = make_handle();
        let snap1 = handle.hook_registry();
        assert!(snap1.is_empty());
        {
            let spec = hooks::config::HookSpec {
                name: "injected".into(),
                event: hooks::event::HookEventName::SessionStart,
                handler_type: hooks::config::HandlerType::Command,
                configured_matcher: None,
                matcher: None,
                enabled: true,
                command: Some("echo injected".into()),
                command_raw: Some("echo injected".into()),
                url: None,
                url_raw: None,
                timeout_ms: 10_000,
                source_dir: std::path::PathBuf::from("/tmp"),
                extra_env: std::collections::HashMap::new(),
                layer: hooks::config::HookProvenance::File,
            };
            handle.shared.hook_registry.write().append_specs(vec![spec]);
        }
        assert!(snap1.is_empty(), "snapshot must not see live mutations");
        let snap2 = handle.hook_registry();
        assert!(!snap2.is_empty(), "fresh snapshot must see mutation");
    }
    #[tokio::test]
    async fn hook_load_errors_reported_for_bad_file() {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let bad_path = cwd.join("bad_settings.json");
        std::fs::write(&bad_path, "NOT VALID JSON").expect("write bad file");
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![HookSourceConfig::HookFile(bad_path)],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::new(config).expect("construction must still succeed");
        assert!(
            !handle.hook_load_errors().is_empty(),
            "bad JSON must produce load errors"
        );
    }
    #[tokio::test]
    async fn hook_registry_global_and_project_sources_merge() {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let global_settings = cwd.join("global.json");
        std::fs::write(
                &global_settings,
                r#"{"hooks":{"session_start":[{"hooks":[{"type":"command","command":"echo global"}]}]}}"#,
            )
            .expect("write");
        let project_settings = cwd.join("project.json");
        std::fs::write(
            &project_settings,
            r#"{"hooks":{"pre_tool_use":[{"hooks":[{"type":"command","command":"echo project"}]}]}}"#,
        )
        .expect("write");
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![HookSourceConfig::HookFile(global_settings)],
            hook_project_sources: vec![HookSourceConfig::HookFile(project_settings)],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::new(config).expect("ok");
        let registry = handle.hook_registry();
        assert_eq!(registry.len(), 2, "both sources must contribute hooks");
    }
    #[tokio::test]
    async fn hook_registry_missing_source_is_non_fatal() {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let missing = cwd.join("does_not_exist.json");
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![HookSourceConfig::HookFile(missing)],
            hook_project_sources: vec![],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::new(config).expect("must not panic on missing source");
        assert!(handle.hook_registry().is_empty());
        assert!(
            handle.hook_load_errors().is_empty(),
            "missing file should not produce errors"
        );
    }
    #[tokio::test]
    async fn hook_registry_empty_directory_yields_empty_registry() {
        let factory = Arc::new(TestSessionContextFactory::new());
        let cwd = factory.temp.path().to_path_buf();
        let empty_dir = cwd.join("empty_hooks");
        std::fs::create_dir_all(&empty_dir).expect("mkdir");
        let config = WorkspaceConfig {
            root_cwd: cwd,
            default_tool_config: baseline_config(),
            respect_gitignore: false,
            memory_config: None,
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            session_factory: factory,
            hook_global_sources: vec![],
            hook_project_sources: vec![HookSourceConfig::Directory(empty_dir)],
            skills_config: Default::default(),
            plugin_discovery_config: Default::default(),
            status_config: Default::default(),
            project_lsp_trusted: true,
        };
        let handle = WorkspaceHandle::new(config).expect("ok");
        assert!(handle.hook_registry().is_empty());
        assert!(handle.hook_load_errors().is_empty());
    }
    #[tokio::test]
    async fn codebase_index_forwarder_abort_releases_shared() {
        let handle = make_handle();
        tokio::task::yield_now().await;
        let before = Arc::strong_count(handle.shared());
        let task = handle.spawn_codebase_index_event_forwarder();
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        assert!(Arc::strong_count(handle.shared()) > before);
        task.abort();
        let _ = task.await;
        assert_eq!(
            Arc::strong_count(handle.shared()),
            before,
            "abort must drop the forwarder's WorkspaceShared ref"
        );
    }
    #[tokio::test]
    async fn resolve_service_path_normal() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let resolved = handle
            .resolve_service_path("src/main.rs", &canonical_root)
            .await
            .expect("normal path should resolve");
        assert_eq!(resolved, root.join("src/main.rs"));
    }
    #[tokio::test]
    async fn resolve_service_path_rejects_empty() {
        let handle = make_handle();
        let canonical_root = handle.canonical_root().await.unwrap();
        let err = handle
            .resolve_service_path("", &canonical_root)
            .await
            .expect_err("empty path must be rejected");
        let msg = format!("{err}");
        assert!(
            msg.contains("empty path"),
            "error should mention empty path: {msg}"
        );
    }
    #[tokio::test]
    async fn resolve_service_path_rejects_absolute_outside_root() {
        let handle = make_handle();
        let canonical_root = handle.canonical_root().await.unwrap();
        let err = handle
            .resolve_service_path("/etc/passwd", &canonical_root)
            .await
            .expect_err("absolute path outside root must be rejected");
        let msg = format!("{err}");
        assert!(
            msg.contains("escapes workspace root"),
            "error should mention escape: {msg}"
        );
    }
    #[tokio::test]
    async fn resolve_service_path_accepts_absolute_within_root() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let rel = handle
            .resolve_service_path("src/main.rs", &canonical_root)
            .await
            .expect("relative path should resolve");
        let abs_input = root.join("src/main.rs");
        let abs = handle
            .resolve_service_path(abs_input.to_str().expect("utf-8 path"), &canonical_root)
            .await
            .expect("absolute path within root should resolve");
        assert_eq!(abs, rel);
    }
    #[tokio::test]
    async fn resolve_service_path_rejects_escape() {
        let handle = make_handle();
        let canonical_root = handle.canonical_root().await.unwrap();
        let err = handle
            .resolve_service_path("../../etc/passwd", &canonical_root)
            .await
            .expect_err("escape path must be rejected");
        let msg = format!("{err}");
        assert!(
            msg.contains("path escapes workspace root"),
            "error should mention escape: {msg}"
        );
    }
    #[tokio::test]
    async fn resolve_service_path_allows_dotdot_within_root() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let resolved = handle
            .resolve_service_path("src/../lib.rs", &canonical_root)
            .await
            .expect("dotdot within root should resolve");
        assert_eq!(resolved, root.join("lib.rs"));
    }
    #[tokio::test]
    async fn resolve_service_path_rejects_symlink_escape() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let outside = tempfile::tempdir().expect("create outside dir");
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "top secret").expect("write secret");
        let link_path = root.join("escape_link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link_path).expect("create symlink");
        #[cfg(not(unix))]
        {
            return;
        }
        let err = handle
            .resolve_service_path("escape_link/secret.txt", &canonical_root)
            .await
            .expect_err("symlink escape must be rejected");
        let msg = format!("{err}");
        assert!(
            msg.contains("symlink escape"),
            "error should mention symlink escape: {msg}"
        );
    }
    /// A *dangling* leaf symlink (target missing, outside root) must be rejected:
    /// `canonicalize` fails NotFound, so the leaf is resolved via `read_link`.
    #[tokio::test]
    #[cfg(unix)]
    async fn resolve_service_path_rejects_dangling_symlink_escape() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let outside = tempfile::tempdir().expect("create outside dir");
        std::os::unix::fs::symlink(outside.path().join("new.txt"), root.join("lnk"))
            .expect("create symlink");
        let err = handle
            .resolve_service_path("lnk", &canonical_root)
            .await
            .expect_err("dangling symlink escape must be rejected");
        assert!(
            format!("{err}").contains("symlink escape"),
            "error should mention symlink escape: {err}"
        );
    }
    /// A multi-hop chain of dangling in-root links ending outside the root must
    /// be followed and rejected (not fall through the ancestor walk).
    #[tokio::test]
    #[cfg(unix)]
    async fn resolve_service_path_rejects_dangling_symlink_chain() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let outside = tempfile::tempdir().expect("outside");
        for i in 0..3 {
            std::os::unix::fs::symlink(
                root.join(format!("lnk{}", i + 1)),
                root.join(format!("lnk{i}")),
            )
            .expect("chain link");
        }
        std::os::unix::fs::symlink(outside.path().join("x"), root.join("lnk3")).expect("tail link");
        let err = handle
            .resolve_service_path("lnk0", &canonical_root)
            .await
            .expect_err("dangling symlink chain escaping root must be rejected");
        assert!(
            format!("{err}").contains("symlink escape")
                || format!("{err}").contains("unresolved symlink chain"),
            "unexpected error: {err}"
        );
    }
    #[tokio::test]
    async fn resolve_service_path_nested_subdir() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let resolved = handle
            .resolve_service_path("a/b/c/d.txt", &canonical_root)
            .await
            .expect("deeply nested path should resolve");
        assert_eq!(resolved, root.join("a/b/c/d.txt"));
    }
    #[tokio::test]
    async fn resolve_service_path_dot_current_dir() {
        let handle = make_handle();
        let root = handle.root_cwd().unwrap();
        let canonical_root = handle.canonical_root().await.unwrap();
        let resolved = handle
            .resolve_service_path("./src/./main.rs", &canonical_root)
            .await
            .expect("dot segments should be stripped");
        assert_eq!(resolved, root.join("src/main.rs"));
    }
    #[tokio::test]
    async fn per_session_hunk_tracker_isolation() {
        let handle = make_handle();
        let child = handle
            .create_session_with_config("child", None, None, None, false)
            .expect("session should be created");
        child.hunk_tracker().record_agent_write(
            std::path::PathBuf::from("/tmp/test-file.rs"),
            "fn main() {}".to_string(),
            0,
            None,
        );
        let child_hunks = child.hunk_tracker().get_all_hunks().await;
        assert!(
            !child_hunks.is_empty(),
            "child session should have tracked hunks"
        );
        let main = handle.session("main").expect("main session present");
        let main_hunks = main.hunk_tracker().get_all_hunks().await;
        assert!(
            main_hunks.is_empty(),
            "main session hunk tracker must be isolated from child: got {} hunks",
            main_hunks.len()
        );
    }
    #[tokio::test]
    async fn cancel_tool_call_marks_call_completed() {
        let handle = make_handle();
        let tracker = handle.activity_tracker();
        tracker.tool_call_started("call-1", "read_file", Some("main"));
        assert_eq!(tracker.snapshot().active_tool_calls, 1);
        handle.cancel_tool_call("main", "call-1");
        assert_eq!(
            tracker.snapshot().active_tool_calls,
            0,
            "cancel_tool_call should mark the call as completed"
        );
    }
    #[tokio::test]
    async fn cancel_tool_call_unknown_id_is_noop() {
        let handle = make_handle();
        handle.cancel_tool_call("main", "never-started");
        assert_eq!(handle.activity_tracker().snapshot().active_tool_calls, 0);
    }
    #[tokio::test]
    async fn on_session_ended_clears_turn_active() {
        let handle = make_handle();
        let tracker = handle.activity_tracker();
        tracker.turn_started("main", 1);
        assert!(tracker.is_turn_active("main"));
        handle.on_session_ended("main");
        assert!(
            !tracker.is_turn_active("main"),
            "on_session_ended should clear turn_active"
        );
    }
    #[tokio::test]
    async fn on_session_ended_unknown_session_is_noop() {
        let handle = make_handle();
        let tracker = handle.activity_tracker();
        let sessions_before = tracker.known_sessions();
        handle.on_session_ended("nonexistent");
        assert_eq!(
            tracker.known_sessions(),
            sessions_before,
            "on_session_ended must not create a new session entry"
        );
    }
    #[tokio::test]
    async fn compute_turn_injections_before_runs_turn_start_and_replies_noop() {
        use tool_protocol::turn_hook::{BeforeTurnPayload, HookReply, TurnHookRequest};
        let handle = make_handle();
        let reply = handle
            .compute_turn_injections(
                "main",
                &TurnHookRequest::Before(BeforeTurnPayload {
                    turn_number: 9,
                    model_id: "model".into(),
                    conversation_message_count: 0,
                    session_relationship: "primary".into(),
                    schema_version: tool_protocol::turn_hook::DEFAULT_SCHEMA_VERSION.into(),
                }),
            )
            .await;
        assert_eq!(reply, HookReply::default());
        assert!(
            handle
                .activity_tracker()
                .known_sessions()
                .iter()
                .any(|s| s == "main"),
            "Before request must drive on_before_turn (activity tracking)"
        );
    }
    /// The default watchdog must undercut the requester's 10s hook timeout.
    #[test]
    fn after_turn_watchdog_default_is_8s() {
        assert_eq!(after_turn_watchdog(), std::time::Duration::from_secs(8));
    }
}
