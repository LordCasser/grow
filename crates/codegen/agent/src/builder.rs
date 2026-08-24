//! AgentBuilder — fluent construction API for building Agents.
use crate::agent::Agent;
use crate::config::{AgentDefinition, BuiltinAgentName, PromptComposition};
use crate::config::{short_tool_name, tool_config_eq, tool_config_matches, tool_id_eq};
use crate::discovery::{SubagentEntry, SubagentSource};
use crate::error::AgentBuildError;
use crate::prompt::context::PromptContext;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tools::bridge::ToolBridge;
use tools::computer::types::{AsyncFileSystem, TerminalBackend};
use tools::notification::ToolNotificationHandle;
use tools::registry::types::SessionContext;
use tools::types::tool::ToolKind;
/// Builds an Agent from an AgentDefinition + session context.
///
/// Two main flows:
///
///   // 1. From a definition file
///   let def = AgentDefinition::from_file("agents/code-reviewer.md")?;
///   let agent = AgentBuilder::new(cwd, terminal_backend, notification_handle)
///       .from_definition(def)
///       .build()
///       .await?;
///
///   // 2. Programmatic (no file)
///   let agent = AgentBuilder::new(cwd, terminal_backend, notification_handle)
///       .with_name("my-agent")
///       .with_description("A custom agent")
///       .with_tools(vec!["read_file".into(), "grep".into()])
///       .build()
///       .await?;
pub struct AgentBuilder {
    working_directory: PathBuf,
    /// Model-facing working directory for path-bearing runtime projections.
    ///
    /// In forked sessions, the real `working_directory` is an overlay/worktree
    /// path (e.g., `~/.grow/worktrees/project/fork-...-overlay`) that must stay
    /// hidden from the model. When set, projections use this value instead of
    /// `self.working_directory`, so they show the original project path. Tool execution (`ToolContext.cwd`,
    /// `SessionContext.cwd`) is unaffected and continues to use the real path.
    prompt_working_directory: Option<String>,
    terminal_backend: Arc<dyn TerminalBackend>,
    fs_backend: Arc<dyn AsyncFileSystem>,
    notification_handle: ToolNotificationHandle,
    owner_session_id: Option<String>,
    parent_scheduler_handle:
        Option<tools::implementations::grow_build::scheduler::types::SchedulerHandle>,
    /// The agent definition — set via from_definition() or built up
    /// via individual with_*() calls.
    definition: Option<AgentDefinition>,
    /// Whether this builder produces a primary or subagent session prompt.
    prompt_audience: crate::prompt::context::PromptAudience,
    name: Option<String>,
    description: Option<String>,
    prompt_composition: PromptComposition,
    tools: Option<Vec<String>>,
    disallowed_tools: Vec<String>,
    skill_names: Vec<String>,
    agents_md: bool,
    custom_agent_role: Option<String>,
    memory_enabled: bool,
    is_non_interactive: bool,
    system_prompt_label: String,
    session_env: Option<Arc<HashMap<String, String>>>,
    resources_persistence: Arc<tools::persistence::ResourcesPersistence>,
    memory_backend: Option<Arc<dyn tools::types::memory_backend::MemoryBackend>>,
    web_fetch_config: tools::implementations::grow_build::web_fetch::WebFetchConfig,
    lsp: Option<std::sync::Arc<dyn tools::implementations::lsp::LspBackend>>,
    app_builder_deployer_config:
        tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig,
    write_file_enabled: bool,
    subagents_enabled: bool,
    background_workflows_enabled: bool,
    ask_user_question_enabled: bool,
    subagent_toggle: HashMap<String, bool>,
    task_model_slugs: Vec<String>,
    skills_config: crate::prompt::skills::SkillsConfig,
    bash_params_json: Option<serde_json::Map<String, serde_json::Value>>,
    ask_user_question_params_json: Option<serde_json::Map<String, serde_json::Value>>,
    plugin_registry: Option<std::sync::Arc<crate::plugins::PluginRegistry>>,
    context_window_tokens: Option<u64>,
    /// Session-scoped MCP tool-result inline cap (bytes). When `Some`, seeded
    /// into the toolset's `TruncationCfg` resource after finalize, where the
    /// MCP truncation path consults it before the process-global cap. The
    /// shell passes the winning repo-level `[mcp] max_output_bytes` here (and
    /// only when that tier wins the precedence stack — see
    /// `resolve_max_mcp_output_bytes_for_cwd` in shell).
    mcp_max_output_bytes: Option<usize>,
    /// Persisted announced skill names from a previous session.
    /// When set, restored into the SkillManager before `seed()` so that
    /// `seed()` sees non-empty `announced_names` and skips the
    /// `BaselineChange` pending, preventing duplicate system-reminder
    /// injection on session resume.
    persisted_announced_skill_names: Option<std::collections::HashSet<String>>,
    /// Pre-discovered skills inherited from a parent session.
    /// When set, `build()` uses these directly instead of running
    /// `list_skills_with_plugins()`.
    preloaded_skills: Option<Vec<tools::implementations::skills::types::SkillInfo>>,
}
/// Ensure the Plan control plane and clarification tool are available.
fn ensure_plan_tools(tool_config: &mut tools::registry::types::ToolServerConfig) {
    use tools::implementations::grow_build;
    let existing: std::collections::HashSet<&str> =
        tool_config.tools.iter().map(|tc| tc.id.as_str()).collect();
    let missing_control = !existing.contains("Grow:plan_control");
    let missing_ask = !existing.contains("Grow:ask_user_question");
    drop(existing);
    if missing_control {
        tool_config
            .tools
            .push((&grow_build::PlanControlTool).into());
    }
    if missing_ask {
        tool_config
            .tools
            .push((&grow_build::AskUserQuestionTool).into());
    }
}

/// Merge a shell-resolved params map into every matching tool's
/// `ToolConfig.params` (single copy of the loop the per-tool injections share).
fn merge_tool_params(
    tool_config: &mut tools::registry::types::ToolServerConfig,
    ids: &[&str],
    map: &serde_json::Map<String, serde_json::Value>,
) {
    for tc in &mut tool_config.tools {
        if ids.contains(&tc.id.as_str()) {
            let params = tc.params.get_or_insert_with(serde_json::Map::new);
            for (k, v) in map {
                params.insert(k.clone(), v.clone());
            }
        }
    }
}
fn apply_workflow_tool_gates(
    tool_config: &mut tools::registry::types::ToolServerConfig,
    background_workflows_enabled: bool,
) {
    use tools::types::tool::ToolKind;
    if !background_workflows_enabled {
        tool_config
            .tools
            .retain(|tool| tool.kind != Some(ToolKind::Workflow));
    }
}
impl AgentBuilder {
    pub fn new(
        working_directory: PathBuf,
        terminal_backend: Arc<dyn TerminalBackend>,
        notification_handle: ToolNotificationHandle,
    ) -> Self {
        Self {
            working_directory,
            prompt_working_directory: None,
            terminal_backend,
            fs_backend: Arc::new(tools::computer::local::LocalFs),
            notification_handle,
            owner_session_id: None,
            parent_scheduler_handle: None,
            definition: None,
            prompt_audience: crate::prompt::context::PromptAudience::Primary,
            name: None,
            description: None,
            prompt_composition: PromptComposition::Extend,
            tools: None,
            disallowed_tools: vec![],
            skill_names: vec![],
            agents_md: true,
            custom_agent_role: None,
            memory_enabled: false,
            is_non_interactive: false,
            system_prompt_label: crate::prompt::context::DEFAULT_SYSTEM_PROMPT_LABEL.to_string(),
            session_env: None,
            resources_persistence: Arc::new(tools::persistence::ResourcesPersistence::noop()),
            memory_backend: None,
            web_fetch_config: Default::default(),
            lsp: None,
            app_builder_deployer_config: Default::default(),
            write_file_enabled: true,
            subagents_enabled: false,
            background_workflows_enabled: false,
            ask_user_question_enabled: true,
            subagent_toggle: HashMap::new(),
            task_model_slugs: Vec::new(),
            skills_config: Default::default(),
            bash_params_json: None,
            ask_user_question_params_json: None,
            plugin_registry: None,
            context_window_tokens: None,
            mcp_max_output_bytes: None,
            persisted_announced_skill_names: None,
            preloaded_skills: None,
        }
    }
    /// Set persisted announced skill names for session resume.
    ///
    /// These names are restored into the `SkillManager` inside `build()`,
    /// after `ToolBridge::finalize_builder()` but before
    /// `seed_skill_discovery()`.  This ensures `seed()` sees non-empty
    /// `announced_names` and skips setting `pending = BaselineChange`.
    pub fn with_persisted_announced_skill_names(
        mut self,
        names: std::collections::HashSet<String>,
    ) -> Self {
        self.persisted_announced_skill_names = Some(names);
        self
    }
    /// Supply pre-discovered skills from a parent session instead of
    /// running filesystem discovery. When set, `build()` skips
    /// `list_skills_with_plugins()` and uses the snapshot directly.
    pub fn with_preloaded_skills(
        mut self,
        skills: Vec<tools::implementations::skills::types::SkillInfo>,
    ) -> Self {
        self.preloaded_skills = Some(skills);
        self
    }
    /// Load from a pre-parsed AgentDefinition.
    pub fn from_definition(mut self, def: AgentDefinition) -> Self {
        self.definition = Some(def);
        self
    }
    /// Set the prompt audience (Primary or Subagent).
    pub fn with_prompt_audience(
        mut self,
        audience: crate::prompt::context::PromptAudience,
    ) -> Self {
        self.prompt_audience = audience;
        self
    }
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
    pub fn with_prompt_composition(mut self, mode: PromptComposition) -> Self {
        self.prompt_composition = mode;
        self
    }
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = Some(tools);
        self
    }
    pub fn with_disallowed_tools(mut self, tools: Vec<String>) -> Self {
        self.disallowed_tools = tools;
        self
    }
    pub fn with_skills(mut self, skill_names: Vec<String>) -> Self {
        self.skill_names = skill_names;
        self
    }
    pub fn with_agents_md(mut self, enabled: bool) -> Self {
        self.agents_md = enabled;
        self
    }
    pub fn with_agent_role(mut self, prompt: String) -> Self {
        self.custom_agent_role = Some(prompt);
        self
    }
    pub fn with_memory_enabled(mut self, enabled: bool) -> Self {
        self.memory_enabled = enabled;
        self
    }
    /// Mark this session as non-interactive (headless / SDK / stdio /
    /// generic-ACP). Suppresses prompt sections that only make sense when
    /// a human is typing into the TUI prompt input (e.g. the `! <command>`
    /// shell-prefix tip and the `<user_guide>` TUI pointer).
    pub fn with_is_non_interactive(mut self, value: bool) -> Self {
        self.is_non_interactive = value;
        self
    }
    pub fn with_system_prompt_label(mut self, label: impl Into<String>) -> Self {
        self.system_prompt_label = label.into();
        self
    }
    pub fn with_session_env(mut self, env: Arc<HashMap<String, String>>) -> Self {
        self.session_env = Some(env);
        self
    }
    /// Session-scoped MCP tool-result inline cap (bytes).
    ///
    /// `Some(v)` is seeded into the toolset's `TruncationCfg` resource after
    /// finalize, so the MCP truncation path uses `v` instead of the
    /// process-global cap. `None` (default) seeds nothing — MCP truncation
    /// falls through to the process-global resolution.
    pub fn with_mcp_max_output_bytes(mut self, bytes: Option<usize>) -> Self {
        self.mcp_max_output_bytes = bytes;
        self
    }
    pub fn with_resources_persistence(
        mut self,
        persistence: Arc<tools::persistence::ResourcesPersistence>,
    ) -> Self {
        self.resources_persistence = persistence;
        self
    }
    /// Set the memory backend for cross-session knowledge retrieval.
    ///
    /// When set, the `memory_search` and `memory_get` tools can access
    /// the indexed memory store. When `None` (default), those tools
    /// return "Memory is not enabled".
    pub fn with_memory_backend(
        mut self,
        backend: Arc<dyn tools::types::memory_backend::MemoryBackend>,
    ) -> Self {
        self.memory_backend = Some(backend);
        self
    }
    /// Set a custom filesystem backend for the ToolBridge.
    ///
    /// When `None` (default), tools use `LocalFs` (direct disk I/O).
    /// Pass an ACP-backed implementation when the client advertises
    /// `clientCapabilities.fs.readTextFile` and `writeTextFile`.
    pub fn with_fs(mut self, fs: Arc<dyn AsyncFileSystem>) -> Self {
        self.fs_backend = fs;
        self
    }
    /// Set the session ID that owns processes spawned by this session's tools.
    pub fn with_owner_session_id(mut self, id: String) -> Self {
        self.owner_session_id = Some(id);
        self
    }
    /// Share the parent's scheduler handle so scheduled tasks survive subagent exit.
    pub fn with_parent_scheduler_handle(
        mut self,
        handle: tools::implementations::grow_build::scheduler::types::SchedulerHandle,
    ) -> Self {
        self.parent_scheduler_handle = Some(handle);
        self
    }
    /// Set the web fetch configuration.
    ///
    /// When `Enabled`, the `web_fetch` tool is registered and a `WebFetchClient`
    /// is injected into `Resources`. When `Disabled` (default), the tool is not
    /// registered. Feature-flagged via remote settings `web_fetch_enabled` and
    /// `GROW_WEB_FETCH` env var.
    pub fn with_web_fetch_config(
        mut self,
        config: tools::implementations::grow_build::web_fetch::WebFetchConfig,
    ) -> Self {
        self.web_fetch_config = config;
        self
    }
    pub fn with_lsp(
        mut self,
        handle: std::sync::Arc<dyn tools::implementations::lsp::LspBackend>,
    ) -> Self {
        self.lsp = Some(handle);
        self
    }
    /// Set the deploy service configuration.
    pub fn with_app_builder_deployer_config(
        mut self,
        config: tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig,
    ) -> Self {
        self.app_builder_deployer_config = config;
        self
    }
    /// Set the dynamic API key provider for tool HTTP clients.
    /// Enable or disable the `write` tool (default: enabled).
    pub fn with_write_file_enabled(mut self, enabled: bool) -> Self {
        self.write_file_enabled = enabled;
        self
    }
    /// Enable or disable subagent (task tool) support.
    ///
    /// When disabled, the `TaskTool` is stripped from the
    /// agent's tool config during `build()`, preventing the model from
    /// spawning child agent sessions.
    pub fn with_subagents_enabled(mut self, enabled: bool) -> Self {
        self.subagents_enabled = enabled;
        self
    }
    pub fn with_background_workflows_enabled(mut self, enabled: bool) -> Self {
        self.background_workflows_enabled = enabled;
        self
    }
    /// Set public model slugs advertised in the Grow Task description.
    pub fn with_task_model_slugs(mut self, slugs: Vec<String>) -> Self {
        self.task_model_slugs = slugs;
        self
    }
    /// Enable or disable the `ask_user_question` tool.
    ///
    /// When disabled, `Grow:ask_user_question` is stripped from the
    /// agent's tool config after `ensure_plan_mode_tools` injection, so
    /// the model cannot ask the user structured questions regardless of
    /// which built-in profile is in use. Driven by the shell's resolved gate
    /// (`resolve_ask_user_question`, default ON — remote settings/config/env act as
    /// a kill-switch) and/or the pager's `--no-ask-user` (`_meta.askUserQuestion`).
    pub fn with_ask_user_question_enabled(mut self, enabled: bool) -> Self {
        self.ask_user_question_enabled = enabled;
        self
    }
    /// Set per-subagent enable/disable toggles from `[subagents.toggle]`.
    ///
    /// Keys are agent names, values are booleans. Omitted agents default
    /// to enabled. When combined with `with_subagents_enabled(true)`, this
    /// controls which individual subagents appear in the Task tool description
    /// and are accepted at spawn time.
    pub fn with_subagent_toggle(mut self, toggle: HashMap<String, bool>) -> Self {
        self.subagent_toggle = toggle;
        self
    }
    /// Set the skills config (custom paths, ignore globs) from config.toml.
    /// Without this, only auto-discovered skills (cwd/.grow/skills, ~/.grow/skills)
    /// are included — custom paths added via `grow/skills/add` would be ignored.
    pub fn with_skills_config(mut self, config: crate::prompt::skills::SkillsConfig) -> Self {
        self.skills_config = config;
        self
    }
    /// Inject `[toolset.bash]` overrides from config.toml into bash tool params.
    pub fn with_bash_params(mut self, params: serde_json::Map<String, serde_json::Value>) -> Self {
        self.bash_params_json = Some(params);
        self
    }
    /// Inject the shell-resolved `[toolset.ask_user_question]` params
    /// (timeout policy) into the ask_user_question tool.
    pub fn with_ask_user_question_params(
        mut self,
        params: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        self.ask_user_question_params_json = Some(params);
        self
    }
    /// Set the plugin registry for plugin-aware skill/agent discovery.
    pub fn with_plugin_registry(
        mut self,
        registry: std::sync::Arc<crate::plugins::PluginRegistry>,
    ) -> Self {
        self.plugin_registry = Some(registry);
        self
    }
    /// Set the model context window size in tokens.
    pub fn with_context_window(mut self, tokens: u64) -> Self {
        self.context_window_tokens = Some(tokens);
        self
    }
    /// Override the working directory shown in model-facing projections.
    /// Tool execution paths are unaffected.
    ///
    /// Used by forked sessions so the model sees the original project path
    /// rather than the internal overlay/worktree path.
    pub fn with_prompt_working_directory(mut self, cwd: String) -> Self {
        self.prompt_working_directory = Some(cwd);
        self
    }
    fn resolve_definition(&self) -> AgentDefinition {
        if let Some(ref def) = self.definition {
            return def.clone();
        }
        let mut def = AgentDefinition::default_grow_build();
        if let Some(ref name) = self.name {
            def.name = name.clone();
        }
        if let Some(ref desc) = self.description {
            def.description = desc.clone();
        }
        def.prompt_composition = self.prompt_composition.clone();
        def.agents_md = self.agents_md;
        if let Some(ref prompt) = self.custom_agent_role {
            def.prompt_body = Some(prompt.clone());
        }
        if !self.skill_names.is_empty() {
            def.skills = self.skill_names.clone();
        }
        if let Some(ref tools) = self.tools {
            def.tools = tools.clone();
        }
        def.disallowed_tools = self.disallowed_tools.clone();
        def
    }
    /// Build the Agent.
    ///
    /// This is the full 10-step build process from the architecture doc.
    pub async fn build(mut self) -> Result<Agent, AgentBuildError> {
        let mut definition = self.resolve_definition();
        let working_dir_str = self.working_directory.to_str().unwrap_or(".").to_string();
        let skill_info = if let Some(preloaded) = self.preloaded_skills.take() {
            preloaded
        } else if definition.discover_skills {
            crate::prompt::skills::list_skills_with_plugins(
                Some(&working_dir_str),
                &self.skills_config,
                self.plugin_registry.as_deref(),
            )
            .await
        } else {
            vec![]
        };
        let preloaded_skill_paths: std::collections::HashSet<String> = if !definition
            .skills
            .is_empty()
        {
            let preloaded =
                crate::prompt::skills::resolve_preloaded_skills(&definition.skills, &skill_info)
                    .await;
            let paths = preloaded.iter().map(|s| s.path.clone()).collect();
            if !preloaded.is_empty() {
                let injection = crate::prompt::skills::format_skills_for_injection(&preloaded);
                if !injection.is_empty() {
                    definition.prompt_body =
                        Some(injection + &definition.prompt_body.unwrap_or_default());
                }
            }
            paths
        } else {
            std::collections::HashSet::new()
        };
        let tool_bridge_builder = ToolBridge::get_builder();
        let state_path = self.resources_persistence.state_path().to_path_buf();
        let mut tool_config = definition.tool_config.clone();
        if !definition.inject_default_tools && tool_config.tools.is_empty() {
            return Err(AgentBuildError::InvalidConfig(format!(
                "agent '{}' declares a curated toolset (inject_default_tools = false) \
                 but its tool list is empty; if the toolset is a registry preset \
                 (e.g. the external harness), the provider crate's register() must run \
                 at process startup before any agent is built",
                definition.name
            )));
        }
        if definition.inject_default_tools {
            if self.memory_backend.is_some() {
                use tools::implementations::memory;
                tool_config
                    .tools
                    .push((&memory::search_tool::MemorySearchImpl).into());
                tool_config
                    .tools
                    .push((&memory::get_tool::MemoryGetImpl).into());
            }
            if self.web_fetch_config.is_enabled() {
                use tools::implementations::grow_build;
                tool_config.tools.push((&grow_build::WebFetchTool).into());
            }
            if self.lsp.is_some() {
                tool_config
                    .tools
                    .push((&tools::implementations::grow_build::LspTool).into());
            }
            let has_write_tool = tool_config
                .tools
                .iter()
                .any(|tc| tc.id.ends_with(":write") || tc.id.ends_with(":Write"));
            if self.write_file_enabled && !has_write_tool {
                tool_config
                    .tools
                    .push((&tools::implementations::grow_build::WriteTool).into());
            }
            ensure_plan_tools(&mut tool_config);
        }
        if self.memory_backend.is_none() {
            let grow_build_ns = tools::types::tool::ToolNamespace::Grow.to_string();
            let mem_search_id = format!(
                "{grow_build_ns}:{}",
                tools::implementations::memory::MEMORY_SEARCH_TOOL_NAME
            );
            let mem_get_id = format!(
                "{grow_build_ns}:{}",
                tools::implementations::memory::MEMORY_GET_TOOL_NAME
            );
            tool_config
                .tools
                .retain(|tc| tc.id != mem_search_id && tc.id != mem_get_id);
        }
        if !self.ask_user_question_enabled {
            let ask_user_id = format!(
                "{}:ask_user_question",
                tools::types::tool::ToolNamespace::Grow,
            );
            tool_config.tools.retain(|tc| tc.id != ask_user_id);
        }
        apply_workflow_tool_gates(&mut tool_config, self.background_workflows_enabled);
        let task_tool_id = format!("{}:{}", tools::types::tool::ToolNamespace::Grow, "task");
        let mut task_stripped = false;
        if !self.subagents_enabled {
            tool_config.tools.retain(|tc| tc.id != task_tool_id);
            task_stripped = true;
        } else {
            let filter = definition.subagent_filter();
            let mut subagents = crate::discovery::all_subagents_with_plugins(
                &self.working_directory,
                &self.subagent_toggle,
                self.plugin_registry.as_deref(),
            );
            subagents.retain(|entry| filter.allows(&entry.name));
            if subagents.is_empty() {
                tool_config.tools.retain(|tc| tc.id != task_tool_id);
                task_stripped = true;
            } else if self.prompt_audience == crate::prompt::context::PromptAudience::Subagent {
                if let Some(task_tc) = tool_config
                    .tools
                    .iter_mut()
                    .find(|tc| tc.id == task_tool_id)
                {
                    task_tc.description_override = Some(CHILD_TASK_DESCRIPTION.to_string());
                }
            } else if let Some(task_tc) = tool_config
                .tools
                .iter_mut()
                .find(|tc| tc.id == task_tool_id)
            {
                task_tc.description_override =
                    Some(build_task_description(&subagents, &self.task_model_slugs));
            }
        }
        if task_stripped {
            use tools::types::tool::ToolNamespace;
            let has_satisfier = |ns: ToolNamespace, id: &str, needs_bg: bool| {
                let fq = format!("{ns}:{id}");
                tool_config.tools.iter().any(|tc| {
                    tc.id == fq
                        && (!needs_bg
                            || tc
                                .params
                                .as_ref()
                                .and_then(|p| p.get("enabled_background"))
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true))
                })
            };
            if !has_satisfier(ToolNamespace::Grow, "run_terminal_cmd", true)
                && !has_satisfier(ToolNamespace::GrowConcise, "run_terminal_cmd", true)
            {
                let lifecycle = ["get_task_output", "kill_task"];
                tool_config
                    .tools
                    .retain(|tc| !lifecycle.contains(&short_tool_name(&tc.id)));
            }
        }
        if let tools::implementations::grow_build::web_fetch::WebFetchConfig::Enabled { ref params } =
            self.web_fetch_config
            && let Ok(params_value) = serde_json::to_value(params)
            && let Some(obj) = params_value.as_object()
        {
            merge_tool_params(&mut tool_config, &["Grow:web_fetch"], obj);
        }
        if let Some(ref bash_params) = self.bash_params_json {
            merge_tool_params(
                &mut tool_config,
                &["Grow:run_terminal_cmd", "GrowConcise:run_terminal_cmd"],
                bash_params,
            );
        }
        if let Some(ref ask_params) = self.ask_user_question_params_json {
            merge_tool_params(&mut tool_config, &["Grow:ask_user_question"], ask_params);
        }
        if !definition.disallowed_tools.is_empty() {
            let before: std::collections::HashSet<String> =
                tool_config.tools.iter().map(|tc| tc.id.clone()).collect();
            tool_config
                .tools
                .retain(|tc| !tool_config_matches(&definition.disallowed_tools, tc));
            let after: std::collections::HashSet<String> =
                tool_config.tools.iter().map(|tc| tc.id.clone()).collect();
            let removed: std::collections::HashSet<&String> = before.difference(&after).collect();
            for d in &definition.disallowed_tools {
                let matched = removed.iter().any(|&id| tool_id_eq(d, id));
                if !matched {
                    tracing::warn!(agent = %definition.name, tool = %d, "disallowedTools entry matched nothing");
                }
            }
        }
        if !definition.tools.is_empty() {
            let registered_tool_ids = tool_bridge_builder.known_tool_ids();
            let mut unresolved: Vec<&str> = Vec::new();
            let mut recognized_but_unavailable: Vec<&str> = Vec::new();
            for t in &definition.tools {
                if t.starts_with("mcp__") {
                    continue;
                }
                if tool_config.tools.iter().any(|tc| tool_config_eq(t, tc)) {
                    continue;
                }
                if registered_tool_ids.iter().any(|id| tool_id_eq(t, id)) {
                    recognized_but_unavailable.push(t);
                } else {
                    unresolved.push(t);
                }
            }
            if !recognized_but_unavailable.is_empty() {
                tracing::debug!(
                    agent = %definition.name,
                    recognized_but_unavailable = ?recognized_but_unavailable,
                    "tools allowlist named recognized tools that aren't enabled; ignoring them"
                );
            }
            tool_config.tools.retain(|tc| {
                tool_config_matches(&definition.tools, tc)
                    || matches!(tc.kind, Some(ToolKind::SearchTool | ToolKind::UseTool))
            });
            tracing::debug!(agent = %definition.name, allowed = ?definition.tools, "tools allowlist applied");
            if !unresolved.is_empty() {
                tracing::warn!(
                    agent = %definition.name,
                    unresolved = ?unresolved,
                    allowed = ?definition.tools,
                    "tools allowlist had unmappable entries; those entries grant no tools"
                );
            }
        }
        tool_config
            .tools
            .retain(|tc| definition.session_tools_allowed(tc));
        let task_deps = ["task", "get_task_output", "kill_task"];
        let task_enabled = tool_config
            .tools
            .iter()
            .any(|tool| short_tool_name(&tool.id) == "task");
        if !task_enabled {
            tool_config
                .tools
                .retain(|tc| !task_deps.contains(&short_tool_name(&tc.id)));
            for tc in &mut tool_config.tools {
                if short_tool_name(&tc.id) == "run_terminal_cmd" {
                    let params = tc.params.get_or_insert_with(Default::default);
                    params.insert("enabled_background".into(), false.into());
                    params.insert("auto_background_on_timeout".into(), false.into());
                }
            }
        }
        let tool_bridge = ToolBridge::finalize_builder(
            tool_bridge_builder,
            tool_config,
            SessionContext {
                backend: self.terminal_backend,
                fs: self.fs_backend,
                cwd: self.working_directory.clone(),
                session_folder: state_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(std::env::temp_dir),
                session_env: self.session_env.unwrap_or_default(),
                notification_handle: self.notification_handle.clone(),
                owner_session_id: self.owner_session_id.clone(),
                subagent: None,
                parent_scheduler_handle: self.parent_scheduler_handle.take(),
                skills: skill_info.clone(),
                resources_persistence: self.resources_persistence,
                memory_backend: self.memory_backend,
                web_fetch_config: self.web_fetch_config,
                lsp: self.lsp,
                app_builder_deployer_config: self.app_builder_deployer_config,
                system_reminder_tag: tools::reminders::DEFAULT_REMINDER_TAG,
            },
        )
        .await
        .map_err(|e| AgentBuildError::ToolError(e.to_string()))?;
        if let Some(bytes) = self.mcp_max_output_bytes {
            tool_bridge.toolset().resources.lock().await.insert(
                tools::types::resources::TruncationCfg(tools::types::context::TruncationConfig {
                    mcp_max_output_bytes: Some(bytes),
                    ..Default::default()
                }),
            );
        }
        if let Some(names) = self.persisted_announced_skill_names {
            tool_bridge.restore_announced_skill_names(names).await;
        }
        let mut agents_md_files = if definition.agents_md {
            crate::prompt::agents_md::read_agents_config_with_paths(&working_dir_str).await
        } else {
            vec![]
        };
        {
            let initial_paths: Vec<PathBuf> = agents_md_files
                .iter()
                .map(|c| PathBuf::from(&c.file_path))
                .collect();
            let git_root = git2::Repository::discover(&self.working_directory)
                .ok()
                .and_then(|repo| repo.workdir().map(|p| p.to_path_buf()));
            let gitignore = crate::prompt::ignore::build_gitignore(git_root.as_deref());
            let canonical_cwd = dunce::canonicalize(&self.working_directory)
                .unwrap_or_else(|_| self.working_directory.clone());
            let canonical_root = git_root.as_ref().and_then(|r| dunce::canonicalize(r).ok());
            let chain: Vec<PathBuf> = if let Some(ref root) = canonical_root {
                let mut dirs = Vec::new();
                let mut current = Some(canonical_cwd.as_path());
                while let Some(dir) = current {
                    dirs.push(dir.to_path_buf());
                    if dir == root.as_path() {
                        break;
                    }
                    current = dir.parent();
                }
                dirs
            } else {
                vec![]
            };
            if let Some(gi) = gitignore.as_ref()
                && let Some(root) = git_root.as_ref()
            {
                tool_bridge
                    .seed_gitignore_filter(gi.clone(), root.clone())
                    .await;
            }
            tool_bridge
                .seed_agents_md(initial_paths, git_root.clone(), chain, gitignore)
                .await;
            let listing_skills = if preloaded_skill_paths.is_empty() {
                skill_info.clone()
            } else {
                skill_info
                    .iter()
                    .filter(|s| !preloaded_skill_paths.contains(&s.path))
                    .cloned()
                    .collect()
            };
            let skill_discovery_cwd = if definition.discover_skills {
                Some(self.working_directory.clone())
            } else {
                None
            };
            tool_bridge
                .seed_skill_discovery(
                    skill_discovery_cwd,
                    git_root,
                    listing_skills,
                    self.prompt_working_directory.clone(),
                    self.context_window_tokens,
                )
                .await;
        }
        if let Some(ref display_cwd) = self.prompt_working_directory {
            for file in &mut agents_md_files {
                file.file_path = file.file_path.replace(&working_dir_str, display_cwd);
            }
        }
        let prompt_context = PromptContext {
            prompt_composition: definition.prompt_composition.clone(),
            audience: self.prompt_audience,
            prompt_body: definition.prompt_body.clone(),
            agents_md_files,
            memory_enabled: self.memory_enabled,
            is_non_interactive: self.is_non_interactive,
            system_prompt_label: self.system_prompt_label,
        };
        let system_prompt = prompt_context.render().unwrap_or_default();
        let role_prompt = prompt_context.render_role(&tool_bridge).await;
        if let Some(rendered) = tool_bridge
            .render_prompt(&definition.description, &prompt_context.placeholders())
            .await
        {
            definition.description = rendered;
        }
        #[allow(clippy::arc_with_non_send_sync)]
        let tool_bridge = Arc::new(tool_bridge);
        Ok(Agent::new(
            definition,
            prompt_context,
            system_prompt,
            role_prompt,
            tool_bridge,
        ))
    }
}
/// CLI naming for the shared [`tool_types::build_task_description`] builder.
const TASK_TOOL_NAMING: tool_types::TaskToolNaming<'static> = tool_types::TaskToolNaming {
    task_tool: "${{ tools.by_kind.task }}",
    subagent_type_param: "${{ params.task.subagent_type }}",
    run_in_background_param: "${{ params.task.run_in_background }}",
    resume_from_param: "${{ params.task.resume_from }}",
    background_retrieval_tool: "${{ tools.by_kind.background_task_action }}",
    isolation_param: "${{ params.task.isolation }}",
};
/// Concise task-tool description for child sessions. Delegation from a child
/// is possible but discouraged — prefer doing the work directly.
///
/// NOTE: This hardcodes the built-in agent type names ("general-purpose",
/// "explore", "plan"). If custom child-visible subagent types become common,
/// consider generating this list dynamically like the parent description does.
const CHILD_TASK_DESCRIPTION: &str = "\
Launch a sub-agent to handle a specific sub-task. Use this only when \n\
the sub-task is clearly independent and would benefit from a separate \n\
context (e.g., a parallel search while you continue working).\n\
\n\
Prefer doing the work yourself unless delegation is clearly necessary.\n\
\n\
Usage: specify ${{ params.task.subagent_type }} (\"general-purpose\" or \"explore\"), \n\
a short ${{ params.task.description }}, and a detailed ${{ params.task.prompt }}.\n\
${{ params.task.run_in_background }}: Returns immediately with a subagent_id. Use the task output tool to retrieve results. This is set to true by default.";
/// CLI [`tool_types::SubagentToolNaming`]: each kind maps to its
/// `${{ tools.by_kind.* }}` template placeholder, so rendering a built-in's
/// `tools_template` reproduces the placeholders for the CLI's `TemplateRenderer`
/// to resolve at finalize time.
const SUBAGENT_TOOL_NAMING: tool_types::SubagentToolNaming<'static> =
    tool_types::SubagentToolNaming {
        execute: "${{ tools.by_kind.execute }}",
        read: "${{ tools.by_kind.read }}",
        edit: "${{ tools.by_kind.edit }}",
        list: "${{ tools.by_kind.list }}",
        search: "${{ tools.by_kind.search }}",
        plan: "${{ tools.by_kind.plan }}",
    };
/// Return the tool-access fragment for a built-in subagent type, sourced from the
/// shared [`tool_types`] catalog and rendered with [`SUBAGENT_TOOL_NAMING`]
/// (which re-emits the `${{ tools.by_kind.* }}` placeholders for the CLI's
/// `TemplateRenderer` to resolve at finalize time).
fn builtin_tools_fragment(name: BuiltinAgentName) -> String {
    let subagent = match name {
        BuiltinAgentName::GeneralPurpose => tool_types::GENERAL_PURPOSE_SUBAGENT,
        BuiltinAgentName::Explore => tool_types::EXPLORE_SUBAGENT,
        _ => return String::new(),
    };
    subagent.render_tools(&SUBAGENT_TOOL_NAMING)
}
const TASK_MODEL_PARAM: &str = "${{ params.task.model }}";
fn task_model_guidance(model_slugs: &[String]) -> String {
    let mut model_slugs = model_slugs.to_vec();
    model_slugs.sort_unstable();
    model_slugs.dedup();
    if model_slugs.is_empty() {
        return format!(
            "\n\nNo explicit model slugs are currently available. \
             Omit `{TASK_MODEL_PARAM}` to inherit the parent model."
        );
    }
    let model_list = model_slugs
        .into_iter()
        .map(|slug| format!("- {slug}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n\nIf the user explicitly asks for the model of a subagent/task, you may ONLY use model slugs from this list:\n\
         {model_list}\n\n\
         If the user does not explicitly request a model, omit `{TASK_MODEL_PARAM}` to inherit the parent model."
    )
}
/// Build the Task tool description with the effective subagent list.
///
/// Maps each [`SubagentEntry`] to the shared
/// [`tool_types::SubagentDescriptor`] and defers to
/// [`tool_types::build_task_description`] so the CLI and the prod chat
/// stack share one builder. Built-in (unshadowed) entries carry the hardcoded
/// tool-name fragment; user-defined entries carry `None` so their raw
/// `description` is used verbatim (markdown is fine — it's model-facing text).
pub(crate) fn build_task_description(
    subagents: &[SubagentEntry],
    model_slugs: &[String],
) -> String {
    let descriptors: Vec<tool_types::SubagentDescriptor> = subagents
        .iter()
        .map(|entry| {
            let tools = match &entry.source {
                SubagentSource::Builtin(b) => Some(builtin_tools_fragment(*b)),
                SubagentSource::UserDefined { .. } => None,
            };
            tool_types::SubagentDescriptor {
                name: entry.name.clone(),
                description: entry.description.clone(),
                tools,
            }
        })
        .collect();
    let mut description = tool_types::build_task_description(&descriptors, &TASK_TOOL_NAMING);
    description.push_str(&task_model_guidance(model_slugs));
    description
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentScope;

    /// reqwest is built with `rustls-no-provider` (see the vendoring notes on
    /// the workspace's rustls setup): production installs the ring provider at
    /// CLI startup, but tests bypass startup, so install it once here.
    #[ctor::ctor]
    fn install_rustls_provider() {
        diagnostics::tls::install_ring_provider_once();
    }

    fn entry(name: &str, desc: &str, source: SubagentSource) -> SubagentEntry {
        SubagentEntry {
            name: name.to_string(),
            description: desc.to_string(),
            source,
            shadows_builtin: None,
            config_source: tools::types::config_source::ConfigSource::Builtin,
        }
    }
    #[test]
    fn build_task_description_builtin_includes_tools() {
        let subagents = vec![
            entry(
                "general-purpose",
                "General-purpose agent.",
                SubagentSource::Builtin(BuiltinAgentName::GeneralPurpose),
            ),
            entry(
                "explore",
                "Explore agent.",
                SubagentSource::Builtin(BuiltinAgentName::Explore),
            ),
        ];
        let desc = build_task_description(&subagents, &[]);
        assert!(
            desc.contains(tool_types::GENERAL_PURPOSE_SUBAGENT.tools_template),
            "should include general-purpose tool names"
        );
        assert!(
            desc.contains(tool_types::EXPLORE_SUBAGENT.tools_template),
            "should include explore tool names"
        );
        assert!(
            desc.contains("- **general-purpose**: General-purpose agent."),
            "should include agent entry"
        );
    }
    #[test]
    fn build_task_description_user_entry_is_raw() {
        let subagents = vec![entry(
            "code-reviewer",
            "Reviews code for bugs and style issues.",
            SubagentSource::UserDefined {
                scope: AgentScope::Project,
            },
        )];
        let desc = build_task_description(&subagents, &[]);
        assert!(desc.contains("- **code-reviewer**: Reviews code for bugs and style issues."));
        assert!(
            !desc.contains("Has access to all tools:"),
            "user-defined entries should not get tool fragments"
        );
    }
    #[test]
    fn build_task_description_user_template_syntax_verbatim() {
        let subagents = vec![entry(
            "my-agent",
            "Uses ${{ some.template }} syntax.",
            SubagentSource::UserDefined {
                scope: AgentScope::User,
            },
        )];
        let desc = build_task_description(&subagents, &[]);
        assert!(
            desc.contains("${{ some.template }}"),
            "template-like syntax in user descriptions should be rendered verbatim"
        );
    }
    #[test]
    fn build_task_description_shadowed_builtin_uses_user_desc() {
        let subagents = vec![SubagentEntry {
            name: "explore".to_string(),
            description: "My custom explore agent.".to_string(),
            source: SubagentSource::UserDefined {
                scope: AgentScope::Project,
            },
            shadows_builtin: Some(BuiltinAgentName::Explore),
            config_source: tools::types::config_source::ConfigSource::Project {
                path: std::path::PathBuf::new(),
            },
        }];
        let desc = build_task_description(&subagents, &[]);
        assert!(
            desc.contains("- **explore**: My custom explore agent."),
            "shadowed built-in should use user description"
        );
        assert!(
            !desc.contains(tool_types::EXPLORE_SUBAGENT.tools_template),
            "shadowed built-in should NOT include built-in tool fragment"
        );
    }
    #[test]
    fn build_task_description_contains_header_and_footer() {
        let subagents = vec![entry(
            "explore",
            "Explore.",
            SubagentSource::Builtin(BuiltinAgentName::Explore),
        )];
        let desc = build_task_description(&subagents, &[]);
        assert!(
            desc.contains("Start a subagent that works on a task independently"),
            "should contain header"
        );
        assert!(
            desc.contains("## Usage notes"),
            "should contain footer with '## Usage notes' section"
        );
    }
    #[test]
    fn build_task_description_uses_template_variables() {
        let subagents = vec![entry(
            "explore",
            "Explore.",
            SubagentSource::Builtin(BuiltinAgentName::Explore),
        )];
        let desc = build_task_description(&subagents, &[]);
        assert!(
            desc.contains("${{ tools.by_kind.task }}"),
            "should use tools.by_kind.task template variable"
        );
        assert!(
            desc.contains("${{ tools.by_kind.read }}"),
            "should use tools.by_kind.read template variable"
        );
        assert!(
            desc.contains("${{ params.task.subagent_type }}"),
            "should use params.task.subagent_type template variable"
        );
        assert!(
            desc.contains("${{ params.task.model }}"),
            "should use params.task.model template variable"
        );
    }
    #[test]
    fn build_task_description_lists_public_model_slugs() {
        let subagents = vec![entry(
            "explore",
            "Explore.",
            SubagentSource::Builtin(BuiltinAgentName::Explore),
        )];
        let desc = build_task_description(
            &subagents,
            &["zeta".to_string(), "alpha".to_string(), "alpha".to_string()],
        );
        assert!(desc.contains(
            "If the user explicitly asks for the model of a subagent/task, you may ONLY use model slugs from this list:\n\
             - alpha\n\
             - zeta"
        ));
        assert!(desc.contains(
            "If the user does not explicitly request a model, omit `${{ params.task.model }}` to inherit the parent model."
        ));
        assert!(!desc.contains("Available model slugs:"));
        assert!(!desc.contains(concat!("grow", " models")));
    }
    #[test]
    fn build_task_description_handles_empty_model_catalog() {
        let subagents = vec![entry(
            "explore",
            "Explore.",
            SubagentSource::Builtin(BuiltinAgentName::Explore),
        )];
        let desc = build_task_description(&subagents, &[]);
        assert!(desc.contains("No explicit model slugs are currently available."));
        assert!(desc.contains("Omit `${{ params.task.model }}` to inherit the parent model."));
        assert!(!desc.contains(concat!("grow", " models")));
    }
    #[test]
    fn task_model_guidance_resolves_model_param_override() {
        use tools::types::template_renderer::TemplateRenderer;
        use tools::types::tool::ToolKind;
        let renderer = TemplateRenderer::new(
            Default::default(),
            std::collections::HashMap::from([(
                ToolKind::Task,
                std::collections::HashMap::from([("model".to_string(), "child_model".to_string())]),
            )]),
        );
        let rendered = renderer
            .render(&task_model_guidance(&["alpha".to_string()]))
            .expect("model guidance should render");
        assert!(rendered.contains("omit `child_model` to inherit the parent model"));
        assert!(!rendered.contains("params.task.model"));
    }
    #[test]
    fn child_task_description_is_concise() {
        assert!(
            CHILD_TASK_DESCRIPTION.contains("Prefer doing the work yourself"),
            "child description should discourage recursive delegation"
        );
        assert!(
            !CHILD_TASK_DESCRIPTION.contains("Agent types:"),
            "child description should not list agent types"
        );
        assert!(
            !CHILD_TASK_DESCRIPTION.contains("<example>"),
            "child description should not contain examples"
        );
        assert!(
            CHILD_TASK_DESCRIPTION.len() < 700,
            "child description should be compact, got {} chars",
            CHILD_TASK_DESCRIPTION.len()
        );
    }
    #[test]
    fn build_task_description_contains_resume_from_guidance() {
        let subagents = vec![entry(
            "general-purpose",
            "GP agent.",
            SubagentSource::Builtin(BuiltinAgentName::GeneralPurpose),
        )];
        let desc = build_task_description(&subagents, &[]);
        assert!(
            desc.contains("Resuming a previous agent (resume_from)"),
            "should contain resume_from section header"
        );
        assert!(
            desc.contains("resume_from"),
            "should reference the resume_from parameter"
        );
        assert!(
            desc.contains("keeps its full transcript and tool state"),
            "should describe resume semantics"
        );
        assert!(
            desc.contains("same subagent_type"),
            "should state the resumed agent must match subagent_type"
        );
    }
    async fn build_pager_agent(
        profile: crate::config::AgentDefinition,
        subagents_enabled: bool,
        ask_user_question_enabled: bool,
    ) -> crate::agent::Agent {
        use tools::computer::local::LocalTerminalBackend;
        use tools::notification::ToolNotificationHandle;
        AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(profile)
        .with_subagents_enabled(subagents_enabled)
        .with_ask_user_question_enabled(ask_user_question_enabled)
        .build()
        .await
        .expect("agent should build for every pager-reachable flag combination")
    }
    #[tokio::test]
    async fn pager_flag_combinations_satisfy_tool_invariants() {
        use crate::config::AgentDefinition;
        struct PagerFlagCase {
            label: &'static str,
            profile: fn() -> AgentDefinition,
            subagents: bool,
            ask_user: bool,
        }
        let cases: &[PagerFlagCase] = &[
            PagerFlagCase {
                label: "grow-build / subagents+ask_user",
                profile: AgentDefinition::default_grow_build,
                subagents: true,
                ask_user: true,
            },
            PagerFlagCase {
                label: "grow-build / subagents / no-ask-user",
                profile: AgentDefinition::default_grow_build,
                subagents: true,
                ask_user: false,
            },
            PagerFlagCase {
                label: "grow-build / no-subagents / ask_user",
                profile: AgentDefinition::default_grow_build,
                subagents: false,
                ask_user: true,
            },
            PagerFlagCase {
                label: "grow-build / no-subagents / no-ask-user",
                profile: AgentDefinition::default_grow_build,
                subagents: false,
                ask_user: false,
            },
        ];
        for case in cases {
            let PagerFlagCase {
                label,
                profile,
                subagents,
                ask_user,
            } = case;
            let agent = build_pager_agent(profile(), *subagents, *ask_user).await;
            let defs = agent.tool_definitions().await;
            let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
            let mut counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for n in &names {
                *counts.entry(*n).or_default() += 1;
            }
            let dupes: Vec<(&&str, &usize)> = counts.iter().filter(|(_, c)| **c > 1).collect();
            assert!(
                dupes.is_empty(),
                "[{label}] tool names must be unique, found duplicates: {dupes:?}; full list: {names:?}"
            );
            let has_ask_user = names.contains(&"ask_user_question");
            assert_eq!(
                has_ask_user, *ask_user,
                "[{label}] ask_user_question presence should match ask_user_question_enabled={ask_user}; got tools: {names:?}"
            );
            let has_task = names.contains(&"spawn_subagent");
            assert_eq!(
                has_task, *subagents,
                "[{label}] spawn_subagent presence should match subagents_enabled={subagents}; got tools: {names:?}"
            );
            assert!(
                names.contains(&"plan_control"),
                "[{label}] plan_control must always be present; got tools: {names:?}"
            );
        }
    }
    #[tokio::test]
    async fn curated_empty_toolset_fails_agent_build() {
        use tools::computer::local::LocalTerminalBackend;
        use tools::notification::ToolNotificationHandle;
        let mut profile = crate::config::AgentDefinition::default_grow_build();
        profile.tool_config = Default::default();
        profile.inject_default_tools = false;
        let result = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(profile)
        .build()
        .await;
        match result {
            Ok(_) => panic!("empty curated toolset must be rejected at build time"),
            Err(err) => {
                assert!(
                    matches!(err, AgentBuildError::InvalidConfig(_)),
                    "expected InvalidConfig, got: {err:?}"
                )
            }
        }
    }
    /// The ask_user_question params merge must run after `ensure_plan_mode_tools`:
    /// a profile that does NOT pre-declare the tool still gets the shell-resolved
    /// timeout params on the injected instance. Fails if the merge is ever
    /// hoisted above the injection.
    #[tokio::test]
    async fn plan_mode_injected_ask_user_question_receives_params() {
        use tools::computer::local::LocalTerminalBackend;
        use tools::implementations::grow_build::ask_user_question::AskUserQuestionParams;
        use tools::notification::ToolNotificationHandle;
        use tools::types::resources::Params;
        let profile = crate::config::AgentDefinition::default_grow_build();
        assert!(
            !profile
                .tool_config
                .tools
                .iter()
                .any(|tc| tc.id == "Grow:ask_user_question"),
            "test premise: the profile must not pre-declare ask_user_question"
        );
        let mut params = serde_json::Map::new();
        params.insert("timeout_enabled".into(), serde_json::Value::Bool(false));
        params.insert("timeout_secs".into(), serde_json::Value::from(5));
        let agent = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(profile)
        .with_ask_user_question_params(params)
        .build()
        .await
        .expect("agent should build");
        let applied = agent
            .tool_bridge()
            .read_resource::<Params<AskUserQuestionParams>>()
            .await
            .expect("finalize must insert Params for the injected ask_user_question");
        assert!(!applied.0.timeout_enabled);
        assert_eq!(applied.0.timeout_secs.get(), 5);
    }
    async fn build_with_tools(tools: Vec<String>, disallowed: Vec<String>) -> crate::agent::Agent {
        use tools::computer::local::LocalTerminalBackend;
        use tools::notification::ToolNotificationHandle;
        let mut def = crate::config::AgentDefinition::default_grow_build();
        def.tools = tools;
        def.disallowed_tools = disallowed;
        AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(def)
        .build()
        .await
        .unwrap()
    }
    /// Build a default agent under a session allowlist + the agent's own
    /// allowlist, returning the effective (short) tool names.
    async fn session_clamp_tool_names(
        own_tools: Vec<String>,
        session_allow: Vec<String>,
    ) -> Vec<String> {
        use tools::computer::local::LocalTerminalBackend;
        use tools::notification::ToolNotificationHandle;
        let mut def = crate::config::AgentDefinition::default_grow_build();
        def.tools = own_tools;
        def.session_tools_allowlist = Some(session_allow);
        let agent = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(def)
        .build()
        .await
        .unwrap();
        agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect()
    }
    /// The session clamp and the agent's own allowlist both bind the effective
    /// toolset (intersection), and a disjoint pair yields deny-all — never the
    /// "empty list == inherit all" collapse.
    #[tokio::test]
    async fn session_clamp_intersects_own_allowlist() {
        let has = |v: &[String], t: &str| v.iter().any(|n| n == t);
        let names = session_clamp_tool_names(vec![], vec!["read_file".into(), "grep".into()]).await;
        assert!(has(&names, "read_file") && has(&names, "grep"), "{names:?}");
        assert!(!has(&names, "run_terminal_cmd"), "{names:?}");
        let names = session_clamp_tool_names(
            vec!["read_file".into(), "search_replace".into()],
            vec!["read_file".into(), "grep".into()],
        )
        .await;
        assert!(has(&names, "read_file"), "{names:?}");
        assert!(
            !has(&names, "search_replace"),
            "session denies it: {names:?}"
        );
        assert!(!has(&names, "grep"), "own allowlist denies it: {names:?}");
        let names = session_clamp_tool_names(
            vec!["search_replace".into()],
            vec!["read_file".into(), "grep".into()],
        )
        .await;
        assert!(
            !has(&names, "search_replace"),
            "session denies it: {names:?}"
        );
        assert!(
            !has(&names, "read_file"),
            "own allowlist denies it: {names:?}"
        );
        assert!(
            !has(&names, "run_terminal_cmd"),
            "must not inherit-all: {names:?}"
        );
    }
    /// An unresolved own-allowlist entry grants nothing; resolved entries and
    /// the session clamp still intersect normally.
    #[tokio::test]
    async fn session_clamp_intersects_partially_unresolved_own_allowlist() {
        let names = session_clamp_tool_names(
            vec!["read_file".into(), "bogus_unresolved_xyz".into()],
            vec!["read_file".into()],
        )
        .await;
        assert!(names.iter().any(|n| n == "read_file"), "{names:?}");
        assert!(
            !names.iter().any(|n| n == "run_terminal_cmd"),
            "session clamp must bind despite the step-4 full-toolset fallback: {names:?}"
        );
    }
    #[test]
    fn session_tools_allowed_clamp() {
        let mut def = crate::config::AgentDefinition::general_purpose();
        let read = tools::registry::types::ToolConfig::from_id("Grow:read_file");
        let grep = tools::registry::types::ToolConfig::from_id("Grow:grep");
        assert!(def.session_tools_allowed(&read));
        def.session_tools_allowlist = Some(vec!["read_file".into()]);
        assert!(def.session_tools_allowed(&read));
        assert!(!def.session_tools_allowed(&grep));
        def.session_tools_denylist = Some(vec!["read_file".into()]);
        assert!(!def.session_tools_allowed(&read));
    }
    const AGENT_TOOLS_BASE: &[&str] = &["read_file", "run_terminal_cmd"];
    #[tokio::test]
    async fn explicit_tool_list_without_agent_disables_task_tools() {
        let mut tools: Vec<String> = AGENT_TOOLS_BASE.iter().map(|s| s.to_string()).collect();
        tools.push("grep".into());
        let agent = build_with_tools(tools, vec![]).await;
        let names: Vec<_> = agent
            .tool_definitions()
            .await
            .into_iter()
            .map(|definition| definition.function.name)
            .collect();
        assert!(!names.iter().any(|name| name == "task"));
    }
    #[tokio::test]
    async fn explicit_subagent_allowlist_filters_task_description() {
        use tools::computer::local::LocalTerminalBackend;
        use tools::notification::ToolNotificationHandle;
        let mut tools: Vec<String> = AGENT_TOOLS_BASE.iter().map(|s| s.to_string()).collect();
        tools.push("spawn_subagent".into());
        tools.extend(
            ["get_command_or_subagent_output", "kill_command_or_subagent"].map(str::to_owned),
        );
        let mut definition = crate::config::AgentDefinition::default_grow_build();
        definition.tools = tools;
        definition.subagents.allow = vec!["explore".into()];
        let agent = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(definition)
        .with_subagents_enabled(true)
        .build()
        .await
        .expect("agent should build");
        let task = agent
            .tool_definitions()
            .await
            .into_iter()
            .find(|definition| definition.function.name == "spawn_subagent")
            .expect("explicit subagent policy must retain the task tool");
        let description = task
            .function
            .description
            .expect("task tool must have a description");
        assert!(description.contains("**explore**"));
        assert!(!description.contains("**plan**"));
        assert!(!description.contains("**general-purpose**"));
    }
    #[tokio::test]
    async fn spawning_blocked_disables_all_background_bash_modes() {
        use tools::computer::local::LocalTerminalBackend;
        use tools::implementations::grow_build::bash::BashParams;
        use tools::notification::ToolNotificationHandle;
        use tools::types::resources::Params;
        let mut definition = crate::config::AgentDefinition::default_grow_build();
        definition.tools = vec!["run_terminal_cmd".into()];
        let bash_params = serde_json::json!({
            "max_timeout_secs": 36_000.0,
            "auto_background_on_timeout": true,
            "allow_background_operator": false,
        })
        .as_object()
        .unwrap()
        .clone();
        let agent = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(definition)
        .with_bash_params(bash_params)
        .build()
        .await
        .expect("spawning-blocked agent should normalize background bash params");
        let applied = agent
            .tool_bridge()
            .read_resource::<Params<BashParams>>()
            .await
            .expect("bash params should be registered");
        assert!(!applied.0.enabled_background);
        assert!(!applied.0.auto_background_on_timeout);
        assert_eq!(applied.0.max_timeout_secs, Some(36_000.0));
        assert!(!applied.0.allow_background_operator);
    }
    /// Canonical tool names produce a real restricted toolset.
    #[tokio::test]
    async fn canonical_tool_names_restrict_the_toolset() {
        let tools = vec![
            "read_file".into(),
            "run_terminal_command".into(),
            "grep".into(),
        ];
        let agent = build_with_tools(tools, vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(
            names.contains(&"read_file".to_string()),
            "read_file missing; got: {names:?}"
        );
        assert!(
            names.contains(&"run_terminal_command".to_string()),
            "run_terminal_command missing; got: {names:?}"
        );
        assert!(
            names.contains(&"grep".to_string()),
            "grep missing; got: {names:?}"
        );
        assert!(
            !names.contains(&"search_replace".to_string()),
            "search_replace must be excluded by the allowlist; got: {names:?}"
        );
    }
    /// A canonical allowlist builds end-to-end: the allowlist is
    /// honored (no full-toolset fallback) and `ask_user_question` stands
    /// alone after the injected plan-mode tools are dropped.
    #[tokio::test]
    async fn ask_user_question_allowlist_builds_without_plan_tools() {
        let tools = vec![
            "read_file".into(),
            "search_replace".into(),
            "ask_user_question".into(),
        ];
        let agent = build_with_tools(tools, vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        for kept in ["read_file", "search_replace", "ask_user_question"] {
            assert!(names.contains(&kept.to_string()), "got: {names:?}");
        }
        for dropped in ["plan_control", "run_terminal_command"] {
            assert!(!names.contains(&dropped.to_string()), "got: {names:?}");
        }
    }
    /// Entries we can't match or map (a typo, a renamed/absent tool) grant no
    /// capability. A malformed restricted Agent must fail closed.
    #[tokio::test]
    async fn unmappable_allowlist_fails_closed() {
        let tools = vec!["Frobnicate".into(), "Wibble".into()];
        let agent = build_with_tools(tools, vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(
            !names.contains(&"read_file".to_string())
                && !names.contains(&"search_replace".to_string())
                && !names.contains(&"run_terminal_command".to_string()),
            "unmappable entries must not widen authority: {names:?}"
        );
    }
    /// End-to-end: an on-disk plugin agent uses canonical tool names.
    #[tokio::test]
    async fn plugin_agent_file_uses_canonical_tools() {
        use tools::computer::local::LocalTerminalBackend;
        use tools::notification::ToolNotificationHandle;
        const MD: &str = "---\n\
            name: test\n\
            description: test agent\n\
            tools: read_file, run_terminal_command, grep\n\
            ---\n\n\
            Test agent body.\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, MD).unwrap();
        let def = crate::config::AgentDefinition::from_file_frontmatter_only(&path).unwrap();
        assert_eq!(
            def.tools,
            vec![
                "read_file".to_string(),
                "run_terminal_command".to_string(),
                "grep".to_string()
            ],
        );
        let agent = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(def)
        .build()
        .await
        .unwrap();
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(
            names.contains(&"read_file".to_string()),
            "read_file missing; got: {names:?}"
        );
        assert!(
            names.contains(&"run_terminal_command".to_string()),
            "run_terminal_command missing; got: {names:?}"
        );
        assert!(
            names.contains(&"grep".to_string()),
            "grep missing; got: {names:?}"
        );
        assert!(
            !names.contains(&"search_replace".to_string()),
            "search_replace must be excluded; got: {names:?}"
        );
    }
    /// A restrictive allowlist must never strip MCP access. Grow keeps the MCP meta-tools
    /// (`search_tool` / `use_tool`) regardless of what the allowlist names.
    #[tokio::test]
    async fn restrictive_allowlist_keeps_mcp_access() {
        let agent = build_with_tools(vec!["read_file".into()], vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(
            names.contains(&"read_file".to_string()),
            "read_file missing; got: {names:?}"
        );
        assert!(
            names.contains(&"search_tool".to_string()) && names.contains(&"use_tool".to_string()),
            "search_tool/use_tool (MCP access) must not be stripped; got: {names:?}"
        );
        assert!(
            !names.contains(&"search_replace".to_string()),
            "search_replace must be excluded; got: {names:?}"
        );
    }
    #[tokio::test]
    async fn disabled_web_fetch_does_not_fall_back() {
        let tools = vec![
            "read_file".into(),
            "grep".into(),
            "list_dir".into(),
            "web_fetch".into(),
        ];
        let agent = build_with_tools(tools, vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(!names.contains(&"web_fetch".to_string()), "got: {names:?}");
        for kept in ["read_file", "grep", "list_dir"] {
            assert!(names.contains(&kept.to_string()), "got: {names:?}");
        }
        for excluded in ["run_terminal_command", "search_replace"] {
            assert!(!names.contains(&excluded.to_string()), "got: {names:?}");
        }
    }
    #[tokio::test]
    async fn requested_enabled_web_fetch_survives_allowlist() {
        use tools::computer::local::LocalTerminalBackend;
        use tools::implementations::grow_build::web_fetch::WebFetchConfig;
        use tools::notification::ToolNotificationHandle;
        let mut definition = crate::config::AgentDefinition::default_grow_build();
        definition.tools = vec![
            "read_file".into(),
            "grep".into(),
            "list_dir".into(),
            "web_fetch".into(),
        ];
        let agent = AgentBuilder::new(
            std::env::temp_dir(),
            Arc::new(LocalTerminalBackend::new()),
            ToolNotificationHandle::noop(),
        )
        .from_definition(definition)
        .with_web_fetch_config(WebFetchConfig::Enabled {
            params: Default::default(),
        })
        .build()
        .await
        .expect("agent should build with requested web tools");
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        for kept in ["read_file", "grep", "list_dir", "web_fetch"] {
            assert!(names.contains(&kept.to_string()), "got: {names:?}");
        }
        for excluded in ["run_terminal_command", "search_replace"] {
            assert!(!names.contains(&excluded.to_string()), "got: {names:?}");
        }
    }
    /// A qualified `mcp__server__tool` allowlist entry is always allowed: it
    /// neither triggers the full-toolset fallback nor strips MCP access.
    #[tokio::test]
    async fn mcp_prefixed_allowlist_entry_keeps_mcp_access() {
        let tools = vec!["mcp__github__create_issue".into(), "read_file".into()];
        let agent = build_with_tools(tools, vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(
            names.contains(&"read_file".to_string()),
            "read_file must be kept; got: {names:?}"
        );
        assert!(
            names.contains(&"search_tool".to_string()) && names.contains(&"use_tool".to_string()),
            "MCP access must be kept; got: {names:?}"
        );
        assert!(
            !names.contains(&"search_replace".to_string())
                && !names.contains(&"run_terminal_command".to_string()),
            "no full-toolset fallback — unlisted tools must be excluded; got: {names:?}"
        );
    }
    /// The canonical MCP search meta-tool remains available without widening
    /// the rest of the toolset.
    #[tokio::test]
    async fn search_tool_allowlist_keeps_search_tool() {
        let agent = build_with_tools(vec!["search_tool".into()], vec![]).await;
        let names: Vec<String> = agent
            .tool_definitions()
            .await
            .iter()
            .map(|d| d.function.name.clone())
            .collect();
        assert!(
            names.contains(&"search_tool".to_string()),
            "search_tool missing; got: {names:?}"
        );
        assert!(
            !names.contains(&"search_replace".to_string()),
            "no full-toolset fallback — search_replace must be excluded; got: {names:?}"
        );
    }
}
