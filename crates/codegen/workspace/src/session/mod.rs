pub mod file_state;
pub mod git;
pub mod jj;
pub(crate) mod swap_policy;
pub mod tool_config;
use crate::capability::CapabilityMode;
use crate::config::{MemoryConfig, SessionContextFactory};
use crate::file_system::{AsyncFsWrapper, LocalFs};
use hunk_tracker::HunkTrackerHandle;
use mcp::servers::McpState;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tool_runtime::WorkspaceViewerContext;
use tools::notification::types::{ToolNotification, ToolNotificationHandle};
use tools::registry::types::{FinalizedToolset, ToolConfig, ToolServerConfig};
/// Minimal result types for git error reporting (duplicated from shell session/result).
pub mod result {
    use serde::Serialize;
    #[derive(Debug, Serialize)]
    pub struct ExtMethodError {
        pub code: i32,
        pub message: String,
        pub data: Option<serde_json::Value>,
    }
    impl ExtMethodError {
        pub fn with_data(code: i32, msg: String, data: impl Serialize) -> Self {
            Self {
                code,
                message: msg,
                data: serde_json::to_value(data).ok(),
            }
        }
    }
    #[derive(Debug)]
    pub struct ExtMethodResult<T> {
        pub result: Option<T>,
        pub error: Option<serde_json::Value>,
    }
}
/// Per-session state held in [`WorkspaceShared::sessions`].
///
/// The `effective_tool_config` baseline and the resolved `toolset` are
/// kept under a single `RwLock` so a hot reload swaps both atomically.
pub struct WorkspaceSession {
    pub(crate) session_id: String,
    pub(crate) cwd: PathBuf,
    pub(crate) session_env: Arc<HashMap<String, String>>,
    pub(crate) capability_mode: CapabilityMode,
    pub(crate) depth: u32,
    pub(crate) fork_budget: u32,
    pub(crate) hunk_tracker: HunkTrackerHandle,
    /// Cancel token for the workspace-spawned [`HunkTrackerActor`] backing
    /// [`Self::hunk_tracker`], fired on session teardown by
    /// [`Self::cancel_hunk_tracker`]. `None` when the tracker is externally
    /// owned (e.g. `create_session_with_tracker` / local shell mode).
    ///
    /// [`HunkTrackerActor`]: hunk_tracker::HunkTrackerActor
    pub(crate) hunk_tracker_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) async_fs: AsyncFsWrapper,
    inner: RwLock<WorkspaceSessionInner>,
    /// Per-session lock that serialises `update_tool_config` calls.
    pub(crate) update_lock: tokio::sync::Mutex<()>,
    /// Per-session MCP state (owned clients, etc.).
    pub(crate) mcp_state: Arc<tokio::sync::Mutex<McpState>>,
    /// Per-session feature-flag bag resolved at creation time, frozen for
    /// the session lifetime. `None` → tools use their safe defaults.
    pub(crate) viewer_ctx: Option<WorkspaceViewerContext>,
    /// Session-lifetime terminal backend (background-task registry +
    /// persistent shell). Created once at session construction; every toolset
    /// re-resolve reuses it, so background tasks and shell state survive
    /// toolset swaps. Its child processes die only via `kill_task`,
    /// [`Self::shutdown_terminal_backend`] (`drop_session`/evict), or process
    /// exit.
    ///
    /// Local-mode exception: `bind_local_session` installs an externally
    /// built toolset via plain [`Self::replace`], so that toolset's
    /// `Terminal` resource is the shell's own backend while this one sits
    /// idle as the sole safe teardown target. Never adopt an externally
    /// owned backend into this field (drop/evict would SIGKILL a backend
    /// shared with the shell) and never query this field for the live task
    /// table — the toolset's `Terminal` resource is the source of truth.
    terminal_backend: crate::config::SessionTerminalBackend,
    /// Canonical JSON of the explicit toolset used for this session. `None`
    /// means the session was resolved from the workspace default. This lets
    /// config updates detect changes without rebuilding an identical toolset.
    tool_config_fingerprint: std::sync::Mutex<Option<serde_json::Value>>,
    /// The last snapshot-driven rebuild failed and kept a stale toolset;
    /// cleared by any successful install. While set, an identical-config
    /// re-apply heals instead of reusing.
    stale_resolve: std::sync::atomic::AtomicBool,
    /// Whether this session forwards `BackgroundTaskCompleted` system notifications.
    #[allow(dead_code)]
    system_notifications: bool,
    /// Per-session notification sender, re-applied across toolset re-resolves.
    system_notify_handle: Option<ToolNotificationHandle>,
    /// Receiver paired with `system_notify_handle`, taken once by the forwarder.
    #[allow(dead_code)]
    pending_notif_rx:
        tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<ToolNotification>>>,
    /// Spawned forwarder handle; aborted on teardown. Sync mutex so the sync
    /// teardown path can abort without an await.
    system_notify_forwarder: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}
struct WorkspaceSessionInner {
    effective_tool_config: Arc<ToolServerConfig>,
    toolset: Arc<FinalizedToolset>,
}
impl std::fmt::Debug for WorkspaceSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceSession")
            .field("session_id", &self.session_id)
            .field("cwd", &self.cwd)
            .field("capability_mode", &self.capability_mode)
            .field("depth", &self.depth)
            .field("fork_budget", &self.fork_budget)
            .finish_non_exhaustive()
    }
}
impl WorkspaceSession {
    pub(crate) fn new(
        session_id: String,
        cwd: PathBuf,
        session_env: Arc<HashMap<String, String>>,
        capability_mode: CapabilityMode,
        depth: u32,
        fork_budget: u32,
        effective_tool_config: Arc<ToolServerConfig>,
        toolset: Arc<FinalizedToolset>,
        terminal_backend: crate::config::SessionTerminalBackend,
        hunk_tracker: HunkTrackerHandle,
        hunk_tracker_cancel: Option<tokio_util::sync::CancellationToken>,
        viewer_ctx: Option<WorkspaceViewerContext>,
        #[allow(dead_code)] system_notifications: bool,
        system_notify_channel: Option<(
            ToolNotificationHandle,
            tokio::sync::mpsc::UnboundedReceiver<ToolNotification>,
        )>,
    ) -> Self {
        let (system_notify_handle, pending_notif_rx) = match system_notify_channel {
            Some((handle, rx)) => (Some(handle), Some(rx)),
            None => (None, None),
        };
        let async_fs = AsyncFsWrapper::new(Arc::new(LocalFs::new(cwd.clone())));
        Self {
            session_id,
            cwd,
            session_env,
            capability_mode,
            depth,
            fork_budget,
            hunk_tracker,
            hunk_tracker_cancel,
            async_fs,
            inner: RwLock::new(WorkspaceSessionInner {
                effective_tool_config,
                toolset,
            }),
            terminal_backend,
            update_lock: tokio::sync::Mutex::new(()),
            tool_config_fingerprint: std::sync::Mutex::new(None),
            stale_resolve: std::sync::atomic::AtomicBool::new(false),
            mcp_state: Arc::new(tokio::sync::Mutex::new(McpState::new(vec![]))),
            viewer_ctx,
            system_notifications,
            system_notify_handle,
            #[allow(dead_code)]
            pending_notif_rx: tokio::sync::Mutex::new(pending_notif_rx),
            system_notify_forwarder: std::sync::Mutex::new(None),
        }
    }
    /// Whether this session opted into `BackgroundTaskCompleted` system
    /// notifications.
    #[allow(dead_code)]
    pub(crate) fn system_notifications(&self) -> bool {
        self.system_notifications
    }
    /// The per-session notification sender, re-applied on every toolset
    /// re-resolve so notifications keep flowing to the forwarder's channel.
    pub(crate) fn system_notify_handle(&self) -> Option<ToolNotificationHandle> {
        self.system_notify_handle.clone()
    }
    /// Take the stashed notification receiver (once) for the per-session
    /// forwarder to own.
    #[allow(dead_code)]
    pub(crate) async fn take_pending_notif_rx(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<ToolNotification>> {
        self.pending_notif_rx.lock().await.take()
    }
    /// Store the spawned forwarder handle, aborting any previous one.
    #[allow(dead_code)]
    pub(crate) fn set_system_notify_forwarder(&self, handle: tokio::task::JoinHandle<()>) {
        let mut guard = self
            .system_notify_forwarder
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(old) = guard.replace(handle) {
            old.abort();
        }
    }
    /// Abort the per-session system-notify forwarder on teardown.
    pub(crate) fn abort_system_notify_forwarder(&self) {
        if let Some(handle) = self
            .system_notify_forwarder
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            handle.abort();
        }
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub fn session_env(&self) -> &Arc<HashMap<String, String>> {
        &self.session_env
    }
    pub fn capability_mode(&self) -> CapabilityMode {
        self.capability_mode
    }
    pub fn depth(&self) -> u32 {
        self.depth
    }
    pub fn fork_budget(&self) -> u32 {
        self.fork_budget
    }
    pub fn hunk_tracker(&self) -> &HunkTrackerHandle {
        &self.hunk_tracker
    }
    /// Per-user feature-flag bag resolved at session-bind time.
    pub fn viewer_ctx(&self) -> Option<&WorkspaceViewerContext> {
        self.viewer_ctx.as_ref()
    }
    pub fn async_fs(&self) -> &AsyncFsWrapper {
        &self.async_fs
    }
    /// The session-lifetime terminal backend, injected into every toolset
    /// re-resolve so background tasks and shell state survive swaps.
    pub(crate) fn terminal_backend(&self) -> &Arc<dyn tools::computer::types::TerminalBackend> {
        self.terminal_backend.backend()
    }
    /// Explicitly shut the session's terminal backend down (kills all of its
    /// child process groups and stops its actor). Called by
    /// `drop_session`/evict so task teardown does not depend on when the last
    /// toolset `Arc` drops.
    pub(crate) fn shutdown_terminal_backend(&self) {
        self.terminal_backend.shutdown();
    }
    /// Cancel the workspace-spawned hunk-tracker actor, if this session owns
    /// one. Runs at the session drop chokepoints so the actor (which pins file
    /// contents in `file_states`) stops even while leaked handle clones hold
    /// its channel open.
    pub(crate) fn cancel_hunk_tracker(&self) {
        if let Some(token) = &self.hunk_tracker_cancel {
            token.cancel();
        }
    }
    /// Return the current resolved toolset (snapshot).
    pub fn toolset(&self) -> Arc<FinalizedToolset> {
        self.inner.read().toolset.clone()
    }
    /// Whether the current toolset's `Terminal` resource is the session-owned
    /// backend. `false` means the toolset is externally owned — the local
    /// (shell) mode shape installed by `bind_local_session`, where the shell's
    /// own backend rides the toolset and the session-owned backend is an idle
    /// decoy. Rebuild paths must skip such sessions: finalizing around
    /// [`Self::terminal_backend`] would swap the decoy into the toolset and
    /// detach tools from the shell's live task table. A toolset with no
    /// `Terminal` resource counts as session-owned (nothing to detach).
    pub(crate) async fn toolset_terminal_is_session_owned(&self) -> bool {
        let toolset = self.toolset();
        let res = toolset.resources.lock().await;
        match res.get::<tools::types::resources::Terminal>() {
            Some(t) => Arc::ptr_eq(&t.0, self.terminal_backend()),
            None => true,
        }
    }
    /// Return the current effective tool config baseline.
    pub fn effective_tool_config(&self) -> Arc<ToolServerConfig> {
        self.inner.read().effective_tool_config.clone()
    }
    /// Whether `fingerprint` matches the explicit toolset used by this
    /// session. `None` means default resolution.
    #[cfg(test)]
    pub(crate) fn tool_config_matches(&self, fingerprint: Option<&serde_json::Value>) -> bool {
        let guard = self
            .tool_config_fingerprint
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.as_ref() == fingerprint
    }
    /// Whether the last snapshot-driven rebuild failed and left the live
    /// toolset stale with respect to the current MCP snapshot.
    pub(crate) fn stale_resolve(&self) -> bool {
        self.stale_resolve
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    /// Mark the live toolset stale: a snapshot-driven rebuild failed and the
    /// previous toolset was kept.
    pub(crate) fn mark_stale_resolve(&self) {
        self.stale_resolve
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    /// Clear the stale marker: a freshly resolved toolset was installed.
    pub(crate) fn clear_stale_resolve(&self) {
        self.stale_resolve
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    /// Record the explicit toolset (or `None` for a default resolution)
    /// this session's toolset was resolved from.
    ///
    /// Unconditional: callers must pair this with the toolset swap under the
    /// session's `update_lock` so fingerprint and live toolset cannot diverge.
    pub(crate) fn set_tool_config_fingerprint(&self, fingerprint: Option<serde_json::Value>) {
        *self
            .tool_config_fingerprint
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = fingerprint;
    }
    /// [`Self::set_tool_config_fingerprint`], but only when no
    /// fingerprint was recorded yet. Session creation uses this outside the
    /// update lock, so a concurrent config update remains authoritative.
    pub(crate) fn set_tool_config_fingerprint_if_unset(
        &self,
        fingerprint: Option<serde_json::Value>,
    ) {
        let mut guard = self
            .tool_config_fingerprint
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = fingerprint;
        }
    }
    /// Replace both the baseline config and the resolved toolset atomically.
    ///
    /// TOOL-STATE CAVEAT: the outgoing toolset is not flushed here, so an
    /// in-process rebuild can drop up to one debounce window (≤500 ms) of
    /// unpersisted state. Intentionally not "fixed" with a flush-before-rebuild:
    /// tool `call()` does not hold `update_lock`, so a concurrent call would
    /// still race. Restart/snapshot scenarios are unaffected.
    pub(crate) fn replace(
        &self,
        new_effective_tool_config: Arc<ToolServerConfig>,
        new_toolset: Arc<FinalizedToolset>,
    ) {
        let mut w = self.inner.write();
        w.effective_tool_config = new_effective_tool_config;
        w.toolset = new_toolset;
    }
    /// [`Self::replace`], but first carries the session's
    /// `BrowserServiceHandle` from the old toolset into the new one.
    /// Rebuilds produce a fresh `FinalizedToolset`, so the browser service
    /// seeded post-finalize (`finalize_session_setup`) must be carried
    /// forward or the session's live browser state is lost.
    ///
    /// Without the optional browser backend there is no browser service to carry;
    /// only the terminal-orphan diagnostic runs before the swap.
    ///
    /// Callers must hold the session's `update_lock` so the read-then-swap
    /// cannot interleave with another rebuild.
    pub(crate) async fn replace_carrying_browser_service(
        &self,
        new_effective_tool_config: Arc<ToolServerConfig>,
        new_toolset: Arc<FinalizedToolset>,
    ) {
        let old_toolset = self.toolset();
        let old_terminal = {
            let res = old_toolset.resources.lock().await;
            res.get::<tools::types::resources::Terminal>()
                .map(|t| t.0.clone())
        };
        if let Some(old_terminal) = old_terminal
            && !Arc::ptr_eq(&old_terminal, self.terminal_backend())
        {
            crate::handle::WORKSPACE_TERMINAL_BACKEND_ORPHANED_TOTAL
                .with_label_values(&["swap"])
                .inc();
            tracing::error!(
                session_id = %self.session_id,
                "toolset swap: outgoing toolset's terminal backend is not the \
                 session-owned one — its background tasks die with the old toolset"
            );
        }
        self.replace(new_effective_tool_config, new_toolset);
    }
}
/// Sink for delivering a workspace-originated ext-notification (method +
/// params JSON) to the local client gateway.
pub type ClientExtSink = std::sync::Arc<dyn Fn(String, serde_json::Value) + Send + Sync>;
/// Workspace-wide shared state.
pub struct WorkspaceShared {
    pub(crate) default_tool_config: ToolServerConfig,
    /// Workspace root directory. Independent of any session — stored
    /// here so it survives session creation/deletion.
    pub(crate) root_cwd: std::path::PathBuf,
    pub(crate) sessions: RwLock<HashMap<String, Arc<WorkspaceSession>>>,
    pub(crate) session_factory: Arc<dyn SessionContextFactory>,
    pub(crate) mcp_tools_snapshot: arc_swap::ArcSwap<Vec<ToolConfig>>,
    pub(crate) events: tokio::sync::broadcast::Sender<workspace_types::WorkspaceEvent>,
    pub(crate) respect_gitignore: bool,
    pub(crate) memory_config: Option<MemoryConfig>,
    pub(crate) hook_registry: Arc<parking_lot::RwLock<hooks::discovery::HookRegistry>>,
    pub(crate) hook_load_errors: Vec<hooks::error::HookError>,
    /// Skill discovery configuration (extra paths, ignore prefixes).
    /// Used by `discover_skills` via the `discovery` module.
    pub(crate) skills_config: crate::discovery::SkillsConfig,
    /// Plugin discovery configuration (CLI dirs, config paths,
    /// disabled/enabled lists). Used by `discover_plugins` via the
    /// `discovery` module.
    pub(crate) plugin_discovery_config: crate::discovery::PluginDiscoveryConfig,
    /// Sink for workspace-originated ext-notifications to the local client
    /// (e.g. `grow/search/fuzzy/status`). `None` until
    /// set via [`WorkspaceHandle::set_client_ext_sink`](crate::handle::WorkspaceHandle::set_client_ext_sink).
    pub(crate) client_ext_sink: arc_swap::ArcSwap<Option<ClientExtSink>>,
    pub(crate) local_registry: tool_runtime::LocalRegistry,
    pub(crate) activity_tracker: std::sync::Arc<crate::activity::ActivityTracker>,
    /// Runtime settings for local activity tracking.
    pub(crate) status_config: crate::status_config::StatusConfig,
    /// Workspace-level fuzzy search manager shared by local operations.
    pub(crate) fuzzy_searches:
        std::sync::Arc<tokio::sync::Mutex<crate::file_system::FuzzySearchManager>>,
    pub(crate) lsp: Option<std::sync::Arc<dyn tools::implementations::lsp::LspBackend>>,
    pub(crate) codebase_indexes:
        std::sync::Arc<parking_lot::Mutex<crate::file_system::CodebaseIndexManager>>,
    /// Resolved `$GROW_WORKSPACE_HOME` — the workspace-owned on-disk state root
    /// (`<grow_home>/workspace` by default).
    pub(crate) workspace_home: std::path::PathBuf,
    /// `(path, size, mtime_ms) → sha256` memo for the client-facing
    /// `workspace.client_fs_*` ops, so unchanged files hash once per
    /// workspace instead of per stat/read.
    /// Test-only seam: runs after the toolset re-resolve returns and before
    /// the post-resolve turn re-check / install in
    /// `resolve_and_swap_session_toolset_locked`, so tests can interleave a
    /// turn start inside the check→install window deterministically.
    #[cfg(test)]
    pub(crate) post_resolve_test_hook: parking_lot::Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    pub(crate) client_fs_hash_memo: crate::file_system::client_fs::FileHashMemo,
}
impl WorkspaceShared {
    /// Workspace root directory.
    pub fn root_cwd(&self) -> &std::path::Path {
        &self.root_cwd
    }
    /// Resolved `$GROW_WORKSPACE_HOME` — the workspace-owned on-disk state root.
    pub fn workspace_home(&self) -> &std::path::Path {
        &self.workspace_home
    }
    pub fn default_tool_config(&self) -> &ToolServerConfig {
        &self.default_tool_config
    }
    pub fn respect_gitignore(&self) -> bool {
        self.respect_gitignore
    }
    pub fn memory_config(&self) -> Option<&MemoryConfig> {
        self.memory_config.as_ref()
    }
    pub fn mcp_tools_snapshot(&self) -> Arc<Vec<ToolConfig>> {
        self.mcp_tools_snapshot.load_full()
    }
    pub fn activity_tracker(&self) -> &std::sync::Arc<crate::activity::ActivityTracker> {
        &self.activity_tracker
    }
    pub fn fuzzy_searches(
        &self,
    ) -> &std::sync::Arc<tokio::sync::Mutex<crate::file_system::FuzzySearchManager>> {
        &self.fuzzy_searches
    }
    pub fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<workspace_types::WorkspaceEvent> {
        self.events.subscribe()
    }
    pub fn codebase_indexes(
        &self,
    ) -> &std::sync::Arc<parking_lot::Mutex<crate::file_system::CodebaseIndexManager>> {
        &self.codebase_indexes
    }
    /// Skill discovery configuration (extra paths and ignore
    /// prefixes). Used by the `discovery` module when the channel's
    /// `discover_skills` method is called.
    pub fn skills_config(&self) -> &crate::discovery::SkillsConfig {
        &self.skills_config
    }
    /// Plugin discovery configuration (CLI dirs, config paths,
    /// disabled/enabled lists). Used by the `discovery` module when
    /// the channel's `discover_plugins` method is called.
    pub fn plugin_discovery_config(&self) -> &crate::discovery::PluginDiscoveryConfig {
        &self.plugin_discovery_config
    }
    /// Re-resolve every session's toolset and emit `ToolsChanged` events.
    ///
    /// Shared implementation used by `on_mcp_snapshot_changed`,
    /// `on_mcp_snapshot_changed`.
    ///
    /// When `use_async_lock` is true, uses `.lock().await` on each
    /// session's `update_lock` (appropriate for spawned async tasks
    /// where notifications must not be silently lost). When false,
    /// uses `try_lock()` and skips sessions whose lock is held.
    pub(crate) async fn re_resolve_all_sessions(
        self: &Arc<Self>,
        source: &str,
        use_async_lock: bool,
    ) -> usize {
        use crate::session::swap_policy::{
            SessionSnapshot, SwapAction, SwapDecision, SwapPolicy, SwapTrigger,
            record_swap_decision,
        };
        let trigger = SwapTrigger::from_rebuild_source(source);
        let mcp_snap = self.mcp_tools_snapshot.load_full();
        let sessions: Vec<(String, Arc<WorkspaceSession>)> = {
            let guard = self.sessions.read();
            guard
                .iter()
                .map(|(id, s)| (id.clone(), s.clone()))
                .collect()
        };
        let mut rebuilt = 0usize;
        for (sid, session) in sessions {
            let guard = if use_async_lock {
                session.update_lock.lock().await
            } else {
                match session.update_lock.try_lock() {
                    Ok(g) => g,
                    Err(_) => {
                        tracing::trace!(
                            session = %sid,
                            source = %source,
                            "skipping rebuild: session update_lock held"
                        );
                        continue;
                    }
                }
            };
            let snapshot =
                SessionSnapshot::capture_for_rebuild(&session, &self.activity_tracker).await;
            match SwapPolicy::evaluate(&snapshot, trigger) {
                SwapDecision::Apply => {}
                SwapDecision::Skip(reason) => {
                    record_swap_decision(
                        &self.activity_tracker,
                        trigger,
                        &sid,
                        SwapAction::Skipped(reason),
                    );
                    tracing::warn!(
                        session = %sid,
                        source = %source,
                        "skipping rebuild: toolset terminal backend is externally \
                         owned (local bind)"
                    );
                    drop(guard);
                    continue;
                }
                decision @ (SwapDecision::Reuse | SwapDecision::Defer(_)) => {
                    debug_assert!(
                        false,
                        "snapshot rebuild produced a non-rebuild decision: {decision:?}"
                    );
                    tracing::error!(
                        session = %sid,
                        source = %source,
                        ?decision,
                        "skipping rebuild: snapshot rebuild policy returned a \
                         non-rebuild decision (policy regression)"
                    );
                    drop(guard);
                    continue;
                }
            }
            let baseline = (*session.effective_tool_config()).clone();
            match crate::session::tool_config::resolve_session_toolset_rebuild(
                baseline,
                session.capability_mode(),
                &mcp_snap,
                session.cwd().to_path_buf(),
                session.session_env().clone(),
                &sid,
                self.session_factory.as_ref(),
                Some(self.local_registry.clone()),
                self.lsp.clone(),
                session.viewer_ctx().cloned(),
                session.system_notify_handle(),
                session.terminal_backend().clone(),
            ) {
                Ok((effective, toolset)) => {
                    session
                        .replace_carrying_browser_service(Arc::new(effective), toolset)
                        .await;
                    session.clear_stale_resolve();
                    record_swap_decision(
                        &self.activity_tracker,
                        trigger,
                        &sid,
                        SwapAction::Applied,
                    );
                    let _ = self
                        .events
                        .send(workspace_types::WorkspaceEvent::ToolsChanged { session_id: sid });
                    rebuilt += 1;
                }
                Err(e) => {
                    session.mark_stale_resolve();
                    record_swap_decision(
                        &self.activity_tracker,
                        trigger,
                        &sid,
                        SwapAction::ApplyFailed,
                    );
                    tracing::warn!(
                        session = %sid,
                        source = %source,
                        error = %e,
                        "snapshot rebuild failed for session"
                    );
                }
            }
            drop(guard);
        }
        rebuilt
    }
}
