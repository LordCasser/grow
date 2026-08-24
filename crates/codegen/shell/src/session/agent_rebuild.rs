//! `AgentRebuildSpec` — the canonical recipe for constructing an
//! [`agent::Agent`] for a given session.
//!
//! INVARIANT: This is the **only** place in the shell crate that calls
//! [`agent::AgentBuilder::new`]. Both initial session spawn
//! ([`crate::session::acp_session::spawn_session_actor`]) and zero-turn
//! harness rebuild
//! ([`crate::session::acp_session::SessionActor::handle_rebuild_agent_for_definition`])
//! go through [`AgentRebuildSpec::build_agent`].
//!
//! ## Why this exists
//!
//! [`agent::Agent`] owns an [`tools::bridge::ToolBridge`]
//! that carries session-scoped channels (notification handle, terminal/fs
//! backends, subagent senders, scheduler set, and plugin registry). The Agent
//! is therefore session-bound — it cannot be shared
//! across sessions and cannot be re-rendered from outside its session
//! context. To rebuild it (e.g. when the user picks a model with a
//! different `agent_type` before sending any user message), we need to
//! retain every input that the original `AgentBuilder` chain consumed.
//! `AgentRebuildSpec` is exactly that retained bag of inputs.
//!
//! ## WHEN ADDING A NEW [`agent::AgentBuilder`]`::with_*` KNOB
//!
//! 1. Add the corresponding field to [`AgentRebuildSpec`].
//! 2. Pass it through in [`AgentRebuildSpec::build_agent`]. The destructure
//!    pattern at the top of `build_agent` forces every field to be used —
//!    drift is a compile error (`#[deny(unused_variables)]`).
//! 3. Populate the field at the call site in `spawn_session_actor`.
//!
//! ## Why some fields are channel senders
//!
//! Several `ToolBridge` resources (e.g. `UserQuestionSender`,
//! `SubagentBackendResource`) are backed by the `tx` half of channels
//! whose `rx` halves are owned by long-lived coordinator tasks spawned
//! in `spawn_session_actor`. The subagent channels are wrapped in a
//! `ChannelBackend` behind `SubagentBackendResource`. On rebuild, we
//! must reuse the **same** senders so the existing coordinator keeps
//! receiving requests; we cannot mint a fresh channel without orphaning
//! the running coordinator.
use agent::config::AgentDefinition;
use agent::error::AgentBuildError;
use agent::prompt::context::PromptAudience;
use agent::prompt::skills::SkillsConfig;
use agent::{Agent, AgentBuilder, CompactionPolicy, ReminderPolicy};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tools::computer::types::{AsyncFileSystem, TerminalBackend};
use tools::implementations::grow_build::ask_user_question::types::UserQuestionRequest;
use tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig;
use tools::implementations::grow_build::monitor::types::MonitorEventBuffer;
use tools::implementations::grow_build::task::types::{SubagentEvent, TaskModelValidator};
use tools::implementations::grow_build::web_fetch::WebFetchConfig;
use tools::implementations::lsp::LspBackend;
use tools::notification::ToolNotificationHandle;
use tools::types::memory_backend::MemoryBackend;
/// Shell-resolved per-tool `ToolConfig.params` JSON maps, bundled into one
/// named struct so the spawn telescopes carry a single argument instead of
/// adjacent identically-typed positionals that a caller could transpose.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedToolParamsJson {
    /// `[toolset.bash]` overrides for the bash tool(s).
    pub bash: Option<serde_json::Map<String, serde_json::Value>>,
    /// `[toolset.ask_user_question]` timeout policy for the ask tool.
    pub ask_user_question: Option<serde_json::Map<String, serde_json::Value>>,
}
/// Cached recipe for building a session-scoped [`Agent`].
///
/// See module docs for the invariant: this is the only construction
/// site for `Agent` in the shell crate. Cloning is intentionally not
/// derived — the spec lives behind an [`Arc`] and is shared by clone of
/// that `Arc`.
pub(crate) struct AgentRebuildSpec {
    pub working_directory: PathBuf,
    pub terminal_backend: Arc<dyn TerminalBackend>,
    pub fs_backend: Arc<dyn AsyncFileSystem>,
    pub tools_notification_handle: ToolNotificationHandle,
    pub resources_persistence: Arc<tools::persistence::ResourcesPersistence>,
    pub session_env: Arc<HashMap<String, String>>,
    pub models_manager: crate::agent::models::ModelsManager,
    pub compaction_policy: CompactionPolicy,
    pub reminder_policy: ReminderPolicy,
    pub memory_enabled: bool,
    pub memory_backend: Option<Arc<dyn MemoryBackend>>,
    pub context_recall_backend:
        Arc<dyn tools::implementations::context_recall::ContextRecallBackend>,
    pub web_fetch_config: WebFetchConfig,
    pub app_builder_deployer_config: AppBuilderDeployerConfig,
    pub write_file_enabled: bool,
    pub subagents_enabled: bool,
    pub subagent_toggle: HashMap<String, bool>,
    pub background_workflows_enabled: bool,
    pub ask_user_question_enabled: bool,
    pub prompt_audience: PromptAudience,
    pub skills_config: SkillsConfig,
    pub context_window_tokens: u64,
    pub prompt_working_directory: Option<String>,
    pub lsp: Option<Arc<dyn LspBackend>>,
    pub plugin_registry: Option<Arc<agent::plugins::PluginRegistry>>,
    pub tool_params_json: ResolvedToolParamsJson,
    pub subagent_event_tx: Option<UnboundedSender<SubagentEvent>>,
    pub monitor_event_buffer: Option<MonitorEventBuffer>,
    pub user_question_tx: UnboundedSender<UserQuestionRequest>,
    pub subagent_depth: u32,
    pub subagents_max_depth: u32,
    pub session_id_str: String,
    pub blocking_wait_depth: Arc<crate::tools::tool_context::BlockingWaitState>,
    pub respect_gitignore: bool,
    pub path_not_found_hints: bool,
    pub is_non_interactive: bool,
    pub system_prompt_label: String,
    pub owner_session_id: Option<String>,
    pub parent_scheduler_handle:
        Option<tools::implementations::grow_build::scheduler::types::SchedulerHandle>,
}
impl AgentRebuildSpec {
    /// Build a fresh [`Agent`] from this spec and an [`AgentDefinition`].
    ///
    /// This is the canonical construction path; see module docs for the
    /// invariant. The destructure pattern below is intentional —
    /// `#[deny(unused_variables)]` ensures any newly added spec field is
    /// used here, otherwise compilation fails.
    #[deny(unused_variables)]
    pub async fn build_agent(
        self: &Arc<Self>,
        definition: AgentDefinition,
    ) -> Result<Agent, AgentBuildError> {
        self.build_agent_inner(definition, None, None).await
    }
    /// Build an agent with optional one-shot overrides for initial spawn.
    ///
    /// `persisted_skill_names`: restored into the `SkillManager` before
    /// `seed()` to prevent duplicate system-reminder injection on resume.
    ///
    /// `preloaded_skills`: parent-discovered skills passed to
    /// `AgentBuilder::with_preloaded_skills()` to bypass filesystem
    /// discovery in subagents.
    ///
    /// Both are consumed once — the rebuild path (`build_agent`) passes
    /// `None` for both so zero-turn model switches get fresh discovery.
    pub async fn build_agent_with_initial_overrides(
        self: &Arc<Self>,
        definition: AgentDefinition,
        persisted_skill_names: Option<std::collections::HashSet<String>>,
        preloaded_skills: Option<Vec<tools::implementations::skills::types::SkillInfo>>,
    ) -> Result<Agent, AgentBuildError> {
        self.build_agent_inner(definition, persisted_skill_names, preloaded_skills)
            .await
    }
    #[deny(unused_variables)]
    async fn build_agent_inner(
        self: &Arc<Self>,
        definition: AgentDefinition,
        persisted_skill_names: Option<std::collections::HashSet<String>>,
        preloaded_skills: Option<Vec<tools::implementations::skills::types::SkillInfo>>,
    ) -> Result<Agent, AgentBuildError> {
        let Self {
            working_directory,
            terminal_backend,
            fs_backend,
            tools_notification_handle,
            resources_persistence,
            session_env,
            models_manager,
            compaction_policy,
            reminder_policy,
            memory_enabled,
            memory_backend,
            context_recall_backend,
            web_fetch_config,
            app_builder_deployer_config,
            write_file_enabled,
            subagents_enabled,
            subagent_toggle,
            background_workflows_enabled,
            ask_user_question_enabled,
            prompt_audience,
            skills_config,
            context_window_tokens,
            prompt_working_directory,
            lsp,
            plugin_registry,
            tool_params_json,
            subagent_event_tx,
            monitor_event_buffer,
            user_question_tx,
            subagent_depth,
            subagents_max_depth,
            session_id_str,
            blocking_wait_depth,
            respect_gitignore,
            path_not_found_hints,
            is_non_interactive,
            system_prompt_label,
            owner_session_id,
            parent_scheduler_handle,
        } = self.as_ref();
        let mut builder = AgentBuilder::new(
            working_directory.clone(),
            terminal_backend.clone(),
            tools_notification_handle.clone(),
        )
        .from_definition(definition)
        .with_compaction_policy(compaction_policy.clone())
        .with_reminder_policy(reminder_policy.clone())
        .with_memory_enabled(*memory_enabled)
        .with_is_non_interactive(*is_non_interactive)
        .with_system_prompt_label(system_prompt_label.clone())
        .with_session_env(session_env.clone())
        .with_resources_persistence(resources_persistence.clone())
        .with_app_builder_deployer_config(app_builder_deployer_config.clone())
        .with_web_fetch_config(web_fetch_config.clone())
        .with_write_file_enabled(*write_file_enabled)
        .with_fs(fs_backend.clone())
        .with_subagents_enabled(*subagents_enabled)
        .with_subagent_toggle(subagent_toggle.clone())
        .with_background_workflows_enabled(*background_workflows_enabled)
        .with_task_model_slugs(
            models_manager
                .available()
                .keys()
                .map(|model_id| model_id.0.to_string())
                .collect::<Vec<_>>(),
        )
        .with_ask_user_question_enabled(*ask_user_question_enabled)
        .with_prompt_audience(*prompt_audience)
        .with_skills_config(skills_config.clone())
        .with_context_window(*context_window_tokens)
        .with_mcp_max_output_bytes(
            crate::util::config::resolve_max_mcp_output_bytes_for_cwd(working_directory),
        );
        if let Some(owner_session_id) = owner_session_id.clone() {
            builder = builder.with_owner_session_id(owner_session_id);
        }
        if let Some(handle) = parent_scheduler_handle.clone() {
            builder = builder.with_parent_scheduler_handle(handle);
        }
        if let Some(memory_backend) = memory_backend.clone() {
            builder = builder.with_memory_backend(memory_backend);
        }
        if let Some(lsp) = lsp.clone() {
            builder = builder.with_lsp(lsp);
        }
        if let Some(plugin_registry) = plugin_registry.clone() {
            builder = builder.with_plugin_registry(plugin_registry);
        }
        if let Some(bash_params_json) = tool_params_json.bash.clone() {
            builder = builder.with_bash_params(bash_params_json);
        }
        if let Some(ask_user_question_params_json) = tool_params_json.ask_user_question.clone() {
            builder = builder.with_ask_user_question_params(ask_user_question_params_json);
        }
        if let Some(prompt_working_directory) = prompt_working_directory.clone() {
            builder = builder.with_prompt_working_directory(prompt_working_directory);
        }
        if let Some(names) = persisted_skill_names {
            builder = builder.with_persisted_announced_skill_names(names);
        }
        if let Some(skills) = preloaded_skills {
            builder = builder.with_preloaded_skills(skills);
        }
        let agent = builder.build().await?;
        agent
            .tool_bridge()
            .update_resource(context_recall_backend.clone())
            .await;
        let model_validator = models_manager.clone();
        agent
            .tool_bridge()
            .update_resource(TaskModelValidator::new(move |requested| {
                model_validator.task_model_error(requested)
            }))
            .await;
        if let Some(event_tx) = subagent_event_tx.clone() {
            use tools::implementations::grow_build::task::backend::{
                ChannelBackend, SubagentBackendResource,
            };
            use tools::implementations::grow_build::task::types::{
                MaxSubagentDepth, SessionIdResource, SubagentDepthCounter, SubagentEventSender,
            };
            let backend = SubagentBackendResource(Arc::new(ChannelBackend::for_session(
                event_tx.clone(),
                session_id_str.clone(),
            )));
            agent.tool_bridge().update_resource(backend).await;
            agent
                .tool_bridge()
                .update_resource(SubagentDepthCounter(*subagent_depth))
                .await;
            agent
                .tool_bridge()
                .update_resource(MaxSubagentDepth(*subagents_max_depth))
                .await;
            agent
                .tool_bridge()
                .update_resource(SessionIdResource(session_id_str.clone()))
                .await;
            agent
                .tool_bridge()
                .update_resource(SubagentEventSender(event_tx))
                .await;
            agent
                .tool_bridge()
                .update_resource(crate::tools::tool_context::subagent_foreground_wait(
                    Arc::clone(blocking_wait_depth),
                ))
                .await;
            if let Some(buffer) = monitor_event_buffer.clone() {
                agent.tool_bridge().update_resource(buffer).await;
            }
        }
        agent
            .tool_bridge()
            .update_resource(tools::types::resources::RespectGitignore(
                *respect_gitignore,
            ))
            .await;
        agent
            .tool_bridge()
            .update_resource(tools::types::resources::PathNotFoundHints(
                *path_not_found_hints,
            ))
            .await;
        {
            use tools::implementations::grow_build::ask_user_question::UserQuestionSender;
            agent
                .tool_bridge()
                .update_resource(UserQuestionSender(user_question_tx.clone()))
                .await;
        }
        Ok(agent)
    }
}
/// Build a stub [`AgentRebuildSpec`] for unit tests.
///
/// Every field is set to a minimal default suitable for test `SessionActor`
/// literals and focused `build_agent` tests.
#[cfg(test)]
pub(crate) fn test_rebuild_spec_default() -> Arc<AgentRebuildSpec> {
    let (uq_tx, _uq_rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(AgentRebuildSpec {
        working_directory: std::env::temp_dir(),
        terminal_backend: Arc::new(tools::computer::local::LocalTerminalBackend::new_local(
            tools::computer::local::SearchShadowConfig::default(),
        )),
        fs_backend: Arc::new(tools::computer::local::LocalFs),
        tools_notification_handle: ToolNotificationHandle::noop(),
        resources_persistence: Arc::new(tools::persistence::ResourcesPersistence::noop()),
        session_env: Arc::new(HashMap::new()),
        models_manager: crate::agent::models::ModelsManager::default(),
        compaction_policy: CompactionPolicy::default(),
        reminder_policy: ReminderPolicy::default(),
        memory_enabled: false,
        memory_backend: None,
        context_recall_backend: crate::session::context_recall::context_recall_channel().0,
        web_fetch_config: WebFetchConfig::Disabled,
        app_builder_deployer_config: AppBuilderDeployerConfig::default(),
        write_file_enabled: true,
        subagents_enabled: false,
        subagent_toggle: HashMap::new(),
        background_workflows_enabled: false,
        ask_user_question_enabled: true,
        prompt_audience: PromptAudience::Primary,
        skills_config: SkillsConfig::default(),
        context_window_tokens: 256_000,
        prompt_working_directory: None,
        lsp: None,
        plugin_registry: None,
        tool_params_json: ResolvedToolParamsJson::default(),
        subagent_event_tx: None,
        monitor_event_buffer: None,
        user_question_tx: uq_tx,
        subagent_depth: 0,
        subagents_max_depth: tools::implementations::grow_build::task::MAX_SUBAGENT_DEPTH,
        session_id_str: "test-session".to_string(),
        blocking_wait_depth: Arc::new(crate::tools::tool_context::BlockingWaitState::new()),
        respect_gitignore: false,
        path_not_found_hints: false,
        is_non_interactive: false,
        system_prompt_label: agent::DEFAULT_SYSTEM_PROMPT_LABEL.to_string(),
        owner_session_id: Some("test-session".to_string()),
        parent_scheduler_handle: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::config::{EndpointsConfig, ModelEntry};
    fn model_entry(internal_id: &str) -> ModelEntry {
        ModelEntry::baseline(internal_id)
    }
    fn task_description(agent: &Agent) -> String {
        let toolset = agent.tool_bridge().toolset();
        let task_name = toolset
            .tool_name_for_kind(tools::types::tool::ToolKind::Task)
            .expect("Grow Task tool should be present");
        toolset
            .tool_definitions()
            .into_iter()
            .find(|definition| definition.function.name == task_name)
            .and_then(|definition| definition.function.description)
            .expect("Grow Task description should be present")
    }
    #[tokio::test(flavor = "current_thread")]
    async fn rebuild_projects_fresh_public_model_keys_into_task_description() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut spec = test_rebuild_spec_default();
                Arc::get_mut(&mut spec)
                    .expect("test rebuild spec should be uniquely owned")
                    .subagents_enabled = true;
                let models_manager = spec.models_manager.clone();
                models_manager
                    .insert_test_entry("zeta-public", model_entry("internal-zeta"));
                models_manager
                    .insert_test_entry("alpha-public", model_entry("internal-alpha"));
                let mut hidden = model_entry("internal-hidden");
                hidden.info.hidden = true;
                models_manager.insert_test_entry("private-hidden-model", hidden);
                let mut unselectable = model_entry("internal-unselectable");
                unselectable.info.user_selectable = false;
                models_manager
                    .insert_test_entry("private-unselectable-model", unselectable);
                let first = spec
                    .build_agent(AgentDefinition::default_grow_build())
                    .await
                    .expect("first agent build should succeed");
                let first_description = task_description(&first);
                assert!(
                    first_description.contains(
                        "If the user explicitly asks for the model of a subagent/task, you may ONLY use model slugs from this list:\n\
                         - alpha-public\n\
                         - zeta-public"
                    )
                );
                assert!(!first_description.contains("private-hidden-model"));
                assert!(!first_description.contains("private-unselectable-model"));
                assert!(!first_description.contains("internal-alpha"));
                let validator = first
                    .tool_bridge()
                    .toolset()
                    .get_resource_cloned::<TaskModelValidator>()
                    .await
                    .expect("Task model validator should be registered");
                assert!(validator.error_for("alpha-public").is_none());
                assert!(validator.error_for("private-hidden-model").is_some());
                models_manager
                    .insert_test_entry("beta-public", model_entry("internal-beta"));
                assert!(validator.error_for("beta-public").is_none());
                let rebuilt = spec
                    .build_agent(AgentDefinition::default_grow_build())
                    .await
                    .expect("rebuilt agent should succeed");
                let rebuilt_description = task_description(&rebuilt);
                assert!(
                    rebuilt_description.contains(
                        "If the user explicitly asks for the model of a subagent/task, you may ONLY use model slugs from this list:\n\
                         - alpha-public\n\
                         - beta-public\n\
                         - zeta-public"
                    )
                );
            })
            .await;
    }
}
