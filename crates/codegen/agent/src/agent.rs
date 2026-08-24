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
            prompt_context,
            system_prompt,
            role_prompt,
            tool_bridge,
        }
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
    /// Used by the runtime turn-end TodoGate together with
    /// [`crate::AgentDefinition::carries_task_completion_discipline`] to
    /// decide whether the active prompt actually carries the discipline
    /// rules the gate's reminder text invokes.
    pub fn prompt_audience(&self) -> crate::prompt::context::PromptAudience {
        self.prompt_context.audience
    }

    /// Tool definitions for the sampling API — delegates to ToolBridge.
    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_bridge.tool_definitions().await
    }

    /// Built-in tool definitions only (excludes MCP tools).
    pub async fn tool_definitions_builtins_only(&self) -> Vec<ToolDefinition> {
        self.tool_bridge.tool_definitions_builtins_only().await
    }
}
