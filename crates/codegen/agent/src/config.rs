//! Agent definition types — parsed from `.grow/agents/*.md` files.
use crate::error::AgentBuildError;
use crate::prompt::context::TemplateOverride;
use crate::prompt::user_message::UserMessageTemplate;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use strum::{AsRefStr, Display, EnumIter, EnumString, IntoStaticStr};
use tools::implementations::grow_build;
use tools::implementations::grow_build_concise;
use tools::implementations::memory;
use tools::implementations::search_tool;
use tools::implementations::use_tool;
use tools::registry::types::{ToolConfig, ToolServerConfig};
/// Process-global registry of externally-provided toolset presets.
///
/// # Visibility
/// Each preset is registered as either **public** or **internal**:
/// - **Public** presets are product presets: they are enumerated by
///   [`preset_names`] / [`all_toolset_presets`] (so they appear in the
///   workspace manifest, preset sets, etc.) *and* resolvable via
///   [`toolset_for_preset`].
/// - **Internal** presets are resolved by name at runtime by the shell's Agent
///   spawn path via [`toolset_for_preset`], but are deliberately
///   NOT enumerated, so a harness-internal preset never leaks into public
///   preset enumeration.
///
/// # Ordering contract
/// [`register_toolset_preset`] / [`register_internal_toolset_preset`] MUST run
/// before the first preset resolution in the process. Presets registered later
/// are still visible to subsequent `toolset_for_preset` / `preset_names` /
/// `all_toolset_presets` calls, but any config resolved before registration
/// will not see them.
/// A toolset preset builder: a function producing a [`ToolServerConfig`].
pub type ToolsetPresetBuilder = fn() -> ToolServerConfig;
/// Whether a registered preset is enumerated publicly or resolved by name only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PresetVisibility {
    /// A product preset: enumerated by `preset_names()` / `all_toolset_presets()`.
    Public,
    /// A harness-internal preset: resolvable by name but never enumerated.
    Internal,
}
static TOOLSET_PRESETS: OnceLock<Mutex<HashMap<String, (ToolsetPresetBuilder, PresetVisibility)>>> =
    OnceLock::new();
fn toolset_preset_registry()
-> &'static Mutex<HashMap<String, (ToolsetPresetBuilder, PresetVisibility)>> {
    TOOLSET_PRESETS.get_or_init(|| Mutex::new(HashMap::new()))
}
/// Register an out-of-tree **public** (product) toolset preset by name. Public
/// presets are enumerated by [`preset_names`] / [`all_toolset_presets`] and
/// resolvable via [`toolset_for_preset`]. See [`TOOLSET_PRESETS`].
pub fn register_toolset_preset(name: &str, builder: ToolsetPresetBuilder) {
    toolset_preset_registry()
        .lock()
        .expect("toolset preset registry poisoned")
        .insert(name.to_string(), (builder, PresetVisibility::Public));
}
/// Register an out-of-tree **internal** toolset preset by name. Internal presets
/// are resolvable via [`toolset_for_preset`] (the shell's Agent spawn path
/// resolves them by name) but are deliberately NOT enumerated by
/// [`preset_names`] / [`all_toolset_presets`], so they never leak into public
/// preset enumeration (manifest generation, product preset sets, …). See
/// [`TOOLSET_PRESETS`].
pub fn register_internal_toolset_preset(name: &str, builder: ToolsetPresetBuilder) {
    toolset_preset_registry()
        .lock()
        .expect("toolset preset registry poisoned")
        .insert(name.to_string(), (builder, PresetVisibility::Internal));
}
/// Look up an externally-registered toolset preset by (already-normalized) name.
/// Resolves BOTH public and internal presets.
fn registered_toolset_preset(name: &str) -> Option<ToolServerConfig> {
    toolset_preset_registry()
        .lock()
        .expect("toolset preset registry poisoned")
        .get(name)
        .map(|(f, _)| f())
}
/// Names of externally-registered **public** presets only (internal presets are
/// intentionally excluded from enumeration).
fn registered_public_toolset_preset_names() -> Vec<String> {
    toolset_preset_registry()
        .lock()
        .expect("toolset preset registry poisoned")
        .iter()
        .filter(|(_, (_, visibility))| *visibility == PresetVisibility::Public)
        .map(|(name, _)| name.clone())
        .collect()
}
/// Bash tool with clearer model-facing names:
/// `run_terminal_cmd` → `run_terminal_command`, `is_background` → `background`.
fn bash_tool_config() -> ToolConfig {
    ToolConfig::from(&grow_build::BashTool)
        .with_name("run_terminal_command")
        .with_param_rename("is_background", "background")
}
/// Task/subagent tool with clearer model-facing names:
/// `task` → `spawn_subagent`, `run_in_background` → `background`.
fn task_tool_config() -> ToolConfig {
    ToolConfig::from(&grow_build::TaskTool)
        .with_name("spawn_subagent")
        .with_param_rename("run_in_background", "background")
}
/// Task output tool renamed for clarity:
/// `get_task_output` → `get_command_or_subagent_output`.
fn task_output_tool_config() -> ToolConfig {
    ToolConfig::from(&grow_build::TaskOutputTool).with_name("get_command_or_subagent_output")
}
/// `wait_tasks` → `wait_commands_or_subagents`.
fn wait_tasks_tool_config() -> ToolConfig {
    ToolConfig::from(&grow_build::WaitTasksTool).with_name("wait_commands_or_subagents")
}
/// `kill_task` → `kill_command_or_subagent`.
fn kill_task_tool_config() -> ToolConfig {
    ToolConfig::from(&grow_build::KillTaskTool).with_name("kill_command_or_subagent")
}
/// Complete toolset executed by the local workspace.
///
/// Extends `default_grow_build_toolset()` with tools that are dynamically
/// injected by `AgentBuilder::build()` or only available in specific modes.
/// This includes tools dynamically injected by the agent builder.
pub fn workspace_grow_build_toolset() -> ToolServerConfig {
    let mut tools = default_grow_build_toolset().tools;
    tools.push((&grow_build::WriteTool).into());
    tools.push((&grow_build::PlanControlTool).into());
    tools.push((&grow_build::AskUserQuestionTool).into());
    tools.push((&grow_build::WebFetchTool).into());
    tools.push((&memory::search_tool::MemorySearchImpl).into());
    tools.push((&memory::get_tool::MemoryGetImpl).into());
    tools.push((&grow_build::LspTool).into());
    ToolServerConfig {
        tools,
        behavior_preset: None,
    }
}
/// Toolset for the `grow-computer` (workspace/sandbox) preset.
fn grow_computer_toolset() -> ToolServerConfig {
    #[allow(unused_mut)]
    let mut tools = vec![
        bash_tool_config(),
        (&grow_build::ReadFileTool).into(),
        (&grow_build::SearchReplaceTool).into(),
        (&grow_build::WriteTool).into(),
        (&grow_build::ListDirTool).into(),
        (&grow_build::GrepTool).into(),
        (&grow_build::KillTerminalCommandTool).into(),
        (&grow_build::GetTerminalCommandOutputTool).into(),
    ];
    ToolServerConfig {
        tools,
        behavior_preset: None,
    }
}
/// Every named toolset preset, as `(normalized_name, config)` pairs.
///
/// Single source of truth: [`toolset_for_preset`] resolves through this
/// table, and the preset-coverage tests iterate it, so a new preset is
/// automatically covered the moment it becomes resolvable.
/// Native (in-crate) toolset presets.
fn native_toolset_presets() -> Vec<(&'static str, ToolServerConfig)> {
    vec![
        // Runtime-only tools (Plan protocol, AskUser, web, memory, LSP, write
        // fallback) are injected later by AgentBuilder. Keeping the preset at
        // the declared base prevents toolPreset resolution from duplicating
        // those fixed runtime additions.
        ("grow-build", default_grow_build_toolset()),
        ("grow-build-concise", grow_build_concise_toolset()),
        ("explore", explore_toolset()),
        ("goal-planner", goal_planner_toolset()),
        ("goal-verifier", goal_verifier_toolset()),
        ("grow-computer", grow_computer_toolset()),
    ]
}
/// Every named **public** toolset preset (native + externally registered public
/// presets), as `(name, config)` pairs. Harness-internal registered presets are
/// intentionally excluded — resolve them by name via [`toolset_for_preset`].
fn all_toolset_presets() -> Vec<(String, ToolServerConfig)> {
    let mut out: Vec<(String, ToolServerConfig)> = native_toolset_presets()
        .into_iter()
        .map(|(name, cfg)| (name.to_string(), cfg))
        .collect();
    for name in registered_public_toolset_preset_names() {
        if !out.iter().any(|(n, _)| *n == name)
            && let Some(cfg) = registered_toolset_preset(&name)
        {
            out.push((name, cfg));
        }
    }
    out
}
/// Names of every named toolset preset (native first, then registered).
pub fn preset_names() -> Vec<String> {
    all_toolset_presets()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}
/// Resolve a named toolset preset to its [`ToolServerConfig`], or `None` if unknown.
pub fn toolset_for_preset(preset: &str) -> Option<ToolServerConfig> {
    let normalized = preset.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    native_toolset_presets()
        .into_iter()
        .find(|(name, _)| *name == normalized)
        .map(|(_, toolset)| toolset)
        .or_else(|| registered_toolset_preset(&normalized))
}
fn default_grow_build_toolset() -> ToolServerConfig {
    ToolServerConfig {
        tools: vec![
            bash_tool_config(),
            (&grow_build::ReadFileTool).into(),
            (&grow_build::SearchReplaceTool).into(),
            (&grow_build::ListDirTool).into(),
            (&grow_build::GrepTool).into(),
            kill_task_tool_config(),
            (&grow_build::TodoWriteTool).into(),
            task_output_tool_config(),
            wait_tasks_tool_config(),
            task_tool_config(),
            (&grow_build::SchedulerCreateTool).into(),
            (&grow_build::SchedulerDeleteTool).into(),
            (&grow_build::SchedulerListTool).into(),
            (&grow_build::MonitorTool).into(),
            (&search_tool::SearchTool).into(),
            (&use_tool::UseTool).into(),
            (&grow_build::GetGoalTool).into(),
            (&grow_build::UpdateGoalProgressTool).into(),
            (&grow_build::RequestGoalReplanTool).into(),
            (&grow_build::UpdateGoalTool).into(),
            (&grow_build::WorkflowTool).into(),
        ],
        behavior_preset: None,
    }
}
fn grow_build_concise_toolset() -> ToolServerConfig {
    ToolServerConfig {
        tools: vec![
            (&grow_build_concise::BashConciseTool).into(),
            (&grow_build_concise::ReadFileConciseTool).into(),
            (&grow_build_concise::SearchReplaceConciseTool).into(),
            (&grow_build::ListDirTool).into(),
            (&grow_build::GrepTool).into(),
            kill_task_tool_config(),
            (&grow_build::TodoWriteTool).into(),
            task_output_tool_config(),
            (&grow_build::SchedulerCreateTool).into(),
            (&grow_build::SchedulerDeleteTool).into(),
            (&grow_build::SchedulerListTool).into(),
            (&grow_build::MonitorTool).into(),
            (&grow_build::GetGoalTool).into(),
            (&grow_build::UpdateGoalProgressTool).into(),
            (&grow_build::RequestGoalReplanTool).into(),
            (&grow_build::UpdateGoalTool).into(),
            (&grow_build::WorkflowTool).into(),
        ],
        behavior_preset: None,
    }
}
/// Hashline toolset: anchor-based read/edit/search + standard utilities.
///
/// `hashline_tools` should be the 3 hashline `ToolConfig` entries produced by
/// `FileToolset::Hashline.tool_configs(&hashline_config)` — they carry the
/// scheme parameters as tool params.
pub fn grow_build_hashline_toolset(
    hashline_tools: Vec<tools::registry::types::ToolConfig>,
) -> ToolServerConfig {
    let mut tools: Vec<tools::registry::types::ToolConfig> = vec![bash_tool_config()];
    tools.extend(hashline_tools);
    tools.extend([
        (&grow_build::ListDirTool).into(),
        kill_task_tool_config(),
        (&grow_build::TodoWriteTool).into(),
        task_output_tool_config(),
        wait_tasks_tool_config(),
        task_tool_config(),
        (&grow_build::SchedulerCreateTool).into(),
        (&grow_build::SchedulerDeleteTool).into(),
        (&grow_build::SchedulerListTool).into(),
        (&grow_build::MonitorTool).into(),
        (&search_tool::SearchTool).into(),
        (&use_tool::UseTool).into(),
        (&grow_build::GetGoalTool).into(),
        (&grow_build::UpdateGoalProgressTool).into(),
        (&grow_build::RequestGoalReplanTool).into(),
        (&grow_build::UpdateGoalTool).into(),
        (&grow_build::WorkflowTool).into(),
    ]);
    ToolServerConfig {
        tools,
        behavior_preset: None,
    }
}
/// Read-only toolset for the **explore** subagent.
///
/// Genuinely read-only: `read_file` (Read), `list_dir` (Glob), `grep` (Grep).
/// `run_terminal_command` (Bash) is intentionally omitted so exploration cannot
/// mutate the workspace — the read-only guarantee is enforced by the toolset,
/// not merely by the prompt. With no `BashTool`, the background-task helpers
/// (`KillTaskTool`/`TaskOutputTool`) are unnecessary and also omitted.
fn explore_toolset() -> ToolServerConfig {
    ToolServerConfig {
        tools: vec![
            (&grow_build::ReadFileTool).into(),
            (&grow_build::ListDirTool).into(),
            (&grow_build::GrepTool).into(),
        ],
        behavior_preset: None,
    }
}

/// Exact host-only toolset for the Goal planner. The planner receives the
/// immutable Goal snapshot in its prompt and may re-read it, but has no shell,
/// mutation, workflow, or delegation surface.
fn goal_planner_toolset() -> ToolServerConfig {
    let mut config = explore_toolset();
    config.tools.push((&grow_build::GetGoalTool).into());
    config
}

/// Exact host-only toolset for the Goal verifier. Its workspace is an
/// isolated worktree; execution is available for evidence collection while
/// every persistent or object-level mutation tool is absent by construction.
fn goal_verifier_toolset() -> ToolServerConfig {
    ToolServerConfig {
        tools: vec![
            bash_tool_config(),
            (&grow_build::ReadFileTool).into(),
            (&grow_build::ListDirTool).into(),
            (&grow_build::GrepTool).into(),
            (&grow_build::GetGoalTool).into(),
        ],
        behavior_preset: None,
    }
}
/// Per-Agent restriction on which peer definitions may be launched through
/// the task tool. This is a tool capability, not an Agent hierarchy: the same
/// definition may be launched anywhere another Agent permits it; primary
/// visibility is decided independently by `subagentOnly` and the capability
/// floor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentFilter {
    allow: Option<HashSet<String>>,
    deny: HashSet<String>,
}

/// Explicit subagent authorization, independent of ordinary tool names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubagentPolicy {
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub allow: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub deny: Vec<String>,
}

impl SubagentFilter {
    pub fn allows(&self, name: &str) -> bool {
        self.allow
            .as_ref()
            .is_none_or(|allowed| allowed.contains(name))
            && !self.deny.contains(name)
    }
}
/// Accepts `"a, b, c"` or `["a", "b"]`. Trims whitespace.
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    struct StringOrVec;
    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a comma-separated string or an array of strings")
        }
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            Ok(s.split(',')
                .map(|item| item.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect())
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                v.push(item);
            }
            Ok(v)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }
    }
    deserializer.deserialize_any(StringOrVec)
}
/// Accepts a positive u32 or null/absent. Rejects 0.
fn deserialize_nonzero_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<u32>::deserialize(deserializer)?;
    if let Some(0) = opt {
        return Err(serde::de::Error::custom("maxTurns must be greater than 0"));
    }
    Ok(opt)
}
/// All built-in agent names as a typed enum.
///
/// Eliminates string matching in discovery and ensures built-in names
/// are defined in exactly one place. The enum covers all built-in
/// agents for centralized name management and `by_name()` dispatch.
///
/// Subagent-only status is declared by each definition's `subagentOnly`
/// frontmatter rather than duplicated in this enum.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum BuiltinAgentName {
    #[strum(serialize = "grow")]
    Grow,
    #[strum(serialize = "grow-build-concise")]
    GrowConcise,
    GeneralPurpose,
    Explore,
    BrowserUse,
}
/// Strict-harness predicate by name. Resolves via `BuiltinAgentName` and
/// delegates to [`AgentDefinition::is_strict_harness`]; unknown names
/// return `false` (conservative — never enforce a harness we can't verify).
/// Callers that already hold an `AgentDefinition` should call that method
/// directly so project-level shadowing is honored.
pub fn is_strict_harness_agent_type(name: &str) -> bool {
    use std::str::FromStr;
    BuiltinAgentName::from_str(name)
        .map(|b| b.definition().is_strict_harness())
        .unwrap_or(false)
}
impl BuiltinAgentName {
    /// Build the `AgentDefinition` for this built-in agent.
    pub fn definition(self) -> AgentDefinition {
        match self {
            Self::Grow => AgentDefinition::default_grow_build(),
            Self::GrowConcise => AgentDefinition::grow_build_concise(),
            Self::GeneralPurpose => AgentDefinition::general_purpose(),
            Self::Explore => AgentDefinition::explore(),
            Self::BrowserUse => AgentDefinition::browser_use(),
        }
    }
    /// Built-in definitions explicitly marked as subagent-only.
    pub fn subagent_variants() -> Vec<Self> {
        use strum::IntoEnumIterator;
        Self::iter()
            .filter(|name| name.definition().subagent_only)
            .collect()
    }
}
/// Portable agent identity — parsed from .grow/agents/*.md.
/// Usable as both a top-level agent and a subagent definition.
///
/// This is the stable, version-controllable contract. It does NOT
/// contain session-level policies (compaction, system reminders).
/// Those are provided by the AgentBuilder at build time.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentDefinition {
    #[serde(default)]
    pub name: String,
    pub description: String,
    /// This definition may be launched by another Agent but is not a valid
    /// primary-session profile and must not appear in the primary Agent picker.
    #[serde(default)]
    pub subagent_only: bool,
    /// Plugin namespace for plugin-backed agents only.
    #[serde(skip)]
    pub plugin_name: Option<String>,
    #[serde(default = "default_prompt_composition")]
    pub prompt_composition: PromptComposition,
    /// Named base tool preset. This is resolved before additional tools and
    /// never carries Behavior semantics.
    #[serde(default = "default_tool_preset")]
    pub tool_preset: String,
    /// Tools layered onto `tool_preset` before runtime injection/filtering.
    #[serde(default)]
    pub additional_tools: Vec<ToolConfig>,
    #[serde(skip, default = "default_grow_build_toolset")]
    pub tool_config: ToolServerConfig,
    /// Runtime capability mode that constrains which tool kinds the agent
    /// can use. Applied during subagent spawn in `handle_subagent_request`
    /// by filtering the definition's `tool_config` before session creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_mode: Option<tool_types::SubagentCapabilityMode>,
    #[serde(default)]
    pub permission_mode: PermissionMode,
    #[serde(default)]
    pub skills: Vec<String>,
    /// When true (the default), the AgentBuilder discovers skills from CWD
    /// at build time and seeds mid-session skill discovery. When false,
    /// skill discovery is suppressed and the agent gets an empty skill
    /// list with no CWD-based runtime discovery.
    #[serde(default = "default_true")]
    pub discover_skills: bool,
    /// Whether to inherit the parent session's discovered skills when
    /// spawned as a subagent. Ignored for primary sessions.
    #[serde(default = "default_true")]
    pub inherit_skills: bool,
    #[serde(default = "default_true")]
    pub agents_md: bool,
    /// When true (the default), the AgentBuilder layers session-level optional
    /// tools on top of the agent's declared `tool_config`: memory_search/get,
    /// web_fetch, lsp, the Grow write fallback, and the plan-mode tools.
    ///
    /// Set this to `false` for harnesses that need an exact, minimal toolset
    /// (e.g. the compat harness, where every advertised tool must match the
    /// model's trained schema). The agent's `tool_config` is then used
    /// verbatim with only the subagent strip applied.
    #[serde(default = "default_true")]
    pub inject_default_tools: bool,
    /// Optional ordinary-tool allowlist. It never authorizes subagents.
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub tools: Vec<String>,
    /// Ordinary-tool denylist. It never authorizes subagents.
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub disallowed_tools: Vec<String>,
    /// Peer Agent authorization for the Task tool.
    #[serde(default)]
    pub subagents: SubagentPolicy,
    #[serde(default)]
    pub effort: Option<Effort>,
    #[serde(default, deserialize_with = "deserialize_nonzero_u32")]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub isolation: Option<IsolationMode>,
    #[serde(default)]
    pub background: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_agent_color")]
    pub color: Option<AgentColor>,
    #[serde(default)]
    pub initial_prompt: Option<String>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerRef>,
    #[serde(default)]
    pub mcp_inheritance: McpInheritance,
    #[serde(default, deserialize_with = "deserialize_hooks_map")]
    pub hooks: Option<HooksConfig>,
    #[serde(default)]
    pub memory: Option<MemoryScope>,
    /// Completion requirement — declares that this agent must call a
    /// specific tool before the turn ends.
    #[serde(default)]
    pub completion_requirement: Option<CompletionRequirement>,
    /// Session-operator tool restrictions (`--tools` / `--disallowed-tools`),
    /// distinct from the agent author's own `tools`/`disallowed_tools`. The
    /// builder applies them as a final clamp over the fully-assembled toolset
    /// (function + hosted), so they bind regardless of later `tool_config`
    /// mutations and compose with the agent's own filters by intersection.
    /// `None` = no session restriction.
    #[serde(skip)]
    pub session_tools_allowlist: Option<Vec<String>>,
    #[serde(skip)]
    pub session_tools_denylist: Option<Vec<String>>,
    #[serde(skip)]
    pub prompt_body: Option<String>,
    #[serde(skip)]
    pub system_prompt: TemplateOverride,
    /// First-user-message template selector. `Default` (the default) lets
    /// the shell layer build the legacy `<user_info>` + `<git_status>`
    /// prefix; `Custom` uses a caller-supplied template string.
    #[serde(default)]
    pub user_message_template: UserMessageTemplate,
    /// Where this definition was loaded from, optional if built in agent definition
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
    /// Discovery scope (project vs user).
    #[serde(skip)]
    pub scope: AgentScope,
}
/// Declares that the agent must call a specific tool before the turn ends.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequirement {
    /// Canonical tool name that must be called.
    pub tool: String,
    /// Reminder text injected when the tool hasn't been called.
    pub reminder: String,
    /// Suggested recovery policy for the harness.
    #[serde(default)]
    pub recovery: Option<RecoveryPolicy>,
}
/// Suggested turn-level recovery policy.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}
/// Per-tool execution config.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecConfig {
    /// Retry config for this tool. None = no retry (execute once).
    #[serde(default)]
    pub retry: Option<ToolRetryConfig>,
}
/// Retry configuration for a single tool.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRetryConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}
/// How the Markdown body interacts with the base prompt template.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptComposition {
    /// Body is appended after the mandatory foundation, audience, and
    /// standard guidance. Default.
    #[default]
    Extend,
    /// Skip optional standard guidance and use the body as the complete role
    /// layer. Mandatory foundation, audience, and runtime context still apply.
    Full,
}
fn default_prompt_composition() -> PromptComposition {
    PromptComposition::Extend
}
/// Where the agent definition was discovered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentScope {
    /// .grow/agents/ (project-level, highest priority)
    Project,
    /// ~/.grow/agents/ (user-level)
    User,
    /// ~/.grow/bundled/agents/ (lowest-priority bundled cache)
    Bundled,
    /// Built-in agent (e.g., default_grow_build(), browser_use()).
    #[default]
    BuiltIn,
}
impl AgentScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Bundled => "bundled",
            Self::BuiltIn => "built-in",
        }
    }
}

/// Why an Agent definition cannot own a primary session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryAgentIssue {
    SubagentOnly,
    MissingWorkspaceRead,
    MissingWorkspaceWrite,
    MissingExecution,
}

impl PrimaryAgentIssue {
    pub fn message(self) -> &'static str {
        match self {
            Self::SubagentOnly => "declared subagentOnly",
            Self::MissingWorkspaceRead => "no workspace read/search capability",
            Self::MissingWorkspaceWrite => "no workspace edit/write capability",
            Self::MissingExecution => "no command execution capability",
        }
    }
}
impl std::fmt::Display for AgentScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
/// Controls which parent MCP servers a subagent inherits.
///
/// Deserializes from:
/// - `"all"` / `"none"` (string, case-insensitive)
/// - `{ "named": ["slack", "github"] }` / `{ "except": ["internal"] }` (map)
///
/// The custom `Deserialize` is needed because `serde_yaml` 0.9 uses YAML
/// tags (`!named`) for externally-tagged enum data variants, but agent
/// definition frontmatter uses the mapping style that JSON also expects.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum McpInheritance {
    #[default]
    All,
    None,
    Named(Vec<String>),
    Except(Vec<String>),
}
impl<'de> Deserialize<'de> for McpInheritance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;
        struct McpInheritanceVisitor;
        impl<'de> de::Visitor<'de> for McpInheritanceVisitor {
            type Value = McpInheritance;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(r#""all", "none", {"named": [...]}, or {"except": [...]}"#)
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                match s.to_ascii_lowercase().as_str() {
                    "all" => Ok(McpInheritance::All),
                    "none" => Ok(McpInheritance::None),
                    other => Err(de::Error::unknown_variant(other, &["all", "none"])),
                }
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let key: String = map
                    .next_key()?
                    .ok_or_else(|| de::Error::invalid_length(0, &"a single-key map"))?;
                let value: Vec<String> = map.next_value()?;
                if map.next_key::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::custom(
                        "mcpInheritance map must have exactly one key",
                    ));
                }
                match key.as_str() {
                    "named" => Ok(McpInheritance::Named(value)),
                    "except" => Ok(McpInheritance::Except(value)),
                    other => Err(de::Error::unknown_variant(other, &["named", "except"])),
                }
            }
        }
        deserializer.deserialize_any(McpInheritanceVisitor)
    }
}
/// Permission mode. Only `BypassPermissions` is wired at spawn; others are forward-compat.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, serde::Serialize, strum::EnumCount)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    AcceptEdits,
    /// Background classifier reviews tool calls.
    Auto,
    /// Silently deny non-pre-approved tools.
    DontAsk,
    BypassPermissions,
}
impl PermissionMode {
    pub const VALID_VALUES: &[&str] = &[
        "default",
        "acceptEdits",
        "auto",
        "dontAsk",
        "bypassPermissions",
    ];
}
const _: () =
    assert!(PermissionMode::VALID_VALUES.len() == <PermissionMode as strum::EnumCount>::COUNT);
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    serde::Serialize,
    IntoStaticStr,
    strum::EnumCount,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    #[strum(serialize = "xhigh")]
    XHigh,
    Max,
}
impl Effort {
    pub const VALID_VALUES: &[&str] = &["low", "medium", "high", "xhigh", "max"];
}
const _: () = assert!(Effort::VALID_VALUES.len() == <Effort as strum::EnumCount>::COUNT);
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    serde::Serialize,
    IntoStaticStr,
    strum::EnumCount,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum IsolationMode {
    None,
    Worktree,
}
impl IsolationMode {
    pub const VALID_VALUES: &[&str] = &["none", "worktree"];
}
const _: () =
    assert!(IsolationMode::VALID_VALUES.len() == <IsolationMode as strum::EnumCount>::COUNT);
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    serde::Serialize,
    AsRefStr,
    EnumString,
    IntoStaticStr,
    strum::EnumCount,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum AgentColor {
    Red,
    Blue,
    Green,
    Yellow,
    Purple,
    Orange,
    Pink,
    Cyan,
}
impl AgentColor {
    pub const VALID_VALUES: &[&str] = &[
        "red", "blue", "green", "yellow", "purple", "orange", "pink", "cyan",
    ];
}
const _: () = assert!(AgentColor::VALID_VALUES.len() == <AgentColor as strum::EnumCount>::COUNT);
/// Never fails: `color` is decorative, but a rejected value fails the whole
/// frontmatter parse, and discovery skips agents that fail to parse — so a
/// typo'd or hex color would silently make the agent unspawnable.
///
/// Frontmatter is only ever decoded by `serde_yaml`, so the intermediate value
/// is captured as `serde_yaml::Value` (total for YAML — tagged scalars and
/// maps with non-string keys included, which have no `serde_json::Value`
/// form). Unrecognized values are dropped to `None` with a warning rather
/// than mapped to a stand-in color the author never wrote.
fn deserialize_agent_color<'de, D>(deserializer: D) -> Result<Option<AgentColor>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use std::str::FromStr;
    let Some(value) = Option::<serde_yaml::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let parsed = value
        .as_str()
        .and_then(|name| AgentColor::from_str(name.trim()).ok());
    if parsed.is_none() {
        tracing::warn!(
            color = ?value,
            valid = ?AgentColor::VALID_VALUES,
            "unrecognized agent color, ignoring"
        );
    }
    Ok(parsed)
}
/// Agent memory scope. Distinct from `storage::MemoryScope` (global-vs-workspace write target).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    serde::Serialize,
    IntoStaticStr,
    strum::EnumCount,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum MemoryScope {
    /// `~/.grow/agent-memory/<name>/`
    User,
    /// `<project>/.grow/agent-memory/<name>/`
    Project,
    /// `<project>/.grow/agent-memory-local/<name>/`
    Local,
}
impl MemoryScope {
    pub const VALID_VALUES: &[&str] = &["user", "project", "local"];
}
const _: () = assert!(MemoryScope::VALID_VALUES.len() == <MemoryScope as strum::EnumCount>::COUNT);
#[derive(Debug)]
pub struct ResolvedMemoryDir {
    pub path: std::path::PathBuf,
    /// No workspace hash needed (already project-scoped).
    pub is_project_scoped: bool,
}
impl MemoryScope {
    pub fn resolve_dir(self, agent_name: &str, project_cwd: &std::path::Path) -> ResolvedMemoryDir {
        match self {
            Self::User => ResolvedMemoryDir {
                path: config::grow_home().join("agent-memory").join(agent_name),
                is_project_scoped: false,
            },
            Self::Project => ResolvedMemoryDir {
                path: project_cwd.join(".grow/agent-memory").join(agent_name),
                is_project_scoped: true,
            },
            Self::Local => ResolvedMemoryDir {
                path: project_cwd
                    .join(".grow/agent-memory-local")
                    .join(agent_name),
                is_project_scoped: true,
            },
        }
    }
}
/// Hooks config validated as an object at parse time. Semantic parsing deferred to spawn.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HooksConfig(pub serde_json::Map<String, serde_json::Value>);
impl HooksConfig {
    pub fn as_value(&self) -> serde_json::Value {
        serde_json::Value::Object(self.0.clone())
    }
}
fn deserialize_hooks_map<'de, D>(deserializer: D) -> Result<Option<HooksConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(serde_json::Value::Object(map)) => Ok(Some(HooksConfig(map))),
        Some(_) => Err(serde::de::Error::custom("hooks must be an object")),
    }
}
/// MCP server reference — typed to catch config errors at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerRef {
    Named(String),
    /// Opaque JSON config resolved to `McpServer` at spawn time.
    Inline {
        name: String,
        config: serde_json::Value,
    },
}
impl<'de> Deserialize<'de> for McpServerRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        match val {
            serde_json::Value::String(s) => Ok(McpServerRef::Named(s)),
            serde_json::Value::Object(obj) if obj.len() == 1 => {
                let (name, config) = obj.into_iter().next().unwrap();
                if !config.is_object() {
                    return Err(serde::de::Error::custom(format!(
                        "mcpServers inline config for '{name}' must be an object"
                    )));
                }
                Ok(McpServerRef::Inline { name, config })
            }
            serde_json::Value::Object(obj) => {
                if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                    Ok(McpServerRef::Inline {
                        name: name.to_string(),
                        config: serde_json::Value::Object(obj),
                    })
                } else {
                    Err(serde::de::Error::custom(
                        "mcpServers entry must be a string, a {name: config} map, \
                         or an object with a 'name' field",
                    ))
                }
            }
            _ => Err(serde::de::Error::custom(
                "mcpServers entry must be a string or object",
            )),
        }
    }
}
impl serde::Serialize for McpServerRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            McpServerRef::Named(s) => serializer.serialize_str(s),
            McpServerRef::Inline { name, config } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(name, config)?;
                map.end()
            }
        }
    }
}
/// Bash tool config overrides (agent-definition layer).
///
/// NOTE: Uses `camelCase` for YAML frontmatter. The `AgentBuilder` maps
/// these into `tools::registry::types::ToolsetConfig.bash`
/// which uses the tools crate's `BashToolConfig` type.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashConfig {
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: f64,
    #[serde(default = "default_output_byte_limit")]
    pub output_byte_limit: usize,
    #[serde(default)]
    pub cmd_prefix: Option<String>,
}
impl Default for BashConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
            output_byte_limit: default_output_byte_limit(),
            cmd_prefix: None,
        }
    }
}
fn default_timeout_secs() -> f64 {
    120.0
}
fn default_output_byte_limit() -> usize {
    200_000
}
fn default_true() -> bool {
    true
}
fn default_tool_preset() -> String {
    "grow-build".to_string()
}
/// Strip a tool id's `Namespace:` prefix, yielding its short name.
pub(crate) fn short_tool_name(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}
/// Whether an allow/deny `entry` refers to tool `id` (by full id or short name).
pub(crate) fn tool_id_eq(entry: &str, id: &str) -> bool {
    entry == id || entry == short_tool_name(id)
}
/// Whether an allow/deny entry refers to a configured tool by either its
/// canonical registry id or the name exposed to the model.
pub(crate) fn tool_config_eq(entry: &str, tool: &ToolConfig) -> bool {
    tool_id_eq(entry, &tool.id)
        || tool
            .name_override
            .as_deref()
            .is_some_and(|name| entry == name)
}
pub(crate) fn tool_config_matches(list: &[String], tool: &ToolConfig) -> bool {
    list.iter().any(|entry| tool_config_eq(entry, tool))
}
impl AgentDefinition {
    /// Resolve the explicit `subagents.allow/deny` policy. Ordinary tool
    /// allow/deny entries have no effect on peer Agent authorization.
    pub fn subagent_filter(&self) -> SubagentFilter {
        let allow = if self.subagents.allow.is_empty() {
            None
        } else {
            Some(self.subagents.allow.iter().cloned().collect())
        };
        SubagentFilter {
            allow,
            deny: self.subagents.deny.iter().cloned().collect(),
        }
    }

    fn resolve_declared_toolset(&mut self) -> Result<(), AgentBuildError> {
        let mut tool_config = toolset_for_preset(&self.tool_preset).ok_or_else(|| {
            AgentBuildError::ParseError(format!(
                "unknown toolPreset '{}'; expected one of: {}",
                self.tool_preset,
                preset_names().join(", ")
            ))
        })?;
        tool_config.tools.extend(self.additional_tools.clone());
        self.tool_config = tool_config;
        Ok(())
    }

    /// Parse an agent definition from a Markdown file with YAML frontmatter.
    ///
    /// File format:
    /// ```text
    /// ---
    /// name: my-agent
    /// description: A custom agent
    /// # ... other fields
    /// ---
    ///
    /// System prompt body goes here...
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, AgentBuildError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(AgentBuildError::IoError)?;
        let mut def = Self::parse(&content)?;
        def.name = file_stem_agent_id(path)?;
        def.source_path = Some(path.to_path_buf());
        def.plugin_name = None;
        def.scope = Self::scope_from_path(path);
        Ok(def)
    }
    /// Parse only YAML frontmatter from an agent file, leaving prompt_body unset.
    pub fn from_file_frontmatter_only(path: impl AsRef<Path>) -> Result<Self, AgentBuildError> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(AgentBuildError::IoError)?;
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            return Err(AgentBuildError::ParseError(
                "missing frontmatter delimiters".to_string(),
            ));
        }
        let after_opening = &trimmed[3..];
        let closing_idx = after_opening.find("\n---").ok_or_else(|| {
            AgentBuildError::ParseError("missing closing frontmatter delimiter".to_string())
        })?;
        let yaml_content = &after_opening[..closing_idx];
        let mut def: AgentDefinition = serde_yaml::from_str(yaml_content)
            .map_err(|e| AgentBuildError::ParseError(e.to_string()))?;
        def.resolve_declared_toolset()?;
        def.name = file_stem_agent_id(path)?;
        def.permission_mode = PermissionMode::Default;
        def.prompt_body = None;
        def.system_prompt = TemplateOverride::None;
        def.source_path = Some(path.to_path_buf());
        def.plugin_name = None;
        def.scope = Self::scope_from_path(path);
        Ok(def)
    }
    /// Parse from string content (for testing and inline definitions).
    pub fn parse(content: &str) -> Result<Self, AgentBuildError> {
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            return Err(AgentBuildError::ParseError(
                "missing frontmatter delimiters".to_string(),
            ));
        }
        let after_opening = &trimmed[3..];
        let closing_idx = after_opening.find("\n---").ok_or_else(|| {
            AgentBuildError::ParseError("missing closing frontmatter delimiter".to_string())
        })?;
        let yaml_content = &after_opening[..closing_idx];
        let after_closing = &after_opening[closing_idx + 4..];
        let body_start = after_closing.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = after_closing[body_start..].trim();
        let prompt_body = if body.is_empty() {
            None
        } else {
            Some(body.to_string())
        };
        let mut def: AgentDefinition = serde_yaml::from_str(yaml_content)
            .map_err(|e| AgentBuildError::ParseError(e.to_string()))?;
        def.resolve_declared_toolset()?;
        // File definitions are prompt profiles. Models and permissions remain
        // session/workspace state even when compatibility frontmatter carries
        // similarly named fields.
        def.permission_mode = PermissionMode::Default;
        def.prompt_body = prompt_body;
        def.plugin_name = None;
        Ok(def)
    }
    /// Determine the scope of a definition file based on its path.
    fn scope_from_path(path: &Path) -> AgentScope {
        let path_str = path.to_string_lossy();
        let grow = config::user_grow_home();
        let home = dirs::home_dir();
        for (dir, scope) in crate::discovery::user_agent_dirs(home.as_deref(), grow.as_deref()) {
            if path.starts_with(&dir) {
                return scope;
            }
        }
        if path_str.contains(".grow/agents/") || path_str.contains(".grow\\agents\\") {
            return AgentScope::Project;
        }
        AgentScope::BuiltIn
    }
}

fn file_stem_agent_id(path: &Path) -> Result<String, AgentBuildError> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            AgentBuildError::ParseError(format!(
                "agent path has no valid UTF-8 file stem: {}",
                path.display()
            ))
        })
}
impl AgentDefinition {
    /// Validate the definition's declared primary-session capability floor.
    ///
    /// A primary Agent must be able to inspect the workspace, make necessary
    /// changes, and execute verification. This is intentionally a definition
    /// check: runtime permission prompts may narrow individual calls, but they
    /// do not turn a worker-only definition into a primary Agent.
    pub fn primary_agent_issues(&self) -> Vec<PrimaryAgentIssue> {
        if self.subagent_only {
            return vec![PrimaryAgentIssue::SubagentOnly];
        }

        let kinds = self.effective_authored_tool_kinds();
        let has = |candidates: &[tools::types::tool::ToolKind]| {
            kinds.iter().any(|kind| candidates.contains(kind))
        };
        use tools::types::tool::ToolKind;
        let mut issues = Vec::new();
        if !has(&[
            ToolKind::Read,
            ToolKind::Search,
            ToolKind::List,
            ToolKind::ListDir,
            ToolKind::Lsp,
        ]) {
            issues.push(PrimaryAgentIssue::MissingWorkspaceRead);
        }
        if !has(&[
            ToolKind::Edit,
            ToolKind::Write,
            ToolKind::Delete,
            ToolKind::Move,
        ]) {
            issues.push(PrimaryAgentIssue::MissingWorkspaceWrite);
        }
        if !has(&[ToolKind::Execute]) {
            issues.push(PrimaryAgentIssue::MissingExecution);
        }
        issues
    }

    pub fn is_primary_agent_eligible(&self) -> bool {
        self.primary_agent_issues().is_empty()
    }

    /// Apply the Agent-authored portion of the tool assembly order for static
    /// eligibility checks. Unknown allowlist entries grant no capability,
    /// matching the runtime builder's fail-closed behavior.
    fn effective_authored_tool_kinds(&self) -> Vec<tools::types::tool::ToolKind> {
        use tools::implementations::grow_build::task::types::SubagentCapabilityModeExt;
        let mut tools = self.tool_config.tools.clone();
        if self.inject_default_tools
            && !tools.iter().any(|tool| {
                matches!(
                    tool.kind,
                    Some(tools::types::tool::ToolKind::Edit | tools::types::tool::ToolKind::Write)
                )
            })
        {
            tools.push((&grow_build::WriteTool).into());
        }
        tools.retain(|tool| !tool_config_matches(&self.disallowed_tools, tool));

        if !self.tools.is_empty() {
            let present_kinds: HashSet<_> = tools.iter().filter_map(|tool| tool.kind).collect();
            let mut allowed_kinds = HashSet::new();
            for entry in &self.tools {
                if tools.iter().any(|tool| tool_config_eq(entry, tool)) {
                    continue;
                }
                match tools::types::kind_for(entry) {
                    Some(kind) if present_kinds.contains(&kind) => {
                        allowed_kinds.insert(kind);
                    }
                    Some(_) => {}
                    None => {}
                }
            }
            tools.retain(|tool| {
                tool_config_matches(&self.tools, tool)
                    || tool.kind.is_some_and(|kind| allowed_kinds.contains(&kind))
                    || matches!(
                        tool.kind,
                        Some(
                            tools::types::tool::ToolKind::SearchTool
                                | tools::types::tool::ToolKind::UseTool
                        )
                    )
            });
        }

        if let Some(mode) = self.capability_mode {
            let allowed = mode.allowed_tool_kinds();
            tools.retain(|tool| tool.kind.is_some_and(|kind| allowed.contains(&kind)));
        }

        tools.into_iter().filter_map(|tool| tool.kind).collect()
    }

    /// Whether `id` passes the session-operator clamp: denylist wins, then an
    /// unset allowlist allows all.
    pub(crate) fn session_tools_allowed(&self, tool: &ToolConfig) -> bool {
        if self
            .session_tools_denylist
            .as_deref()
            .is_some_and(|deny| tool_config_matches(deny, tool))
        {
            return false;
        }
        self.session_tools_allowlist
            .as_deref()
            .is_none_or(|allow| tool_config_matches(allow, tool))
    }
    /// Replace the file-operation tools (read/edit/search) in the tool config
    /// with the given set. Used by the shell layer to swap from standard to
    /// hashline toolset based on `config.toml` / remote settings.
    /// True iff the active system prompt template for `audience` carries
    /// the `<task_completion_discipline>` block.
    ///
    /// Used by the runtime turn-end TodoGate to gate firing on sessions
    /// whose prompt actually references the rules the gate's reminder
    /// text invokes. The block has been removed from every built-in
    /// template, so this returns `false` unconditionally. Kept as a
    /// helper so the gate's call-site stays stable in case the block
    /// is reintroduced behind a future flag.
    pub fn carries_task_completion_discipline(
        &self,
        _audience: crate::prompt::context::PromptAudience,
    ) -> bool {
        false
    }
    /// True iff this agent's wire format is non-interchangeable with the
    /// stock harness, so a client-supplied `_meta.agentProfile` must NOT
    /// override it. Strict iff any of: bespoke `system_prompt` template,
    /// bespoke `user_message_template`, or curated toolset
    /// (`!inject_default_tools`). Stock `grow-build*` agents leave all
    /// three at defaults and are non-strict.
    pub fn is_strict_harness(&self) -> bool {
        use crate::prompt::context::TemplateOverride;
        use crate::prompt::user_message::UserMessageTemplate;
        let prompt_is_custom = !matches!(self.system_prompt, TemplateOverride::None);
        let user_template_is_custom =
            !matches!(self.user_message_template, UserMessageTemplate::Default);
        let toolset_is_curated = !self.inject_default_tools;
        prompt_is_custom || user_template_is_custom || toolset_is_curated
    }
    /// Swap the definition's file tools for the equivalents in `file_tools`
    /// (hashline vs standard), slot by slot — never granting a slot the
    /// definition doesn't already have (read-only toolsets stay read-only).
    pub fn override_file_tools(&mut self, file_tools: Vec<tools::registry::types::ToolConfig>) {
        const FILE_TOOL_SLOTS: &[[&str; 2]] = &[
            ["Grow:read_file", "GrowHashline:hashline_read"],
            ["Grow:search_replace", "GrowHashline:hashline_edit"],
            ["Grow:grep", "GrowHashline:hashline_grep"],
        ];
        for tool in self.tool_config.tools.iter_mut() {
            let Some(slot) = FILE_TOOL_SLOTS
                .iter()
                .find(|slot| slot.contains(&tool.id.as_str()))
            else {
                continue;
            };
            if let Some(replacement) = file_tools.iter().find(|ft| slot.contains(&ft.id.as_str())) {
                *tool = replacement.clone();
            }
        }
    }
    /// Shared defaults for out-of-tree built-in agent registrations.
    pub fn builtin_defaults(name: &str, description: &str) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_string(),
            subagent_only: false,
            plugin_name: None,
            prompt_composition: PromptComposition::Extend,
            tool_preset: default_tool_preset(),
            additional_tools: vec![],
            tool_config: default_grow_build_toolset(),
            capability_mode: None,
            permission_mode: PermissionMode::Default,
            skills: vec![],
            agents_md: true,
            discover_skills: true,
            inherit_skills: true,
            inject_default_tools: true,
            disallowed_tools: vec![],
            subagents: SubagentPolicy::default(),
            tools: vec![],
            effort: None,
            max_turns: None,
            isolation: None,
            background: None,
            color: None,
            initial_prompt: None,
            mcp_servers: vec![],
            mcp_inheritance: McpInheritance::All,
            hooks: None,
            memory: None,
            session_tools_allowlist: None,
            session_tools_denylist: None,
            completion_requirement: None,
            prompt_body: None,
            system_prompt: TemplateOverride::None,
            source_path: None,
            user_message_template: UserMessageTemplate::Default,
            scope: AgentScope::BuiltIn,
        }
    }
    pub fn default_grow_build() -> Self {
        let mut definition = Self::parse(include_str!("../prompts/agents/grow.md"))
            .expect("embedded grow Agent definition must be valid");
        definition.scope = AgentScope::BuiltIn;
        definition
    }
    /// Grow Concise agent definition — concise output format for SFT/RL.
    pub fn grow_build_concise() -> Self {
        Self::embedded_builtin(include_str!("../prompts/agents/grow-build-concise.md"))
    }
    /// General-purpose subagent definition.
    pub fn general_purpose() -> Self {
        let mut definition = Self::parse(include_str!("../prompts/agents/general-purpose.md"))
            .expect("embedded general-purpose Agent definition must be valid");
        definition.scope = AgentScope::BuiltIn;
        definition
    }
    /// Explore subagent — fast, read-only codebase exploration.
    pub fn explore() -> Self {
        let mut definition = Self::parse(include_str!("../prompts/agents/explore.md"))
            .expect("embedded explore Agent definition must be valid");
        definition.scope = AgentScope::BuiltIn;
        definition
    }
    /// Host-only Goal planning stage. This profile is intentionally not a
    /// `BuiltinAgentName`, so discovery and the general Task catalog cannot
    /// expose or resolve it.
    pub fn goal_planner() -> Self {
        Self::embedded_builtin(include_str!("../prompts/agents/goal-planner.md"))
    }
    /// Host-only Goal verification stage; see [`Self::goal_planner`].
    pub fn goal_verifier() -> Self {
        Self::embedded_builtin(include_str!("../prompts/agents/goal-verifier.md"))
    }
    /// Browser Use agent definition.
    pub fn browser_use() -> Self {
        Self::embedded_builtin(include_str!("../prompts/agents/browser-use.md"))
    }
    fn embedded_builtin(source: &'static str) -> Self {
        let mut definition = Self::parse(source).expect("embedded Agent definition must be valid");
        definition.scope = AgentScope::BuiltIn;
        definition
    }
    /// Deserialize an agent definition from a JSON value (e.g. from ACP `_meta.agentProfile`).
    ///
    /// Unlike `parse()` (which reads YAML frontmatter + Markdown body from a file),
    /// this method accepts a flat JSON object where `promptBody` is an explicit
    /// string field rather than the body below `---` delimiters.
    ///
    /// ```json
    /// {
    ///   "name": "my-agent",
    ///   "description": "A custom agent profile.",
    ///   "promptComposition": "extend",
    ///   "permissionMode": "dontAsk",
    ///   "promptBody": "You are a specialized coding assistant..."
    /// }
    /// ```
    pub fn from_json(value: &serde_json::Value) -> Result<Self, AgentBuildError> {
        let mut definition_value = value.clone();
        if let Some(object) = definition_value.as_object_mut() {
            object.remove("promptBody");
        }
        let mut def: AgentDefinition = serde_json::from_value(definition_value)
            .map_err(|e| AgentBuildError::ParseError(e.to_string()))?;
        if def.name.trim().is_empty() {
            return Err(AgentBuildError::ParseError(
                "agent name must not be empty".to_string(),
            ));
        }
        if let Some(body) = value.get("promptBody").and_then(|v| v.as_str()) {
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                def.prompt_body = Some(trimmed.to_string());
            }
        }
        def.resolve_declared_toolset()?;
        def.permission_mode = PermissionMode::Default;
        def.scope = AgentScope::BuiltIn;
        Ok(def)
    }
    /// Serialize to a JSON value suitable for `from_json` roundtrip.
    /// Handles `prompt_body` which is `#[serde(skip)]` on the struct.
    pub fn to_json_value(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).expect("AgentDefinition is always serializable");
        if let Some(ref body) = self.prompt_body {
            value["promptBody"] = serde_json::Value::String(body.clone());
        }
        value
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Native presets only.
    #[test]
    fn toolset_for_preset_resolves_known_names() {
        for name in [
            "grow-build",
            "grow_build",
            "grow-build-concise",
            "explore",
            "grow-computer",
            "grow_computer",
        ] {
            assert!(
                toolset_for_preset(name).is_some(),
                "preset `{name}` should resolve"
            );
        }
        assert!(toolset_for_preset("does-not-exist").is_none());
    }
    #[test]
    fn presets_select_distinct_toolsets_by_size() {
        let gb = toolset_for_preset("grow-build").unwrap();
        let explore = toolset_for_preset("explore").unwrap();
        assert!(explore.tools.len() < gb.tools.len());
    }
    fn grow_computer_exclusive_ids() -> Vec<String> {
        #[allow(unused_mut)]
        let mut ids: Vec<String> = vec![
            ToolConfig::from(&grow_build::GetTerminalCommandOutputTool).id,
            ToolConfig::from(&grow_build::KillTerminalCommandTool).id,
        ];
        ids
    }
    #[test]
    fn grow_computer_preset_is_curated_grow_build_subset() {
        let gc = toolset_for_preset("grow-computer").unwrap();
        let gb = workspace_grow_build_toolset();
        let gb_ids: std::collections::HashSet<&str> =
            gb.tools.iter().map(|t| t.id.as_str()).collect();
        let exclusive_ids = grow_computer_exclusive_ids();
        assert!(!gc.tools.is_empty());
        for t in &gc.tools {
            if exclusive_ids.contains(&t.id) {
                continue;
            }
            assert!(
                gb_ids.contains(t.id.as_str()),
                "grow-computer tool `{}` must also ship in the resolved grow-build toolset",
                t.id
            );
        }
        assert!(
            gc.tools.len() < gb.tools.len(),
            "grow-computer should be a curated subset of grow-build"
        );
    }
    #[test]
    fn grow_computer_uses_subagent_free_background_task_tools() {
        let gc = toolset_for_preset("grow-computer").unwrap();
        let ids: std::collections::HashSet<&str> = gc.tools.iter().map(|t| t.id.as_str()).collect();
        assert!(
            ids.contains(
                ToolConfig::from(&grow_build::GetTerminalCommandOutputTool)
                    .id
                    .as_str()
            )
        );
        assert!(
            ids.contains(
                ToolConfig::from(&grow_build::KillTerminalCommandTool)
                    .id
                    .as_str()
            )
        );
        assert!(!ids.contains(ToolConfig::from(&grow_build::TaskOutputTool).id.as_str()));
        assert!(!ids.contains(ToolConfig::from(&grow_build::KillTaskTool).id.as_str()));
        assert!(!ids.contains(ToolConfig::from(&grow_build::TaskTool).id.as_str()));
        for t in &gc.tools {
            if t.id == ToolConfig::from(&grow_build::GetTerminalCommandOutputTool).id
                || t.id == ToolConfig::from(&grow_build::KillTerminalCommandTool).id
            {
                assert!(t.name_override.is_none(), "tool `{}` must not rename", t.id);
            }
        }
    }
    /// The grow-computer preset must ship a full-file write tool. Guards
    /// against `search_replace` being the only
    /// file-mutation path, which has no single-tool full-rewrite when the
    /// empty-old_string overwrite guard is enabled.
    #[test]
    fn grow_computer_preset_includes_write_tool() {
        let gc = toolset_for_preset("grow-computer").unwrap();
        let write_id = ToolConfig::from(&grow_build::WriteTool).id;
        assert!(
            gc.tools.iter().any(|t| t.id == write_id),
            "grow-computer preset must include the `{write_id}` tool"
        );
    }
    #[test]
    fn grow_computer_preset_excludes_plan_and_lsp() {
        let gc = toolset_for_preset("grow-computer").unwrap();
        let gc_ids: std::collections::HashSet<&str> =
            gc.tools.iter().map(|t| t.id.as_str()).collect();
        for excluded in [
            ToolConfig::from(&grow_build::LspTool).id,
            ToolConfig::from(&grow_build::PlanControlTool).id,
        ] {
            assert!(
                !gc_ids.contains(excluded.as_str()),
                "grow-computer preset must not advertise `{excluded}`"
            );
        }
        let full = workspace_grow_build_toolset();
        let full_ids: std::collections::HashSet<&str> =
            full.tools.iter().map(|t| t.id.as_str()).collect();
        for present in [
            ToolConfig::from(&grow_build::LspTool).id,
            ToolConfig::from(&grow_build::PlanControlTool).id,
        ] {
            assert!(
                full_ids.contains(present.as_str()),
                "workspace_grow_build_toolset must ship `{present}`"
            );
        }
    }
    /// Exhaustive match → adding a new `BuiltinAgentName` won't compile
    /// until classified.
    fn expected_strict_harness(name: BuiltinAgentName) -> bool {
        match name {
            BuiltinAgentName::Grow
            | BuiltinAgentName::GrowConcise
            | BuiltinAgentName::GeneralPurpose
            | BuiltinAgentName::Explore
            | BuiltinAgentName::BrowserUse => false,
        }
    }
    /// Invariant: structural `is_strict_harness()` must match the
    /// hand-classified expectation for every built-in variant.
    #[test]
    fn is_strict_harness_matches_structural_classification_for_all_builtins() {
        use strum::IntoEnumIterator;
        for variant in BuiltinAgentName::iter() {
            let structural = variant.definition().is_strict_harness();
            let expected = expected_strict_harness(variant);
            assert_eq!(
                structural, expected,
                "BuiltinAgentName::{variant:?}: structural={structural} but \
                 expected={expected}. Update `expected_strict_harness` if the \
                 change is intentional.",
            );
        }
    }
    #[test]
    fn is_strict_harness_agent_type_classifies_by_name() {
        for non_strict in [
            "grow",
            "grow-build-concise",
            "unknown-a",
            "unknown-b",
            "browser-use",
            "custom-user-agent",
            "",
            "grow-build-totally-made-up",
        ] {
            assert!(
                !is_strict_harness_agent_type(non_strict),
                "{non_strict} should be non-strict",
            );
        }
    }
    #[test]
    fn test_parse_valid_full_definition() {
        let content = r#"---
name: test-agent
description: A test agent
promptComposition: full
tools:
  - read_file
  - grep
permissionMode: dontAsk
agentsMd: false
subagentOnly: true
---

You are a test agent.
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(def.name, "test-agent");
        assert_eq!(def.description, "A test agent");
        assert_eq!(def.prompt_composition, PromptComposition::Full);
        assert_eq!(def.permission_mode, PermissionMode::Default);
        assert!(!def.agents_md);
        assert!(def.subagent_only);
        assert_eq!(def.prompt_body.as_deref(), Some("You are a test agent."));
        assert_eq!(
            def.tools,
            vec!["read_file".to_string(), "grep".to_string()],
            "tools allowlist must be parsed from YAML frontmatter"
        );
    }
    #[test]
    fn root_agent_markdown_example_parses_all_supported_features() {
        let def = AgentDefinition::parse(include_str!("../../../../agent.md.example"))
            .expect("repository-root agent.md.example must remain a valid Agent definition");

        assert_eq!(def.name, "example-agent");
        assert_eq!(def.prompt_composition, PromptComposition::Extend);
        assert_eq!(
            def.capability_mode,
            Some(tool_types::SubagentCapabilityMode::ReadWrite)
        );
        assert_eq!(def.effort, Some(Effort::High));
        assert_eq!(def.max_turns, Some(40));
        assert_eq!(def.isolation, Some(IsolationMode::Worktree));
        assert_eq!(def.background, Some(false));
        assert_eq!(def.color, Some(AgentColor::Green));
        assert!(def.initial_prompt.is_some());
        assert_eq!(def.skills, ["coding"]);
        assert!(def.discover_skills);
        assert!(def.inherit_skills);
        assert!(def.agents_md);
        assert!(def.subagent_only);
        assert!(def.inject_default_tools);
        assert!(!def.tools.is_empty());
        assert_eq!(def.disallowed_tools, ["deploy_app"]);
        assert!(def.tool_config.behavior_preset.is_none());
        assert_eq!(def.additional_tools.len(), 2);
        assert!(def.tool_config.tools.len() > def.additional_tools.len());
        assert_eq!(def.mcp_servers.len(), 2);
        assert_eq!(
            def.mcp_inheritance,
            McpInheritance::Except(vec!["private-parent-server".into()])
        );
        let hooks = def.hooks.as_ref().expect("example must cover inline hooks");
        let (hook_specs, hook_errors) =
            hooks::config::parse_hooks_from_value(&hooks.as_value(), "agent.md.example");
        assert!(
            hook_errors.is_empty(),
            "invalid example hooks: {hook_errors:?}"
        );
        assert_eq!(hook_specs.len(), 2);
        assert_eq!(def.memory, Some(MemoryScope::Project));
        assert!(def.completion_requirement.is_some());
        assert!(matches!(
            def.user_message_template,
            UserMessageTemplate::Custom(_)
        ));
        assert_eq!(
            def.permission_mode,
            PermissionMode::Default,
            "Agent files must not override session permission state"
        );
    }
    #[test]
    fn primary_eligibility_requires_read_write_and_execute() {
        let primary = AgentDefinition::default_grow_build();
        assert!(primary.is_primary_agent_eligible());

        let mut read_only = AgentDefinition::explore();
        read_only.subagent_only = false;
        assert_eq!(
            read_only.primary_agent_issues(),
            vec![
                PrimaryAgentIssue::MissingWorkspaceWrite,
                PrimaryAgentIssue::MissingExecution,
            ]
        );

        let worker = AgentDefinition::general_purpose();
        assert_eq!(
            worker.primary_agent_issues(),
            vec![PrimaryAgentIssue::SubagentOnly]
        );
    }

    #[test]
    fn primary_eligibility_applies_agent_tool_denials() {
        let mut definition = AgentDefinition::default_grow_build();
        definition.disallowed_tools.push("run_terminal_cmd".into());
        assert!(
            definition
                .primary_agent_issues()
                .contains(&PrimaryAgentIssue::MissingExecution)
        );
    }
    #[test]
    fn test_parse_tools_and_disallowed_together() {
        let content = r#"---
name: mixed-tools
description: Both tools and disallowedTools
tools:
  - read_file
  - grep
  - search_replace
  - task
disallowedTools:
  - search_replace
---

Mixed agent.
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(def.tools.len(), 4, "tools allowlist should have 4 entries");
        assert_eq!(
            def.disallowed_tools,
            vec!["search_replace".to_string()],
            "disallowedTools should have 1 entry"
        );
    }
    #[test]
    fn explicit_subagent_policy_parses_independently_from_tools() {
        let content = r#"---
name: coordinator
description: test
tools: Read, Bash
subagents:
  allow: [worker, researcher]
---

Agent.
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(
            def.tools,
            vec!["Read", "Bash"],
            "ordinary tool parsing must not carry subagent syntax"
        );
        assert!(def.subagent_filter().allows("worker"));
        assert!(def.subagent_filter().allows("researcher"));
    }
    #[test]
    fn subagent_filter_uses_explicit_allowlist() {
        let mut def = AgentDefinition::default_grow_build();
        def.subagents.allow = vec!["explore".into(), "reviewer".into()];
        let filter = def.subagent_filter();
        assert!(filter.allows("explore"));
        assert!(filter.allows("reviewer"));
        assert!(!filter.allows("plan"));
    }
    #[test]
    fn subagent_filter_denylist_wins() {
        let mut def = AgentDefinition::default_grow_build();
        def.subagents.deny = vec!["plan".into()];
        let filter = def.subagent_filter();
        assert!(filter.allows("explore"));
        assert!(!filter.allows("plan"));

        def.subagents.allow = vec!["plan".into()];
        def.subagents.deny = vec!["plan".into()];
        let filter = def.subagent_filter();
        assert!(!filter.allows("plan"));
    }
    #[test]
    fn subagent_filter_is_independent_of_tool_allowlist() {
        let mut def = AgentDefinition::default_grow_build();
        assert!(def.subagent_filter().allows("explore"));

        def.tools = vec!["read_file".into(), "grep".into()];
        assert!(def.subagent_filter().allows("explore"));
    }
    #[test]
    fn test_parse_tools_comma_separated() {
        let content = r#"---
name: comma-tools
description: Comma-separated tools
tools: read_file, grep, list_dir
disallowedTools: search_replace, write
---

Agent.
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(
            def.tools,
            vec!["read_file", "grep", "list_dir"],
            "comma-separated tools must parse correctly"
        );
        assert_eq!(
            def.disallowed_tools,
            vec!["search_replace", "write"],
            "comma-separated disallowedTools must parse correctly"
        );
    }
    #[test]
    fn test_parse_tool_names_preserves_case() {
        let content = r#"---
name: ci-test
description: test
tools: read, Read
---

Agent.
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(
            def.tools,
            vec!["read", "Read"],
            "ordinary tool names should preserve their declared case"
        );
    }
    #[test]
    fn test_parse_max_turns_zero_rejected() {
        let content = "---\nname: test\ndescription: Test\nmaxTurns: 0\n---\n";
        let result = AgentDefinition::parse(content);
        assert!(
            result.is_err(),
            "maxTurns: 0 should be rejected at parse time"
        );
    }
    #[test]
    fn test_parse_minimal_defaults_none_fields() {
        let content = "---\nname: minimal\ndescription: Test\n---\n";
        let def = AgentDefinition::parse(content).unwrap();
        assert!(def.effort.is_none());
        assert!(def.max_turns.is_none());
        assert!(def.isolation.is_none());
        assert!(def.background.is_none());
        assert!(def.color.is_none());
        assert!(def.initial_prompt.is_none());
        assert!(def.memory.is_none());
        assert!(def.hooks.is_none());
    }
    #[test]
    fn test_parse_profile_fields() {
        let content = r#"---
name: full-fields
description: All new fields
effort: high
maxTurns: 10
isolation: worktree
background: true
color: blue
initialPrompt: "hello world"
---

Agent body.
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(def.effort, Some(Effort::High));
        assert_eq!(def.max_turns, Some(10));
        assert_eq!(def.isolation, Some(IsolationMode::Worktree));
        assert_eq!(def.background, Some(true));
        assert_eq!(def.color, Some(AgentColor::Blue));
        assert_eq!(def.initial_prompt.as_deref(), Some("hello world"));
    }
    #[test]
    fn test_parse_minimal_definition() {
        let content = r#"---
name: minimal
description: Minimal agent
---
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(def.name, "minimal");
        assert_eq!(def.description, "Minimal agent");
        assert_eq!(def.prompt_composition, PromptComposition::Extend);
        assert!(def.agents_md);
        assert!(def.prompt_body.is_none());
    }
    #[test]
    fn mcp_server_ref_parse_and_reject() {
        let v: McpServerRef = serde_json::from_value(serde_json::json!("slack")).unwrap();
        assert_eq!(v, McpServerRef::Named("slack".to_string()));
        let v: McpServerRef =
            serde_json::from_value(serde_json::json!({"s": {"type": "stdio"}})).unwrap();
        assert!(matches!(v, McpServerRef::Inline { ref name, .. } if name == "s"));
        let v: McpServerRef =
            serde_json::from_value(serde_json::json!({"name": "s", "type": "stdio"})).unwrap();
        assert!(matches!(v, McpServerRef::Inline { ref name, .. } if name == "s"));
        assert!(
            serde_json::from_value::<McpServerRef>(serde_json::json!({"type": "stdio"})).is_err()
        );
        assert!(serde_json::from_value::<McpServerRef>(serde_json::json!(42)).is_err());
        assert!(serde_json::from_value::<McpServerRef>(serde_json::json!({"s": "bad"})).is_err());
    }
    #[test]
    fn memory_scope_resolve_dir() {
        let cwd = std::path::Path::new("/project");
        let user = MemoryScope::User.resolve_dir("a", cwd);
        assert!(user.path.ends_with("agent-memory/a"));
        assert!(!user.is_project_scoped);
        let proj = MemoryScope::Project.resolve_dir("a", cwd);
        assert_eq!(
            proj.path,
            std::path::PathBuf::from("/project/.grow/agent-memory/a")
        );
        assert!(proj.is_project_scoped);
        let local = MemoryScope::Local.resolve_dir("a", cwd);
        assert_eq!(
            local.path,
            std::path::PathBuf::from("/project/.grow/agent-memory-local/a")
        );
        assert!(local.is_project_scoped);
    }
    #[test]
    fn all_new_enum_variants_parse() {
        for effort in Effort::VALID_VALUES {
            let c = format!("---\nname: t\ndescription: t\neffort: {effort}\n---\n");
            assert!(
                AgentDefinition::parse(&c).unwrap().effort.is_some(),
                "effort: {effort}"
            );
        }
        for iso in IsolationMode::VALID_VALUES {
            let c = format!("---\nname: t\ndescription: t\nisolation: {iso}\n---\n");
            assert!(
                AgentDefinition::parse(&c).unwrap().isolation.is_some(),
                "isolation: {iso}"
            );
        }
        for color in AgentColor::VALID_VALUES {
            let c = format!("---\nname: t\ndescription: t\ncolor: {color}\n---\n");
            let parsed = AgentDefinition::parse(&c).unwrap().color;
            assert_eq!(
                parsed.map(<&'static str>::from),
                Some(*color),
                "color: {color}"
            );
        }
        for memory in MemoryScope::VALID_VALUES {
            let c = format!("---\nname: t\ndescription: t\nmemory: {memory}\n---\n");
            assert!(
                AgentDefinition::parse(&c).unwrap().memory.is_some(),
                "memory: {memory}"
            );
        }
    }
    #[test]
    fn test_parse_allows_path_derived_name() {
        let content = r#"---
description: No name
----
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert!(def.name.is_empty());
    }
    #[test]
    fn unparseable_color_is_dropped_instead_of_dropping_the_agent() {
        for (declared, expected) in [
            ("Purple", Some(AgentColor::Purple)),
            ("  CYAN  ", Some(AgentColor::Cyan)),
            ("teal", None),
            ("\"#ff0000\"", None),
            ("chartreuse", None),
            ("42", None),
            ("[red, blue]", None),
            ("!custom x", None),
            ("{1: 2}", None),
        ] {
            let c = format!("---\nname: t\ndescription: t\ncolor: {declared}\n---\n");
            let def = AgentDefinition::parse(&c)
                .unwrap_or_else(|e| panic!("color {declared} must not fail the parse: {e}"));
            assert_eq!(def.color, expected, "color: {declared}");
            assert_eq!(def.name, "t");
        }
    }
    #[test]
    fn absent_or_null_color_stays_none() {
        let def = AgentDefinition::parse("---\nname: t\ndescription: t\n---\n").unwrap();
        assert!(def.color.is_none());
        let def = AgentDefinition::parse("---\nname: t\ndescription: t\ncolor:\n---\n").unwrap();
        assert!(def.color.is_none());
    }
    #[test]
    fn test_parse_missing_name() {
        let content = r#"---
description: No name
---
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert!(def.name.is_empty());
    }
    #[test]
    fn test_parse_missing_delimiters() {
        let content = "Just some text without frontmatter";
        let result = AgentDefinition::parse(content);
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentBuildError::ParseError(msg) => {
                assert!(msg.contains("delimiter") || msg.contains("frontmatter"));
            }
            e => panic!("Expected ParseError, got: {:?}", e),
        }
    }
    #[test]
    fn test_parse_unknown_fields_rejected() {
        let content = r#"---
name: test
description: Test
hooks:
  PreToolUse:
    - matcher: Bash
      hooks:
        - type: command
          command: echo hi
memory: user
unknownField: value
---
"#;
        assert!(AgentDefinition::parse(content).is_err());
    }
    #[test]
    fn test_unsupported_frontmatter_fields_are_rejected() {
        for field in [
            "mode",
            "model",
            "variant",
            "permission",
            "permissions",
            "request",
        ] {
            let content = format!("---\nname: test\ndescription: Test\n{field}: value\n---\n");
            assert!(
                AgentDefinition::parse(&content).is_err(),
                "unsupported field `{field}` must be rejected",
            );
        }
    }
    #[test]
    fn test_parse_completion_requirement() {
        let content = r#"---
name: completion-test
description: Test completion requirement parsing
completionRequirement:
  tool: my_agent__complete_task
  reminder: You must call complete_task
  recovery:
    maxRetries: 5
    baseDelayMs: 5000
    maxDelayMs: 60000
---
"#;
        let def = AgentDefinition::parse(content).unwrap();
        let req = def.completion_requirement.unwrap();
        assert_eq!(req.tool, "my_agent__complete_task");
        assert_eq!(req.reminder, "You must call complete_task");
        let recovery = req.recovery.unwrap();
        assert_eq!(recovery.max_retries, 5);
        assert_eq!(recovery.base_delay_ms, 5000);
        assert_eq!(recovery.max_delay_ms, 60000);
    }
    #[test]
    fn test_builtin_browser_use() {
        let def = AgentDefinition::browser_use();
        assert_eq!(def.name, "browser-use");
        assert_eq!(def.prompt_composition, PromptComposition::Full);
        assert!(!def.agents_md);
    }
    #[test]
    fn test_completion_requirement_round_trips() {
        let content = r#"---
name: roundtrip
description: Test round-trip
completionRequirement:
  tool: my__complete
  reminder: Please complete
  recovery:
    maxRetries: 3
    baseDelayMs: 1000
    maxDelayMs: 10000
---
"#;
        let def = AgentDefinition::parse(content).unwrap();
        let req = def.completion_requirement.as_ref().unwrap();
        assert_eq!(req.tool, "my__complete");
        assert_eq!(req.reminder, "Please complete");
        let rec = req.recovery.as_ref().unwrap();
        assert_eq!(rec.max_retries, 3);
        assert_eq!(rec.base_delay_ms, 1000);
        assert_eq!(rec.max_delay_ms, 10000);
    }
    #[test]
    fn test_default_tool_config_has_grow_build_tools() {
        let content = r#"---
name: default-tools
description: Test default tool config
---
"#;
        let def = AgentDefinition::parse(content).unwrap();
        assert!(
            !def.tool_config.tools.is_empty(),
            "default tool_config should have grow_build tools"
        );
    }
    #[test]
    fn test_permission_mode_round_trips() {
        for v in PermissionMode::VALID_VALUES {
            let content = format!("---\nname: test\ndescription: Test\npermissionMode: {v}\n---\n");
            AgentDefinition::parse(&content)
                .unwrap_or_else(|e| panic!("PermissionMode '{v}' failed parse: {e}"));
        }
    }
    #[test]
    fn test_prompt_composition_round_trips() {
        for (yaml_val, expected) in [
            ("extend", PromptComposition::Extend),
            ("full", PromptComposition::Full),
        ] {
            let content =
                format!("---\nname: test\ndescription: Test\npromptComposition: {yaml_val}\n---\n");
            let def = AgentDefinition::parse(&content).unwrap();
            assert_eq!(def.prompt_composition, expected, "Failed for: {yaml_val}");
        }
    }
    #[test]
    fn test_from_file_sets_scope_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test-agent.md");
        std::fs::write(
            &file_path,
            "---\nname: file-test\ndescription: From file\n---\n",
        )
        .unwrap();
        let def = AgentDefinition::from_file(&file_path).unwrap();
        assert_eq!(def.name, "test-agent");
        assert_eq!(def.source_path, Some(file_path));
    }
    #[test]
    fn test_from_json_minimal() {
        let json = serde_json::json!({
            "name": "acp-agent",
            "description": "An agent from ACP"
        });
        let def = AgentDefinition::from_json(&json).unwrap();
        assert_eq!(def.name, "acp-agent");
        assert_eq!(def.description, "An agent from ACP");
        assert_eq!(def.prompt_composition, PromptComposition::Extend);
        assert!(def.agents_md);
        assert!(def.prompt_body.is_none());
        assert_eq!(def.scope, AgentScope::BuiltIn);
    }
    #[test]
    fn test_from_json_has_default_toolset_with_task_tool() {
        let json = serde_json::json!({
            "name": "grow-build",
            "description": "Multi-surface coding agent.",
            "promptComposition": "extend",
            "permissionMode": "dontAsk",
            "agentsMd": true,
            "promptBody": "You are a coding assistant."
        });
        let def = AgentDefinition::from_json(&json).unwrap();
        let task_tool_id = "Grow:task";
        assert!(
            def.tool_config.tools.iter().any(|tc| tc.id == task_tool_id),
            "from_json() without toolConfig should include TaskTool in default toolset, \
             got tool IDs: {:?}",
            def.tool_config
                .tools
                .iter()
                .map(|tc| &tc.id)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn test_from_json_with_prompt_body() {
        let json = serde_json::json!({
            "name": "custom-agent",
            "description": "Agent with prompt body",
            "promptBody": "You are a specialized coding assistant.\n\nFocus on Rust."
        });
        let def = AgentDefinition::from_json(&json).unwrap();
        assert_eq!(def.name, "custom-agent");
        assert_eq!(
            def.prompt_body.as_deref(),
            Some("You are a specialized coding assistant.\n\nFocus on Rust.")
        );
    }
    #[test]
    fn test_from_json_ignores_permission_mode() {
        let json = serde_json::json!({
            "name": "auto-accept-agent",
            "description": "Agent with dontAsk permission mode",
            "permissionMode": "dontAsk",
            "promptBody": "## Auto-accept Mode"
        });
        let def = AgentDefinition::from_json(&json).unwrap();
        assert_eq!(def.permission_mode, PermissionMode::Default);
        assert_eq!(def.prompt_body.as_deref(), Some("## Auto-accept Mode"));
    }
    #[test]
    fn test_from_json_empty_prompt_body_is_none() {
        let json = serde_json::json!({
            "name": "test",
            "description": "Test",
            "promptBody": "   "
        });
        let def = AgentDefinition::from_json(&json).unwrap();
        assert!(
            def.prompt_body.is_none(),
            "Whitespace-only promptBody should be None"
        );
    }
    #[test]
    fn test_from_json_missing_required_fields() {
        let json = serde_json::json!({
            "description": "Missing name"
        });
        let result = AgentDefinition::from_json(&json);
        assert!(result.is_err());
    }
    #[test]
    fn test_from_json_rejects_unknown_fields() {
        let json = serde_json::json!({
            "name": "test",
            "description": "Test",
            "unknownField": "value",
            "futureFeature": true
        });
        assert!(AgentDefinition::from_json(&json).is_err());
    }
    #[test]
    fn to_json_value_roundtrips_through_from_json() {
        let mut original = AgentDefinition::parse(
                "---\nname: test-agent\ndescription: A test\npermissionMode: dontAsk\n---\nYou are a helper.",
            )
            .unwrap();
        original.tools = vec!["read_file".to_string(), "grep".to_string()];
        original.disallowed_tools = vec!["custom_tool".to_string()];
        let json = original.to_json_value();
        let recovered = AgentDefinition::from_json(&json).unwrap();
        assert_eq!(recovered.name, "test-agent");
        assert_eq!(recovered.description, "A test");
        assert_eq!(recovered.prompt_body.as_deref(), Some("You are a helper."));
        assert_eq!(recovered.permission_mode, PermissionMode::Default);
        assert_eq!(recovered.tools, vec!["read_file", "grep"]);
        assert_eq!(recovered.disallowed_tools, vec!["custom_tool"]);
    }
    #[test]
    fn test_model_frontmatter_is_rejected() {
        let content = "---\nname: test\ndescription: Test\nmodel: grow-3-fast\n---\n";
        assert!(AgentDefinition::parse(content).is_err());
    }
    #[test]
    fn test_model_override_in_json_is_rejected() {
        let json = serde_json::json!({
            "name": "test",
            "description": "Test",
            "model": "grow-code-fast-1"
        });
        assert!(AgentDefinition::from_json(&json).is_err());
    }
    #[test]
    fn test_builtin_agent_name_strum_round_trip() {
        use std::str::FromStr;
        for (s, expected) in [
            ("grow", BuiltinAgentName::Grow),
            ("grow-build-concise", BuiltinAgentName::GrowConcise),
            ("general-purpose", BuiltinAgentName::GeneralPurpose),
            ("explore", BuiltinAgentName::Explore),
            ("browser-use", BuiltinAgentName::BrowserUse),
        ] {
            let parsed = BuiltinAgentName::from_str(s).unwrap();
            assert_eq!(parsed, expected, "from_str failed for: {s}");
            assert_eq!(parsed.as_ref(), s, "as_ref failed for: {s}");
        }
    }
    #[test]
    fn test_builtin_agent_name_unknown_returns_err() {
        use std::str::FromStr;
        assert!(BuiltinAgentName::from_str("nonexistent").is_err());
        assert!(BuiltinAgentName::from_str("not-a-builtin-agent").is_err());
    }
    #[test]
    fn test_builtin_agent_name_definition_names_match() {
        use strum::IntoEnumIterator;
        for variant in BuiltinAgentName::iter() {
            let def = variant.definition();
            assert_eq!(
                def.name,
                variant.as_ref(),
                "definition().name doesn't match as_ref() for {:?}",
                variant
            );
        }
    }
    #[test]
    fn test_builtin_agent_name_subagent_variants() {
        let variants = BuiltinAgentName::subagent_variants();
        assert_eq!(variants.len(), 3);
        assert!(variants.contains(&BuiltinAgentName::GeneralPurpose));
        assert!(variants.contains(&BuiltinAgentName::Explore));
        assert!(variants.contains(&BuiltinAgentName::BrowserUse));
        assert_eq!(
            variants,
            vec![
                BuiltinAgentName::GeneralPurpose,
                BuiltinAgentName::Explore,
                BuiltinAgentName::BrowserUse
            ]
        );
    }
    #[test]
    fn mcp_inheritance_default_when_omitted() {
        let def = AgentDefinition::parse("---\nname: t\ndescription: t\n---\n").unwrap();
        assert_eq!(def.mcp_inheritance, McpInheritance::All);
    }
    #[test]
    fn mcp_inheritance_all_parses() {
        let def =
            AgentDefinition::parse("---\nname: t\ndescription: t\nmcpInheritance: all\n---\n")
                .unwrap();
        assert_eq!(def.mcp_inheritance, McpInheritance::All);
    }
    #[test]
    fn mcp_inheritance_none_parses() {
        let def =
            AgentDefinition::parse("---\nname: t\ndescription: t\nmcpInheritance: none\n---\n")
                .unwrap();
        assert_eq!(def.mcp_inheritance, McpInheritance::None);
    }
    #[test]
    fn mcp_inheritance_named_parses() {
        let content = "---\nname: t\ndescription: t\nmcpInheritance:\n  named:\n    - slack\n    - github\n---\n";
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(
            def.mcp_inheritance,
            McpInheritance::Named(vec!["slack".into(), "github".into()])
        );
    }
    #[test]
    fn mcp_inheritance_except_parses() {
        let content =
            "---\nname: t\ndescription: t\nmcpInheritance:\n  except:\n    - internal\n---\n";
        let def = AgentDefinition::parse(content).unwrap();
        assert_eq!(
            def.mcp_inheritance,
            McpInheritance::Except(vec!["internal".into()])
        );
    }
    #[test]
    fn mcp_inheritance_round_trips_via_json() {
        let json = serde_json::json!({
            "name": "t",
            "description": "t",
            "mcpInheritance": {"named": ["a", "b"]}
        });
        let def = AgentDefinition::from_json(&json).unwrap();
        assert_eq!(
            def.mcp_inheritance,
            McpInheritance::Named(vec!["a".into(), "b".into()])
        );
        let serialized = def.to_json_value();
        let recovered = AgentDefinition::from_json(&serialized).unwrap();
        assert_eq!(recovered.mcp_inheritance, def.mcp_inheritance);
    }
    fn def_with_template(tpl: crate::prompt::context::TemplateOverride) -> AgentDefinition {
        let mut def = AgentDefinition::default_grow_build();
        def.system_prompt = tpl;
        def
    }
    #[test]
    fn carries_discipline_false_for_every_template_and_audience() {
        for tpl in [
            crate::prompt::context::TemplateOverride::None,
            crate::prompt::context::TemplateOverride::Custom("fake".to_string()),
        ] {
            let def = def_with_template(tpl.clone());
            for audience in [
                crate::prompt::context::PromptAudience::Primary,
                crate::prompt::context::PromptAudience::Subagent,
            ] {
                assert!(
                    !def.carries_task_completion_discipline(audience),
                    "discipline block was removed; helper must return false \
                     (template: {tpl:?}, audience: {audience:?})"
                );
            }
        }
    }
}
