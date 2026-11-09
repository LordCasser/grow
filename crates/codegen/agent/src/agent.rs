//! Agent — a fully built agent: definition + session context.

use std::sync::Arc;

use tools::bridge::ToolBridge;
use tools::types::definition::ToolDefinition;

use crate::config::{AgentDefinition, CompletionRequirement};
use crate::prompt::context::PromptContext;

/// A fully built agent: definition + session context.
///
/// NOT portable — tied to a specific session via its ToolBridge and rendered
/// prompt layers. Runtime lifecycle policy remains owned by the host session.
///
/// Created by AgentBuilder from an AgentDefinition + session context.
///
/// The Agent is effectively immutable after construction. It holds
/// Arc<ToolBridge> — mutations to tool state (MCP registration,
/// completion tracking, retry config) go through ToolBridge's
/// internal locks.
pub struct Agent {
    /// The definition this agent was built from.
    definition: AgentDefinition,

    /// Toggle snapshot used to project this Agent's tool catalog.
    subagent_toggle: std::collections::HashMap<String, bool>,

    /// The context that produced the current system prompt.
    /// Stored for inspection, re-rendering, and serialization.
    prompt_context: PromptContext,

    /// The rendered stable system prompt (cached from `PromptContext::render`).
    system_prompt: String,

    /// The rendered Agent-authored role. The shell projects this through an
    /// append-only Timeline Control event instead of mutating `system_prompt`.
    role_prompt: Option<String>,

    /// The tool bridge — owns ToolRegistry + ToolState + SessionContext.
    tool_bridge: Arc<ToolBridge>,
}

impl Agent {
    /// Create a new Agent.
    ///
    /// Normally called by `AgentBuilder::build()`. Exposed publicly for
    /// test helpers that need to construct an Agent with a pre-built ToolBridge.
    pub fn new(
        definition: AgentDefinition,
        prompt_context: PromptContext,
        system_prompt: String,
        role_prompt: Option<String>,
        tool_bridge: Arc<ToolBridge>,
    ) -> Self {
        Self {
            definition,
            subagent_toggle: Default::default(),
            prompt_context,
            system_prompt,
            role_prompt,
            tool_bridge,
        }
    }

    pub(crate) fn with_subagent_toggle(
        mut self,
        toggle: std::collections::HashMap<String, bool>,
    ) -> Self {
        self.subagent_toggle = toggle;
        self
    }

    pub fn subagent_toggle(&self) -> &std::collections::HashMap<String, bool> {
        &self.subagent_toggle
    }

    // ── From definition ──────────────────────────────────────────────

    /// Agent name (unique identifier).
    pub fn name(&self) -> &str {
        &self.definition.name
    }

    /// Agent description.
    pub fn description(&self) -> &str {
        &self.definition.description
    }

    /// The full agent definition.
    pub fn definition(&self) -> &AgentDefinition {
        &self.definition
    }

    /// Completion requirement, if any.
    pub fn completion_requirement(&self) -> Option<&CompletionRequirement> {
        self.definition.completion_requirement.as_ref()
    }

    // ── Session-level ────────────────────────────────────────────────

    /// The rendered system prompt.
    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// Rendered Agent-authored role for the Timeline `system.role` layer.
    pub fn role_prompt(&self) -> Option<&str> {
        self.role_prompt.as_deref()
    }

    /// The tool bridge for this agent.
    pub fn tool_bridge(&self) -> &Arc<ToolBridge> {
        &self.tool_bridge
    }

    /// Activate this staged harness over the session-owned live tool state.
    /// Agent construction keeps the bridge unique until this boundary so the
    /// resource-domain swap cannot affect the currently sampling harness.
    pub async fn activate_resource_domain(
        &mut self,
        domain: &tools::registry::types::SessionResourceDomain,
    ) -> Result<(), String> {
        let bridge = Arc::get_mut(&mut self.tool_bridge)
            .ok_or_else(|| "staged Agent ToolBridge is already shared".to_string())?;
        bridge.activate_resource_domain(domain).await
    }

    pub async fn prepare_resource_domain_activation(
        &mut self,
        domain: &tools::registry::types::SessionResourceDomain,
    ) -> Result<tools::registry::types::PreparedResourceActivation, String> {
        let bridge = Arc::get_mut(&mut self.tool_bridge)
            .ok_or_else(|| "staged Agent ToolBridge is already shared".to_string())?;
        bridge.prepare_resource_domain_activation(domain).await
    }

    /// Cached AGENTS.md section (derived from prompt_context).
    pub fn agents_md_section(&self) -> Option<String> {
        self.prompt_context.format_agents_md_section()
    }

    /// AGENTS.md content formatted for user-message injection.
    ///
    /// Returns the `<system-reminder>` block to prepend as a user message,
    /// respecting audience (compacted for subagents) and template.
    pub fn agents_md_user_reminder(&self) -> Option<String> {
        self.prompt_context.agents_md_user_reminder()
    }

    /// Audience this agent's prompt was rendered for (Primary or Subagent).
    ///
    pub fn prompt_audience(&self) -> crate::prompt::context::PromptAudience {
        self.prompt_context.audience
    }

    /// Tool definitions valid for direct sampling API declarations.
    /// The complete bridge catalog remains available to meta-tool discovery.
    /// Long catalog identities are not truncated or aliased for providers.
    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_bridge
            .tool_definitions()
            .await
            .into_iter()
            .filter(|definition| provider_tool_name_valid(&definition.function.name))
            .collect()
    }

    /// Built-in tool definitions only (excludes MCP tools).
    pub async fn tool_definitions_builtins_only(&self) -> Vec<ToolDefinition> {
        self.tool_bridge.tool_definitions_builtins_only().await
    }
}

// Keep this projection local: MCP catalog names have a different length bound.
fn provider_tool_name_valid(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tools::implementations::use_tool::UseTool;

    #[tokio::test]
    async fn sampling_definitions_preserve_provider_names_and_complete_bridge_catalog() {
        let bridge = Arc::new(ToolBridge::for_test());
        let prefix = format!("server__{}", "a".repeat(56));
        assert_eq!(prefix.len(), 64);
        let names = [
            "use_tool".to_owned(),
            "server__short".to_owned(),
            prefix.clone(),
            format!("{prefix}a"),
            format!("{prefix}b"),
            format!("{prefix}{}", "a".repeat(192)),
            "1invalid".to_owned(),
            "server:scope__tool".to_owned(),
        ];
        for (index, name) in names.iter().enumerate() {
            bridge.toolset().register_tool(
                name.clone(),
                UseTool,
                Some(serde_json::json!({"type": "object", "properties": {"value": {"const": index}}})),
            ).unwrap();
        }
        let catalog = bridge.tool_definitions().await;
        assert_eq!(catalog.len(), names.len());
        let agent = Agent::new(
            AgentDefinition::default_grow_build(),
            PromptContext::default(),
            String::new(),
            None,
            Arc::clone(&bridge),
        );
        let projection = agent.tool_definitions().await;
        let expected: Vec<_> = catalog
            .iter()
            .filter(|definition| names[..3].contains(&definition.function.name))
            .cloned()
            .collect();
        assert_eq!(
            serde_json::to_value(&projection).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(projection.len(), 3);
        assert_eq!(
            serde_json::to_value(bridge.tool_definitions().await).unwrap(),
            serde_json::to_value(catalog).unwrap()
        );
    }

    #[test]
    fn provider_name_format_remains_ascii_and_bounded() {
        for name in ["", "1tool", "-tool", "a.b", "a:b", "é", "a\n"] {
            assert!(!provider_tool_name_valid(name), "accepted {name:?}");
        }
        for name in ["_tool", "Tool_1-name", &"a".repeat(64)] {
            assert!(provider_tool_name_valid(name), "rejected {name:?}");
        }
        assert!(!provider_tool_name_valid(&"a".repeat(65)));
    }
}
