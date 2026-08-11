//! Production subagent definition discovery and tool-policy resolution.
use super::config::{SubagentPersona, SubagentRole};
use super::types::{EffectiveRuntimeConfig, ResolutionError};
use agent::config::{AgentDefinition, IsolationMode};
use agent::plugins::PluginRegistry;
use std::collections::HashMap;
use std::path::Path;
use tool_types::SubagentIsolationMode;
use tools::implementations::grow_build::task::types::{
    SubagentRuntimeOverrides, prune_orphaned_background_task_tools,
};
use tools::registry::types::ToolConfig;
use tools::types::tool::ToolKind;
/// Inputs that affect definition discovery and global enablement.
pub struct DefinitionResolutionContext<'a> {
    pub cwd: &'a Path,
    pub plugins: Option<&'a PluginRegistry>,
    pub cli_agents: &'a [AgentDefinition],
    pub toggles: &'a HashMap<String, bool>,
}
/// Inputs for validating a type when only session CLI names are available.
pub struct DefinitionValidationContext<'a> {
    pub cwd: &'a Path,
    pub plugins: Option<&'a PluginRegistry>,
    pub cli_agent_names: &'a [String],
    pub toggles: &'a HashMap<String, bool>,
}
/// Parent/runtime inputs that choose the production child harness flavor.
pub struct HarnessToolsetContext<'a> {
    pub harness_override: Option<&'a str>,
    pub parent_agent_name: Option<&'a str>,
    pub file_tool_overrides: Option<&'a [ToolConfig]>,
}
/// `false` twin: the alternate flavors re-select toolset presets and
/// templates, so none is representable when the optional harness is compiled
/// out. Keeps ungated call sites compiling.
pub fn subagent_harness_flavor_is_representable(_agent_type: &str) -> bool {
    false
}
/// Apply the production parent/harness-dependent child toolset selection.
pub fn apply_harness_toolset(
    #[allow(unused_variables)] subagent_type: &str,
    context: &HarnessToolsetContext<'_>,
    definition: &mut AgentDefinition,
) {
    let flavor_agent = context.harness_override.or_else(|| {
        context
            .parent_agent_name
            .filter(|name| subagent_harness_flavor_is_representable(name))
    });
    if flavor_agent.is_some_and(subagent_harness_flavor_is_representable) {
    } else if let Some(file_tools) = context.file_tool_overrides {
        definition.override_file_tools(file_tools.to_vec());
    }
}
/// Discover the same project/builtin/user/plugin definition used by production,
/// with session CLI definitions as the final fallback.
pub fn discover_agent_definition(
    subagent_type: &str,
    context: &DefinitionResolutionContext<'_>,
) -> Option<AgentDefinition> {
    agent::discovery::by_name_in_cwd_with_plugins(subagent_type, context.cwd, context.plugins)
        .or_else(|| {
            context
                .cli_agents
                .iter()
                .find(|definition| definition.name == subagent_type)
                .cloned()
        })
}
/// Sorted model-facing names available under the current discovery context.
pub fn available_agent_names(context: &DefinitionResolutionContext<'_>) -> Vec<String> {
    let mut available: Vec<String> =
        agent::discovery::all_subagents_with_plugins(context.cwd, context.toggles, context.plugins)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
    for definition in context.cli_agents {
        if context
            .toggles
            .get(&definition.name)
            .copied()
            .unwrap_or(true)
            && !available.contains(&definition.name)
        {
            available.push(definition.name.clone());
        }
    }
    available.sort();
    available
}
/// Apply the production global toggle.
pub fn gate_agent_definition(
    subagent_type: &str,
    context: &DefinitionResolutionContext<'_>,
) -> Result<(), ResolutionError> {
    if !context.toggles.get(subagent_type).copied().unwrap_or(true) {
        return Err(ResolutionError::Disabled {
            subagent_type: subagent_type.to_string(),
        });
    }
    Ok(())
}
/// Validate discovery and the global toggle without cloning definitions.
pub fn validate_agent_name(
    subagent_type: &str,
    context: &DefinitionValidationContext<'_>,
) -> Result<(), ResolutionError> {
    let resolves = context
        .cli_agent_names
        .iter()
        .any(|name| name == subagent_type)
        || agent::discovery::by_name_in_cwd_with_plugins(
            subagent_type,
            context.cwd,
            context.plugins,
        )
        .is_some();
    if !resolves {
        let mut available: Vec<String> = agent::discovery::all_subagents_with_plugins(
            context.cwd,
            context.toggles,
            context.plugins,
        )
        .into_iter()
        .map(|entry| entry.name)
        .collect();
        for name in context.cli_agent_names {
            if context.toggles.get(name).copied().unwrap_or(true) && !available.contains(name) {
                available.push(name.clone());
            }
        }
        available.sort();
        return Err(ResolutionError::Unknown {
            subagent_type: subagent_type.to_owned(),
            available,
        });
    }
    let gate_context = DefinitionResolutionContext {
        cwd: context.cwd,
        plugins: context.plugins,
        cli_agents: &[],
        toggles: context.toggles,
    };
    gate_agent_definition(subagent_type, &gate_context)
}
/// Discover and gate one production agent definition.
pub fn resolve_agent_definition(
    subagent_type: &str,
    context: &DefinitionResolutionContext<'_>,
) -> Result<AgentDefinition, ResolutionError> {
    let definition = discover_agent_definition(subagent_type, context).ok_or_else(|| {
        ResolutionError::Unknown {
            subagent_type: subagent_type.to_string(),
            available: available_agent_names(context),
        }
    })?;
    gate_agent_definition(subagent_type, context)?;
    Ok(definition)
}

/// Resolve an exact host-owned Goal stage profile. These definitions bypass
/// ordinary discovery deliberately: project/user Agents cannot shadow them,
/// and the general Task tool cannot name them.
pub fn resolve_goal_stage_definition(
    subagent_type: &str,
    role: tools::implementations::grow_build::task::types::GoalSubagentRole,
) -> Option<AgentDefinition> {
    use tools::implementations::grow_build::task::types::GoalSubagentRole;
    match (role, subagent_type) {
        (GoalSubagentRole::Planner, "goal-planner") => Some(AgentDefinition::goal_planner()),
        (GoalSubagentRole::Verifier, "goal-verifier") => Some(AgentDefinition::goal_verifier()),
        _ => None,
    }
}
/// Resolve the role selected by production: type-specific first, then persona.
pub fn select_role<'a>(
    subagent_type: &str,
    overrides: &SubagentRuntimeOverrides,
    roles: &'a HashMap<String, SubagentRole>,
) -> (Option<&'a SubagentRole>, Option<String>) {
    if let Some(role) = roles.get(subagent_type) {
        return (Some(role), Some(subagent_type.to_string()));
    }
    let Some(persona) = overrides.persona.as_deref() else {
        return (None, None);
    };
    match roles.get(persona) {
        Some(role) => (Some(role), Some(persona.to_string())),
        None => (None, None),
    }
}
/// Fill runtime values whose defaults live on the resolved agent definition.
pub fn apply_definition_runtime_defaults(
    runtime: &mut EffectiveRuntimeConfig,
    definition: &AgentDefinition,
) {
    if runtime.capability_mode.is_none() {
        runtime.capability_mode = definition.capability_mode;
    }
    if runtime.reasoning_effort.is_none() {
        runtime.reasoning_effort = definition
            .effort
            .map(|effort| <&str>::from(effort).to_string());
    }
    if runtime.isolation == SubagentIsolationMode::None
        && definition.isolation == Some(IsolationMode::Worktree)
    {
        runtime.isolation = SubagentIsolationMode::Worktree;
    }
}
/// Apply capability filtering and recursion depth to the exact production
/// definition toolset.
pub fn apply_child_tool_policy(definition: &mut AgentDefinition, allow_nested_subagents: bool) {
    if !allow_nested_subagents {
        definition
            .tool_config
            .tools
            .retain(|tool| tool.kind != Some(ToolKind::Task));
        prune_orphaned_background_task_tools(&mut definition.tool_config);
    }
}

/// Restrict Goal-owned children to the immutable blackboard view. Even an
/// initial `All` grant cannot restore an object mutation the delegated Goal
/// role does not own.
pub fn apply_goal_object_tool_policy(definition: &mut AgentDefinition) {
    definition.tool_config.tools.retain(|tool| {
        tool.kind.is_some_and(|kind| {
            !matches!(
                kind,
                ToolKind::GoalProgressUpdate
                    | ToolKind::GoalReplanRequest
                    | ToolKind::GoalLifecycleUpdate
            )
        })
    });
}
/// Resolve runtime overrides and definition defaults in the production order.
pub fn resolve_runtime_config(
    subagent_type: &str,
    overrides: &SubagentRuntimeOverrides,
    roles: &HashMap<String, SubagentRole>,
    personas: &HashMap<String, SubagentPersona>,
    cwd: Option<&Path>,
    definition: &AgentDefinition,
) -> EffectiveRuntimeConfig {
    let (role, role_name) = select_role(subagent_type, overrides, roles);
    let mut runtime =
        super::overrides::resolve_effective_overrides(overrides, role, personas, cwd, role_name);
    apply_definition_runtime_defaults(&mut runtime, definition);
    runtime
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context<'a>(
        cwd: &'a Path,
        toggles: &'a HashMap<String, bool>,
    ) -> DefinitionResolutionContext<'a> {
        DefinitionResolutionContext {
            cwd,
            plugins: None,
            cli_agents: &[],
            toggles,
        }
    }
    #[test]
    fn builtin_explore_keeps_execute_latent_but_respects_depth() {
        let cwd = tempfile::tempdir().unwrap();
        let toggles = HashMap::new();
        let mut definition =
            resolve_agent_definition("explore", &context(cwd.path(), &toggles)).unwrap();
        apply_child_tool_policy(&mut definition, false);
        let kinds: Vec<Option<ToolKind>> = definition
            .tool_config
            .tools
            .iter()
            .map(|tool| tool.kind)
            .collect();
        assert!(kinds.contains(&Some(ToolKind::Read)));
        assert!(kinds.contains(&Some(ToolKind::Search)));
        assert!(kinds.contains(&Some(ToolKind::Execute)));
        assert!(!kinds.iter().any(|kind| matches!(
            kind,
            Some(ToolKind::Edit | ToolKind::Write | ToolKind::Delete | ToolKind::Move)
        )));
        assert!(!kinds.contains(&Some(ToolKind::Task)));
    }
    #[test]
    fn gates_disabled_definitions() {
        let cwd = tempfile::tempdir().unwrap();
        let toggles = HashMap::from([("explore".to_string(), false)]);
        let disabled = context(cwd.path(), &toggles);
        assert!(matches!(
            resolve_agent_definition("explore", &disabled),
            Err(ResolutionError::Disabled { .. })
        ));
    }
    #[test]
    fn definition_defaults_fill_runtime_without_overwriting_explicit_values() {
        let cwd = tempfile::tempdir().unwrap();
        let toggles = HashMap::new();
        let mut definition =
            resolve_agent_definition("explore", &context(cwd.path(), &toggles)).unwrap();
        definition.isolation = Some(IsolationMode::Worktree);
        let mut runtime = EffectiveRuntimeConfig::default();
        apply_definition_runtime_defaults(&mut runtime, &definition);
        assert_eq!(runtime.isolation, SubagentIsolationMode::Worktree);
    }

    #[test]
    fn goal_object_policy_keeps_read_and_rejects_every_mutation() {
        let cwd = tempfile::tempdir().unwrap();
        let toggles = HashMap::new();
        let mut definition =
            resolve_agent_definition("general-purpose", &context(cwd.path(), &toggles)).unwrap();
        apply_goal_object_tool_policy(&mut definition);
        let kinds: Vec<Option<ToolKind>> = definition
            .tool_config
            .tools
            .iter()
            .map(|tool| tool.kind)
            .collect();
        assert!(kinds.contains(&Some(ToolKind::GoalRead)));
        assert!(!kinds.contains(&Some(ToolKind::GoalProgressUpdate)));
        assert!(!kinds.contains(&Some(ToolKind::GoalReplanRequest)));
        assert!(!kinds.contains(&Some(ToolKind::GoalLifecycleUpdate)));

        definition
            .tool_config
            .tools
            .push(ToolConfig::from_id("custom:opaque"));
        apply_goal_object_tool_policy(&mut definition);
        assert!(
            definition
                .tool_config
                .tools
                .iter()
                .all(|tool| tool.kind.is_some())
        );
    }

    #[test]
    fn host_goal_profiles_are_role_bound_and_minimal() {
        use tools::implementations::grow_build::task::types::GoalSubagentRole;
        let planner = resolve_goal_stage_definition("goal-planner", GoalSubagentRole::Planner)
            .expect("planner profile");
        let verifier = resolve_goal_stage_definition("goal-verifier", GoalSubagentRole::Verifier)
            .expect("verifier profile");
        assert!(
            resolve_goal_stage_definition("goal-verifier", GoalSubagentRole::Planner).is_none()
        );
        assert!(
            planner
                .tool_config
                .tools
                .iter()
                .all(|tool| !matches!(tool.kind, Some(ToolKind::Execute | ToolKind::Task)))
        );
        assert!(
            verifier
                .tool_config
                .tools
                .iter()
                .any(|tool| tool.kind == Some(ToolKind::Execute))
        );
        assert_eq!(verifier.isolation, Some(IsolationMode::Worktree));
        let verifier_kinds: Vec<_> = verifier
            .tool_config
            .tools
            .iter()
            .map(|tool| tool.kind)
            .collect();
        assert!(
            verifier.tool_config.tools.iter().all(|tool| {
                matches!(
                    tool.kind,
                    Some(
                        ToolKind::Execute
                            | ToolKind::Read
                            | ToolKind::ListDir
                            | ToolKind::List
                            | ToolKind::Search
                            | ToolKind::GoalRead
                    )
                )
            }),
            "verifier kinds: {verifier_kinds:?}"
        );
    }
}
