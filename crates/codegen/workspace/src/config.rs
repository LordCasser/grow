//! Workspace and session configuration types.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tools::registry::types::{SessionContext, ToolRegistryBuilder, ToolServerConfig};
/// Default capacity for the workspace event broadcast channel.
pub const DEFAULT_EVENT_BUFFER_CAPACITY: usize = 64;
/// A session-lifetime terminal backend paired with its explicit shutdown hook.
///
/// The backend (background-task registry + persistent shell) is owned by the
/// [`WorkspaceSession`](crate::session::WorkspaceSession) and injected into
/// every toolset re-resolve for that session, so background tasks and shell
/// state survive toolset swaps. The shutdown hook fires the backend's cancel
/// token — killing every child process group and stopping the actor — so
/// `drop_session`/evict teardown is an explicit act rather than a side effect
/// of the last `Arc` drop.
#[derive(Clone)]
pub struct SessionTerminalBackend {
    backend: Arc<dyn tools::computer::types::TerminalBackend>,
    shutdown: Arc<dyn Fn() + Send + Sync>,
}
impl SessionTerminalBackend {
    /// Pair an already-erased `backend` with its shutdown hook.
    ///
    /// Extension point for [`SessionContextFactory`] implementors whose
    /// backend is not a `LocalTerminalBackend` (the fields are private, so
    /// this is the only way to satisfy `build_terminal_backend` for other
    /// backend types); in-repo factories use [`Self::local`].
    pub fn new(
        backend: Arc<dyn tools::computer::types::TerminalBackend>,
        shutdown: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self { backend, shutdown }
    }
    /// Wrap a [`LocalTerminalBackend`], wiring the shutdown hook to its
    /// cancel token.
    ///
    /// [`LocalTerminalBackend`]: tools::computer::local::LocalTerminalBackend
    pub fn local(backend: tools::computer::local::LocalTerminalBackend) -> Self {
        let canceller = backend.clone();
        Self {
            backend: Arc::new(backend),
            shutdown: Arc::new(move || canceller.cancel()),
        }
    }
    /// The type-erased backend, as injected into toolset resolves.
    pub fn backend(&self) -> &Arc<dyn tools::computer::types::TerminalBackend> {
        &self.backend
    }
    /// Explicitly shut the backend down: kills all of its child process
    /// groups and stops its actor.
    pub fn shutdown(&self) {
        (self.shutdown)();
    }
}
impl std::fmt::Debug for SessionTerminalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionTerminalBackend")
            .finish_non_exhaustive()
    }
}
/// Pluggable producer of [`SessionContext`] / [`ToolRegistryBuilder`]
/// for each session.
///
/// The workspace itself doesn't know how to construct the tool runtime
/// (terminal backend, file system, persistence path, MCP client config,
/// notification handle, ...) -- those come from the local embedder. The
/// embedder hands us a factory at
/// `WorkspaceHandle::new` time and we call it on every session
/// resolution.
pub trait SessionContextFactory: Send + Sync {
    /// Build a fresh [`SessionContext`] for the given session, around the
    /// given terminal `backend` (constructing one here would waste an actor
    /// per resolve — the pipeline rebuilds toolsets around the session-owned
    /// backend, so the caller always supplies it).
    fn build_session_context(
        &self,
        session_id: &str,
        cwd: PathBuf,
        session_env: Arc<HashMap<String, String>>,
        backend: Arc<dyn tools::computer::types::TerminalBackend>,
    ) -> SessionContext;
    /// Build the session-lifetime terminal backend for a new session.
    /// Called once per session creation; toolset re-resolves reuse the
    /// session's stored backend instead of building another.
    fn build_terminal_backend(&self) -> SessionTerminalBackend;
    /// Build a fresh [`ToolRegistryBuilder`] with the workspace's
    /// full set of registered tools.
    fn registry_builder(&self) -> ToolRegistryBuilder;
    fn known_tool_ids(&self) -> Arc<std::collections::HashSet<String>> {
        Arc::new(self.registry_builder().known_tool_ids())
    }
}
/// Placeholder for the cross-session memory backend config.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MemoryConfig {}
/// Top-level config required to construct a [`crate::handle::WorkspaceHandle`].
///
/// `#[non_exhaustive]` so future fields are non-breaking.
#[non_exhaustive]
pub struct WorkspaceConfig {
    /// Workspace root directory.
    pub root_cwd: PathBuf,
    /// Baseline tool config for the main session.
    pub default_tool_config: ToolServerConfig,
    /// Whether session-scoped fs operations should respect `.gitignore`.
    pub respect_gitignore: bool,
    /// Optional cross-session memory config.
    pub memory_config: Option<MemoryConfig>,
    /// Capacity of the workspace event broadcast channel.
    pub event_buffer_capacity: usize,
    /// Pluggable [`SessionContext`] / [`ToolRegistryBuilder`] producer.
    pub session_factory: Arc<dyn SessionContextFactory>,
    /// Global hook sources (for example `$GROW_HOME/hooks/`).
    pub hook_global_sources: Vec<HookSourceConfig>,
    /// Project-scoped hook sources (e.g. `<project>/.grow/hooks/`).
    pub hook_project_sources: Vec<HookSourceConfig>,
    /// Skill discovery configuration: additional skill paths and
    /// path-prefix ignore list. Stored on `WorkspaceShared` for
    /// `discover_skills` calls. Defaults to empty (no extra paths,
    /// no ignores).
    pub skills_config: crate::discovery::SkillsConfig,
    /// Plugin discovery configuration: CLI plugin dirs, config paths,
    /// and disabled/enabled lists. Stored on `WorkspaceShared` for
    /// `discover_plugins` calls. Defaults to empty.
    pub plugin_discovery_config: crate::discovery::PluginDiscoveryConfig,
    /// Runtime settings for local activity tracking.
    pub status_config: crate::status_config::StatusConfig,
    /// Folder-trust verdict for repo-local (project-scoped) LSP servers from
    /// `<cwd>/.grow/lsp.json`: `false` drops them at load, `true` keeps them. The
    /// shell caller resolves the verdict and threads it in; callers without a
    /// folder-trust decision pass `true`.
    pub project_lsp_trusted: bool,
}
/// A single hook source: either one canonical JSON hook file or a directory of
/// such files. Maps 1:1 to [`hooks::discovery::HookSource`]
/// but uses owned `PathBuf` so the config struct is `'static`.
#[derive(Debug, Clone)]
pub enum HookSourceConfig {
    /// A single explicitly configured JSON hook file.
    HookFile(PathBuf),
    /// A directory of `*.json` hook files (e.g. `~/.grow/hooks/`).
    Directory(PathBuf),
}
