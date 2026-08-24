//! Tool config resolution pipeline.
//!
//! Two-step resolution:
//! 1. `effective_tool_config = config.tool_config.unwrap_or_else(|| parent.effective_tool_config.clone())`
//! 2. `toolset = build_finalized_toolset(effective_tool_config, &session.cwd, &session.session_env, ...)`
use crate::config::SessionContextFactory;
use crate::error::{WorkspaceError, WorkspaceResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tools::registry::types::{FinalizedToolset, ToolConfig, ToolRegistryBuilder, ToolServerConfig};
/// Create-shaped entry of the resolution pipeline: run
/// [`resolve_session_toolset_rebuild`] around a FRESH factory-built
/// session-lifetime terminal backend, and return that backend so the caller
/// can store it on the session it is creating. Session-less resolves (the
pub(crate) fn resolve_session_toolset(
    effective_tool_config: ToolServerConfig,
    cwd: PathBuf,
    session_env: Arc<HashMap<String, String>>,
    session_id: &str,
    factory: &dyn SessionContextFactory,
    local_registry: Option<tool_runtime::LocalRegistry>,
    lsp: Option<std::sync::Arc<dyn tools::implementations::lsp::LspBackend>>,
    viewer_ctx: Option<tool_runtime::WorkspaceViewerContext>,
    notification_handle: Option<tools::notification::types::ToolNotificationHandle>,
) -> WorkspaceResult<(
    ToolServerConfig,
    Arc<FinalizedToolset>,
    crate::config::SessionTerminalBackend,
)> {
    let terminal_backend = factory.build_terminal_backend();
    let (effective, toolset) = resolve_session_toolset_rebuild(
        effective_tool_config,
        cwd,
        session_env,
        session_id,
        factory,
        local_registry,
        lsp,
        viewer_ctx,
        notification_handle,
        terminal_backend.backend().clone(),
    )?;
    Ok((effective, toolset, terminal_backend))
}
/// Rebuild-shaped entry: finalize the effective config around an
/// EXISTING session-owned terminal backend. The parameter is non-optional on
/// purpose: every toolset-swap call site must state which backend it rebuilds
/// around, so background tasks and shell state can never be orphaned by a
/// resolve that silently built a fresh backend.
///
/// Returns the *unmodified* `effective_tool_config` (step-1 baseline) so
/// the caller can store it on the session. The FinalizedToolset reflects
/// exactly that baseline. Workspace owns resource assembly, not actor
/// authorization or MCP lifecycle; the shell's live bridge, exact-identity
/// capability state, and call-bound RWX permit are the single boundary.
pub(crate) fn resolve_session_toolset_rebuild(
    effective_tool_config: ToolServerConfig,
    cwd: PathBuf,
    session_env: Arc<HashMap<String, String>>,
    session_id: &str,
    factory: &dyn SessionContextFactory,
    local_registry: Option<tool_runtime::LocalRegistry>,
    lsp: Option<std::sync::Arc<dyn tools::implementations::lsp::LspBackend>>,
    viewer_ctx: Option<tool_runtime::WorkspaceViewerContext>,
    notification_handle: Option<tools::notification::types::ToolNotificationHandle>,
    terminal_backend: Arc<dyn tools::computer::types::TerminalBackend>,
) -> WorkspaceResult<(ToolServerConfig, Arc<FinalizedToolset>)> {
    let mut builder = factory.registry_builder();
    if let Some(lr) = local_registry {
        builder = builder.with_local_registry(lr);
    }
    let mut ctx = factory.build_session_context(session_id, cwd, session_env, terminal_backend);
    if let Some(lsp_handle) = lsp {
        ctx.lsp = Some(lsp_handle);
    }
    if let Some(handle) = notification_handle {
        ctx.notification_handle = handle;
    }
    let toolset = builder
        .finalize_with_trunc_config(
            effective_tool_config.clone(),
            ctx,
            tools::types::context::TruncationConfig::default(),
            viewer_ctx,
        )
        .map_err(|errs| {
            let summary: Vec<String> = errs.iter().map(|e| e.summary()).collect();
            WorkspaceError::Finalize(summary.join("; "))
        })?;
    Ok((effective_tool_config, Arc::new(toolset)))
}
/// Sanitize a `session_id` into a single safe filesystem path segment: chars
/// outside `[A-Za-z0-9_-]` become `_`, empty becomes `anon`. When any
/// replacement happened, an 8-hex digest of the ORIGINAL id is appended so the
/// mapping stays injective — plain substitution would collide distinct ids
/// (`sess/1` and `sess_1`) into one directory, cross-contaminating
/// persistence, rehydration, and [`crate::recovery::cleanup_stale_sessions`].
/// Already-safe ids (the common UUID case) map to themselves.
fn sanitize_session_id(session_id: &str) -> String {
    let mut safe = String::with_capacity(session_id.len());
    let mut modified = false;
    for c in session_id.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            safe.push(c);
        } else {
            safe.push('_');
            modified = true;
        }
    }
    if safe.is_empty() {
        safe.push_str("anon");
        modified = true;
    }
    if modified {
        let digest = crate::file_utils::sha256_hex(session_id.as_bytes());
        safe.push('-');
        safe.push_str(&digest[..8]);
    }
    safe
}
/// `<root>/sessions/<sanitized_id>`, creating the directory when possible.
fn ensure_session_dir(root: &std::path::Path, session_id: &str) -> (PathBuf, std::io::Result<()>) {
    let dir = root.join("sessions").join(sanitize_session_id(session_id));
    let created = std::fs::create_dir_all(&dir);
    (dir, created)
}
/// [`SessionContextFactory`] for local workspace sessions.
///
/// Workspace-only sessions do not own a durable Resources store, so they pass
/// an explicit no-op persistence capability. Grow sessions provide their
/// canonical session-directory capability through `AgentBuilder` instead.
///
/// [`SessionContext::session_folder`] is `/tmp/sessions/<sanitized_id>/`
/// (terminal logs and other tool artifacts — not the project `cwd`).
///
/// Terminal backends are persistent-shell [`LocalTerminalBackend`]s, built
/// once per session by [`build_terminal_backend`] and passed into every
/// [`build_session_context`] call.
///
/// [`build_terminal_backend`]: crate::config::SessionContextFactory::build_terminal_backend
/// [`build_session_context`]: crate::config::SessionContextFactory::build_session_context
/// [`LocalTerminalBackend`]: tools::computer::local::LocalTerminalBackend
pub struct WorkspaceSessionContextFactory;
impl Default for WorkspaceSessionContextFactory {
    fn default() -> Self {
        Self::new()
    }
}
impl WorkspaceSessionContextFactory {
    pub fn new() -> Self {
        Self
    }
    /// `/tmp/sessions/<sanitized_id>/` for terminal logs and other tool artifacts.
    fn resolve_session_folder(session_id: &str) -> PathBuf {
        let (dir, created) = ensure_session_dir(std::path::Path::new("/tmp"), session_id);
        if let Err(e) = created {
            tracing::warn!(
                session = %session_id,
                dir = %dir.display(),
                error = %e,
                "session_folder: failed to create dir; tools may create it on write"
            );
        }
        dir
    }
}
impl SessionContextFactory for WorkspaceSessionContextFactory {
    fn build_session_context(
        &self,
        session_id: &str,
        cwd: PathBuf,
        session_env: Arc<HashMap<String, String>>,
        backend: Arc<dyn tools::computer::types::TerminalBackend>,
    ) -> tools::registry::types::SessionContext {
        use tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig;
        let fs = Arc::new(tools::computer::local::LocalFs)
            as Arc<dyn tools::computer::types::AsyncFileSystem>;
        let notification_handle = tools::notification::ToolNotificationHandle::noop();
        tools::registry::types::SessionContext {
            backend,
            fs,
            cwd,
            session_folder: Self::resolve_session_folder(session_id),
            session_env,
            notification_handle,
            owner_session_id: Some(session_id.to_string()),
            subagent: None,
            parent_scheduler_handle: None,
            skills: vec![],
            resources_persistence: Arc::new(tools::persistence::ResourcesPersistence::noop()),
            memory_backend: None,
            web_fetch_config: build_web_fetch_config(),
            lsp: None,
            app_builder_deployer_config: AppBuilderDeployerConfig::default(),
            system_reminder_tag: tools::reminders::DEFAULT_REMINDER_TAG,
        }
    }
    fn build_terminal_backend(&self) -> crate::config::SessionTerminalBackend {
        crate::config::SessionTerminalBackend::local(
            tools::computer::local::LocalTerminalBackend::new(),
        )
    }
    fn registry_builder(&self) -> ToolRegistryBuilder {
        ToolRegistryBuilder::new()
    }
    fn known_tool_ids(&self) -> Arc<std::collections::HashSet<String>> {
        static IDS: std::sync::LazyLock<Arc<std::collections::HashSet<String>>> =
            std::sync::LazyLock::new(|| Arc::new(ToolRegistryBuilder::new().known_tool_ids()));
        IDS.clone()
    }
}
/// Build web fetch config. Enabled with default params unless
/// `GROW_DISABLE_WEB_FETCH=1` is set.
fn build_web_fetch_config() -> tools::implementations::grow_build::web_fetch::WebFetchConfig {
    use tools::implementations::grow_build::web_fetch::{WebFetchConfig, WebFetchParams};
    if std::env::var("GROW_DISABLE_WEB_FETCH").is_ok_and(|v| v == "1" || v == "true") {
        return WebFetchConfig::Disabled;
    }
    let mut params = WebFetchParams::default();
    if let Ok(proxy) = std::env::var("GROW_WEB_FETCH_PROXY") {
        params.proxy_endpoint = Some(proxy);
    }
    if config::env_bool("GROW_WEB_FETCH_ALLOW_LOCAL") == Some(true) {
        params.allow_local = Some(true);
    }
    WebFetchConfig::Enabled { params }
}
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use crate::config::SessionContextFactory;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tools::computer::local::{LocalFs, LocalTerminalBackend};
    use tools::notification::ToolNotificationHandle;
    use tools::registry::types::{
        SessionContext, ToolConfig, ToolRegistryBuilder, ToolServerConfig,
    };
    use tools::types::tool::ToolKind;
    /// Test factory: builds a `SessionContext` rooted at a per-test temp dir.
    pub struct TestSessionContextFactory {
        pub temp: TempDir,
    }
    impl Default for TestSessionContextFactory {
        fn default() -> Self {
            Self::new()
        }
    }
    impl TestSessionContextFactory {
        pub fn new() -> Self {
            Self {
                temp: TempDir::new().expect("create temp dir"),
            }
        }
    }
    impl SessionContextFactory for TestSessionContextFactory {
        fn build_session_context(
            &self,
            session_id: &str,
            cwd: PathBuf,
            session_env: Arc<HashMap<String, String>>,
            backend: Arc<dyn tools::computer::types::TerminalBackend>,
        ) -> SessionContext {
            let session_root = self
                .temp
                .path()
                .join(super::sanitize_session_id(session_id));
            std::fs::create_dir_all(&session_root).expect("create session root");
            SessionContext {
                backend,
                fs: Arc::new(LocalFs),
                cwd,
                session_folder: session_root.clone(),
                session_env,
                notification_handle: ToolNotificationHandle::noop(),
                owner_session_id: None,
                subagent: None,
                parent_scheduler_handle: None,
                skills: vec![],
                resources_persistence: Arc::new(
                    tools::persistence::ResourcesPersistence::local(
                        session_root.join("resources_state.json"),
                    )
                    .expect("pin resources state test store"),
                ),
                memory_backend: None,
                web_fetch_config: Default::default(),
                lsp: None,
                app_builder_deployer_config: Default::default(),
                system_reminder_tag: tools::reminders::DEFAULT_REMINDER_TAG,
            }
        }
        fn build_terminal_backend(&self) -> crate::config::SessionTerminalBackend {
            crate::config::SessionTerminalBackend::local(LocalTerminalBackend::new())
        }
        fn registry_builder(&self) -> ToolRegistryBuilder {
            ToolRegistryBuilder::new()
        }
    }
    /// `ToolConfig` builder helper.
    pub fn tc(id: &str, kind: Option<ToolKind>) -> ToolConfig {
        ToolConfig {
            id: id.to_owned(),
            params: None,
            name_override: None,
            params_name_overrides: None,
            description_override: None,
            kind,
        }
    }
    /// Minimal valid `ToolServerConfig` for finalize-time tests.
    pub fn baseline_config() -> ToolServerConfig {
        ToolServerConfig {
            tools: vec![
                tc("Grow:read_file", Some(ToolKind::Read)),
                tc("Grow:search_replace", Some(ToolKind::Edit)),
                tc("Grow:grep", Some(ToolKind::Search)),
                tc("Grow:list_dir", Some(ToolKind::ListDir)),
            ],
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SessionContextFactory;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tools::types::tool::ToolKind;
    fn factory_for_test() -> Arc<dyn SessionContextFactory> {
        Arc::new(test_support::TestSessionContextFactory::new())
    }
    fn empty_env() -> Arc<HashMap<String, String>> {
        Arc::new(HashMap::new())
    }
    #[tokio::test]
    async fn resolve_session_toolset_preserves_baseline() {
        let factory = factory_for_test();
        let cwd = PathBuf::from("/tmp");
        let baseline = test_support::baseline_config();
        let baseline_ids: Vec<String> = baseline.tools.iter().map(|t| t.id.clone()).collect();
        let (eff, ts, _backend) = resolve_session_toolset(
            baseline,
            cwd,
            empty_env(),
            "main",
            factory.as_ref(),
            None,
            None,
            None,
            None,
        )
        .expect("resolve");
        assert_eq!(
            eff.tools
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<String>>(),
            baseline_ids
        );
        assert!(!ts.tool_definitions().is_empty());
    }
    #[test]
    fn factory_session_folder_is_tmp_sessions_not_project_cwd() {
        let cwd = PathBuf::from("/workspace");
        let folder = WorkspaceSessionContextFactory::resolve_session_folder("sess-1");
        let expected = PathBuf::from("/tmp/sessions/sess-1");
        assert_eq!(folder, expected);
        assert!(folder.is_dir());
        assert!(!folder.starts_with(&cwd));
        assert_eq!(
            folder.join("terminal").join("call-42.log"),
            PathBuf::from("/tmp/sessions/sess-1/terminal/call-42.log")
        );
    }
    #[test]
    fn factory_session_folder_sanitizes_and_isolates_ids() {
        let sessions = PathBuf::from("/tmp/sessions");
        let hostile = WorkspaceSessionContextFactory::resolve_session_folder("../../etc");
        assert!(hostile.starts_with(&sessions));
        assert_eq!(hostile.parent(), Some(sessions.as_path()));
        assert_ne!(hostile, sessions.join("etc"));
        let a = WorkspaceSessionContextFactory::resolve_session_folder("sess/1");
        let b = WorkspaceSessionContextFactory::resolve_session_folder("sess_1");
        assert_ne!(a, b);
        let empty = WorkspaceSessionContextFactory::resolve_session_folder("");
        assert_eq!(empty.parent(), Some(sessions.as_path()));
        assert!(
            empty
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|s| s.starts_with("anon-"))
        );
        let uuid = "019f3c0a-e2c2-79a3-908d-e8a8f088fe52";
        assert_eq!(
            WorkspaceSessionContextFactory::resolve_session_folder(uuid),
            sessions.join(uuid)
        );
    }
    #[test]
    fn ensure_session_dir_creates_scoped_direct_child() {
        let home = tempfile::TempDir::new().unwrap();
        let (under_home, ok) = ensure_session_dir(home.path(), "shared-id");
        assert!(ok.is_ok());
        assert_eq!(under_home, home.path().join("sessions").join("shared-id"));
        assert!(under_home.is_dir());
        let (under_tmp, ok) = ensure_session_dir(std::path::Path::new("/tmp"), "shared-id");
        assert!(ok.is_ok());
        assert_eq!(under_tmp, PathBuf::from("/tmp/sessions/shared-id"));
        assert!(under_tmp.is_dir());
    }
    /// Sanitization is injective: distinct ids that substitute to the same
    /// base string still map to distinct directories (hash disambiguator),
    /// while already-safe ids map to themselves.
    #[test]
    fn sanitize_session_id_is_injective() {
        assert_eq!(super::sanitize_session_id("sess-1_a"), "sess-1_a");
        assert_ne!(
            super::sanitize_session_id("sess/1"),
            super::sanitize_session_id("sess_1"),
            "substitution collisions must be disambiguated"
        );
        assert_ne!(
            super::sanitize_session_id("sess/1"),
            super::sanitize_session_id("sess.1"),
        );
        assert!(super::sanitize_session_id("").starts_with("anon-"));
    }
    /// A toolset rebuilt for the SAME session rehydrates Resources from
    /// disk; a DIFFERENT session_id cold-starts with no cross-contamination.
    #[tokio::test]
    async fn resources_state_rehydrates_same_session_and_cold_starts_other() {
        use tools::types::resources::{State, WebCitationCounter};
        let factory = test_support::TestSessionContextFactory::new();
        let cwd = PathBuf::from("/tmp");
        let (_eff, ts_a, _backend_a) = resolve_session_toolset(
            test_support::baseline_config(),
            cwd.clone(),
            empty_env(),
            "sess-A",
            &factory,
            None,
            None,
            None,
            None,
        )
        .expect("build toolset A");
        {
            let mut res = ts_a.resources.lock().await;
            let counter = res.get_or_default::<State<WebCitationCounter>>();
            counter.counter = 123;
        }
        ts_a.save_and_flush_persistence().await.unwrap();
        drop(ts_a);
        let (_eff, ts_b, _backend_b) = resolve_session_toolset(
            test_support::baseline_config(),
            cwd.clone(),
            empty_env(),
            "sess-A",
            &factory,
            None,
            None,
            None,
            None,
        )
        .expect("build toolset B");
        {
            let res = ts_b.resources.lock().await;
            let counter = res
                .get::<State<WebCitationCounter>>()
                .expect("WebCitationCounter must be present after rehydration");
            assert_eq!(
                counter.counter, 123,
                "tool state must survive a rebuild for the same session (rehydration)"
            );
        }
        let (_eff, ts_c, _backend_c) = resolve_session_toolset(
            test_support::baseline_config(),
            cwd,
            empty_env(),
            "sess-B",
            &factory,
            None,
            None,
            None,
            None,
        )
        .expect("build toolset C");
        {
            let res = ts_c.resources.lock().await;
            let contaminated = res
                .get::<State<WebCitationCounter>>()
                .is_some_and(|c| c.counter == 123);
            assert!(
                !contaminated,
                "a different session_id must cold-start, never inherit sess-A state"
            );
        }
    }
}
