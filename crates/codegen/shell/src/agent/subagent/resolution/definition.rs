//! Production subagent definition discovery and tool-policy resolution.
use super::types::{EffectiveRuntimeConfig, ResolutionError};
use agent::config::{AgentDefinition, Effort, IsolationMode};
use agent::plugins::PluginRegistry;
use std::collections::HashMap;
use std::path::Path;
use tool_types::SubagentIsolationMode;
use tools::implementations::grow_build::task::types::SubagentRuntimeOverrides;
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

fn authored_eligibility(
    definition: &mut AgentDefinition,
) -> &mut tools::registry::types::ToolServerConfig {
    if definition.authored_capability_tools.is_none() {
        definition.authored_capability_tools = Some(definition.tool_config.clone());
    }
    definition
        .authored_capability_tools
        .as_mut()
        .expect("initialized above")
}

/// Apply recursion depth to the immutable authored eligibility snapshot while
/// keeping the live schema stable. The dispatcher, not visibility filtering,
/// enforces the resulting forbidden status.
pub fn apply_child_tool_policy(definition: &mut AgentDefinition, allow_nested_subagents: bool) {
    if !allow_nested_subagents {
        authored_eligibility(definition)
            .tools
            .retain(|tool| tool.kind != Some(ToolKind::Task));
    }
}

/// A delegated child may read its immutable Goal snapshot, but only the
/// owning primary Session may mutate Goal lifecycle state.
pub fn apply_goal_object_tool_policy(definition: &mut AgentDefinition) {
    authored_eligibility(definition).tools.retain(|tool| {
        tool.kind
            .is_some_and(|kind| kind != ToolKind::GoalLifecycleUpdate)
    });
}
/// Resolve runtime overrides and definition defaults in the production order.
pub fn resolve_runtime_config(
    overrides: &SubagentRuntimeOverrides,
    definition: &AgentDefinition,
) -> EffectiveRuntimeConfig {
    EffectiveRuntimeConfig {
        model: overrides.model.clone(),
        reasoning_effort: overrides.reasoning_effort.clone().or_else(|| {
            definition
                .effort
                .map(|effort| Some(<&str>::from(effort).to_string()))
        }),
        capability_mode: overrides
            .capability_mode
            .or(definition.capability_mode)
            .unwrap_or_default(),
        isolation: overrides.isolation.unwrap_or_else(|| {
            if definition.isolation == Some(IsolationMode::Worktree) {
                SubagentIsolationMode::Worktree
            } else {
                SubagentIsolationMode::None
            }
        }),
    }
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
    fn builtin_explore_keeps_execute_latent() {
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
    fn max_depth_keeps_task_visible_but_removes_authored_eligibility() {
        let cwd = tempfile::tempdir().unwrap();
        let toggles = HashMap::new();
        let mut definition =
            resolve_agent_definition("general-purpose", &context(cwd.path(), &toggles)).unwrap();
        assert!(
            definition
                .tool_config
                .tools
                .iter()
                .any(|tool| tool.kind == Some(ToolKind::Task))
        );
        apply_child_tool_policy(&mut definition, false);
        assert!(
            definition
                .tool_config
                .tools
                .iter()
                .any(|tool| tool.kind == Some(ToolKind::Task))
        );
        assert!(
            !definition
                .authored_capability_tools
                .as_ref()
                .unwrap()
                .tools
                .iter()
                .any(|tool| tool.kind == Some(ToolKind::Task))
        );
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
        let runtime = resolve_runtime_config(&SubagentRuntimeOverrides::default(), &definition);
        assert_eq!(runtime.isolation, SubagentIsolationMode::Worktree);
        assert_eq!(
            runtime.capability_mode,
            tool_types::SubagentCapabilityMode::ReadOnly
        );

        let explicit = resolve_runtime_config(
            &SubagentRuntimeOverrides {
                capability_mode: Some(tool_types::SubagentCapabilityMode::Execute),
                ..Default::default()
            },
            &definition,
        );
        assert_eq!(
            explicit.capability_mode,
            tool_types::SubagentCapabilityMode::Execute
        );

        let general =
            resolve_agent_definition("general-purpose", &context(cwd.path(), &toggles)).unwrap();
        assert_eq!(
            resolve_runtime_config(&SubagentRuntimeOverrides::default(), &general).capability_mode,
            tool_types::SubagentCapabilityMode::ReadWrite
        );
    }

    #[test]
    fn explicit_reasoning_disable_does_not_inherit_definition_effort() {
        let cwd = tempfile::tempdir().unwrap();
        let toggles = HashMap::new();
        let mut definition =
            resolve_agent_definition("general-purpose", &context(cwd.path(), &toggles)).unwrap();
        definition.effort = Some(Effort::High);

        let runtime = resolve_runtime_config(
            &SubagentRuntimeOverrides {
                reasoning_effort: Some(None),
                ..Default::default()
            },
            &definition,
        );

        assert_eq!(runtime.reasoning_effort, Some(None));
    }

    #[test]
    fn goal_object_policy_keeps_lifecycle_visible_but_forbidden() {
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
        assert!(kinds.contains(&Some(ToolKind::GoalLifecycleUpdate)));
        let eligible_kinds: Vec<_> = definition
            .authored_capability_tools
            .as_ref()
            .unwrap()
            .tools
            .iter()
            .filter_map(|tool| tool.kind)
            .collect();
        assert!(eligible_kinds.contains(&ToolKind::GoalRead));
        assert!(!eligible_kinds.contains(&ToolKind::GoalLifecycleUpdate));

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
                .any(|tool| tool.id == "custom:opaque")
        );
    }
}
