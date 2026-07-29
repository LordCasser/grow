use crate::agent::subagent::SubagentSpawnContext;
use agent_client_protocol as acp;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use xai_acp_lib::AcpAgentGatewaySender as GatewaySender;
pub(crate) type GatewayOut = <acp::AgentSide as xai_acp_lib::AcpSide>::OutMessage;
pub(crate) fn test_gateway() -> GatewaySender {
    let (tx, _rx) = mpsc::unbounded_channel();
    GatewaySender::new(tx)
}
/// Like `test_gateway` but returns the receiver; keep it alive for the test.
pub(crate) fn test_gateway_with_receiver() -> (GatewaySender, mpsc::UnboundedReceiver<GatewayOut>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (GatewaySender::new(tx), rx)
}
pub(crate) fn ctx_with_toggle(toggle: HashMap<String, bool>) -> SubagentSpawnContext {
    let (tx, _rx) = mpsc::unbounded_channel();
    SubagentSpawnContext {
        lsp: None,
        parent_max_turns: None,
        client_hooks: Default::default(),
        sampling_config: grow_sampler::SamplerConfig {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: Default::default(),
            auth_scheme: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: 256_000,
            force_http1: false,
            max_retries: None,
            stream_tool_calls: false,
            idle_timeout_secs: None,
            reasoning_effort: None,
            origin_client: None,
            attribution_callback: None,
            bearer_resolver: None,
            compactions_remaining: None,
            compaction_at_tokens: None,
            doom_loop_recovery: None,
        },
        attribution_callback: None,
        alpha_test_key: None,
        auth_method_id: acp::AuthMethodId::new("test"),
        model_id: acp::ModelId::new("test"),
        auth: None,
        parent_cwd: PathBuf::from("/tmp"),
        parent_session_id: "test-parent".into(),
        yolo_mode: false,
        subagent_event_tx: tx,
        hunk_tracker_handle: xai_hunk_tracker::HunkTrackerHandle::noop(),
        hunk_tracking_enabled: false,
        fs: Arc::new(grow_workspace::file_system::LocalFs::new(PathBuf::from(
            "/tmp",
        ))),
        terminal: Arc::new(crate::terminal::TerminalRunner::new(
            Arc::new(test_gateway()),
            acp::SessionId::new("test"),
        )),
        session_env: Arc::new(HashMap::new()),
        memory_config: None,
        web_fetch_config: Default::default(),
        app_builder_deployer_config: Default::default(),
        write_file_enabled: true,
        goal_enabled: false,
        background_workflows_enabled: false,
        ask_user_question_enabled: true,
        parent_cmd_tx: None,
        parent_session_info: None,
        subagent_roles: HashMap::new(),
        subagent_personas: HashMap::new(),
        parent_chat_state: None,
        available_models: indexmap::IndexMap::new(),
        subagent_model_overrides: HashMap::new(),
        subagent_toggle: toggle,
        todo_gate: false,
        remote_settings: None,
        laziness_debug_log: None,
        respect_gitignore: false,
        path_not_found_hints: false,
        plugin_registry: None,
        models_manager: Default::default(),
        file_tool_overrides: None,
        agent_config: None,
        hook_registry: None,
        parent_depth: 0,
        subagents_max_depth: grow_tools::implementations::grow_build::task::MAX_SUBAGENT_DEPTH,
        inference_idle_timeout_secs: 600,
        auto_compact_threshold_tiers: crate::agent::subagent::AutoCompactThresholdTiers::default(),
        permission_handle: None,
        worktree_type: crate::util::config::WorktreeType::Linked,
        image_description_model: crate::test_support::TEST_MODEL.to_owned(),
        workspace_ops: grow_workspace::WorkspaceOps::for_test(),
        auth_manager: Arc::new(crate::auth::AuthManager::new(
            std::path::Path::new("/tmp/nonexistent-grow-test"),
            crate::auth::ServiceAuthConfig::default(),
        )),
        parent_agent_name: None,
        parent_mcp_configs: vec![],
        managed_mcp_state: crate::session::managed_mcp::ManagedMcpStateHandle::default(),
        managed_mcp_proxy_base_url: String::new(),
        parent_mcp_pool: None,
        parent_tool_definitions: None,
        parent_skills: None,
        parent_skills_config: grow_agent::prompt::skills::SkillsConfig::default(),
        parent_compat: grow_tools::types::compat::CompatConfig::default(),
        task_completion_reservations: None,
        task_output_tool_name: grow_tools::reminders::task_completion::DEFAULT_TASK_OUTPUT_TOOL
            .to_string(),
        auto_wake_enabled: true,
        goal_loop_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        parent_terminal_backend: None,
        parent_notification_handle: None,
        parent_scheduler_handle: None,
    }
}
#[derive(Default)]
pub(crate) struct DummyLspDispatch;
#[async_trait::async_trait]
impl grow_tools::implementations::lsp::LspBackend for DummyLspDispatch {
    fn ensure_started_background(&self) {}
    async fn ensure_ready(&self) -> Result<(), String> {
        Ok(())
    }
    fn is_ready(&self) -> bool {
        true
    }
    async fn dispatch(
        &self,
        _input: &grow_tools::implementations::lsp::LspToolInput,
    ) -> grow_tools::implementations::lsp::LspToolResult {
        grow_tools::implementations::lsp::LspToolResult {
            text: String::new(),
            is_error: false,
        }
    }
    async fn drain_diagnostics(
        &self,
        _timeout: std::time::Duration,
    ) -> Option<grow_tools::implementations::lsp::DiagnosticsSummary> {
        None
    }
    async fn notify_file_changed(&self, _path: &std::path::Path, _content: &str) {}
    async fn read_diagnostics(
        &self,
        _paths: &[std::path::PathBuf],
    ) -> Vec<grow_tools::implementations::lsp::FileDiagnosticEntry> {
        vec![]
    }
}
