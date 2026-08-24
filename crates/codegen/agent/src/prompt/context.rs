//! First-class system prompt rendering context.
//!
//! `PromptContext` captures the agent-specific inputs to prompt rendering
//! in memory. The rendered stable system core and mutable Agent role are
//! recorded separately in Timeline; this builder state is never a parallel
//! persistence format.
use crate::config::PromptComposition;
use crate::prompt::agents_md::{self, AgentConfigFile};
use crate::prompt::template::{
    MANDATORY_CORE_PROMPT, PRIMARY_AUDIENCE_PROMPT, SESSION_EXTENSIONS_PROMPT, STANDARD_PROMPT,
    SUBAGENT_AUDIENCE_PROMPT,
};
use serde::{Deserialize, Serialize};
/// Controls which base template and catalog sections are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptAudience {
    /// Top-level interactive session. Full base template, all catalog sections.
    #[default]
    Primary,
    /// Child/subagent session with a compact base template.
    Subagent,
}
use tools::types::template_renderer::TemplateRenderer;
/// Agent-specific inputs for system prompt rendering.
///
/// Rendering goes through `ToolBridge::render_prompt()`.
#[derive(Debug, Clone)]
pub struct PromptContext {
    /// Which prompt mode produced this context.
    pub prompt_composition: PromptComposition,
    /// Whether this is a primary (parent) or subagent (child) session.
    /// Controls base template choice and catalog section rendering.
    pub audience: PromptAudience,
    /// Agent role body. It is rendered separately from the stable system head
    /// and enters Timeline as an append-only `system.role` control context.
    /// In Full mode the optional standard guidance is omitted, while the
    /// mandatory foundation and audience layers remain.
    pub prompt_body: Option<String>,
    /// AGENTS.md files discovered during build, in precedence order
    /// (repo root → CWD; deeper files override).
    pub agents_md_files: Vec<AgentConfigFile>,
    /// Whether the memory system is enabled for this session.
    /// When true, the system prompt includes a `<memory>` section telling
    /// the model it can use `memory_search` and `memory_get`.
    pub memory_enabled: bool,
    /// Whether the agent is running in a non-interactive (headless / SDK /
    /// stdio / generic-ACP).
    pub is_non_interactive: bool,
    /// Identity in the primary grow-build system prompt (`You are <label>…`).
    /// Not the UI picker name. Defaults to [`DEFAULT_SYSTEM_PROMPT_LABEL`].
    pub system_prompt_label: String,
}
/// Default identity on trim-tool-descriptions (`You are Grow`).
pub const DEFAULT_SYSTEM_PROMPT_LABEL: &str = "Grow";
fn default_system_prompt_label() -> String {
    DEFAULT_SYSTEM_PROMPT_LABEL.to_string()
}
impl Default for PromptContext {
    fn default() -> Self {
        Self {
            prompt_composition: PromptComposition::Extend,
            audience: PromptAudience::default(),
            prompt_body: None,
            agents_md_files: vec![],
            memory_enabled: false,
            is_non_interactive: false,
            system_prompt_label: default_system_prompt_label(),
        }
    }
}
impl PromptContext {
    /// Format the AGENTS.md section as a `<system-reminder>` block.
    ///
    /// Returns `None` if no AGENTS.md files were discovered.
    pub fn format_agents_md_section(&self) -> Option<String> {
        agents_md::format_agents_md_section(&self.agents_md_files)
    }
    /// AGENTS.md content for injection as a prepended user message.
    ///
    /// - Subagents and primary sessions both get the full block, so a child
    ///   verifier sees the same project instructions as the main agent.
    pub fn agents_md_user_reminder(&self) -> Option<String> {
        self.format_agents_md_section()
    }
    /// Build the placeholder JSON for template rendering.
    ///
    /// These are the agent-specific values that get merged with the
    /// tool context in `TemplateRenderer::render_with_extra()`.
    pub fn placeholders(&self) -> serde_json::Value {
        serde_json::json!({
            "memory_enabled": self.memory_enabled,
            "is_non_interactive": self.is_non_interactive,
            "system_prompt_label": self.system_prompt_label.as_str(),
        })
    }
    /// Render the stable system head without consulting mutable tool state.
    ///
    /// The head contains only the mandatory core and fixed audience. Mutable
    /// Agent policy, tool guidance, memory capability, and role prose are
    /// deliberately excluded; use
    /// [`Self::render_role`] for the Timeline control projection.
    pub fn render(&self) -> Option<String> {
        let renderer = TemplateRenderer::new(Default::default(), Default::default());
        self.render_with_renderer(&renderer)
    }

    /// Render mutable Agent-scoped guidance through the finalized tool-name
    /// renderer for its Timeline control projection.
    pub async fn render_role(&self, tool_bridge: &tools::bridge::ToolBridge) -> Option<String> {
        let renderer = tool_bridge.template_renderer_snapshot().await?;
        self.render_role_with_renderer(&renderer)
    }
    /// Render the stable system head from a finalized renderer.
    pub fn render_with_renderer(&self, renderer: &TemplateRenderer) -> Option<String> {
        let placeholders = self.placeholders();
        let render = |template: &str| renderer.render_with_extra(template, &placeholders).ok();
        let mut sections = vec![render(MANDATORY_CORE_PROMPT)?];
        sections.push(render(match self.audience {
            PromptAudience::Primary => PRIMARY_AUDIENCE_PROMPT,
            PromptAudience::Subagent => SUBAGENT_AUDIENCE_PROMPT,
        })?);
        let prompt = sections
            .into_iter()
            .filter(|section| !section.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        Some(prompt)
    }

    /// Render the complete mutable Agent layer. `Extend` includes Grow's
    /// standard guidance before the authored role; `Full` omits that optional
    /// guidance. Session extensions live here because Agent switches can
    /// change the available tools.
    pub fn render_role_with_renderer(&self, renderer: &TemplateRenderer) -> Option<String> {
        let placeholders = self.placeholders();
        let render = |template: &str| {
            renderer
                .render_with_extra(template, &placeholders)
                .unwrap_or_else(|_| template.to_owned())
        };
        let mut sections = Vec::new();
        if self.prompt_composition == PromptComposition::Extend {
            sections.push(render(STANDARD_PROMPT));
        }
        if let Some(body) = self.prompt_body.as_deref() {
            sections.push(render(body));
        }
        sections.push(render(SESSION_EXTENSIONS_PROMPT));
        let prompt = sections
            .into_iter()
            .filter(|section| !section.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        (!prompt.is_empty()).then_some(prompt)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_context() -> PromptContext {
        PromptContext {
            prompt_composition: PromptComposition::Extend,
            audience: PromptAudience::Primary,
            prompt_body: None,
            agents_md_files: vec![],
            memory_enabled: false,
            is_non_interactive: false,
            system_prompt_label: default_system_prompt_label(),
        }
    }
    #[test]
    fn test_placeholders_contains_agent_fields() {
        let ctx = test_context();
        let p = ctx.placeholders();
        assert_eq!(p["memory_enabled"], false);
        assert_eq!(p["system_prompt_label"], DEFAULT_SYSTEM_PROMPT_LABEL);
    }
    #[test]
    fn test_placeholders_system_prompt_label_override() {
        let mut ctx = test_context();
        ctx.system_prompt_label = "Grow Internal".into();
        let p = ctx.placeholders();
        assert_eq!(p["system_prompt_label"], "Grow Internal");
    }
    #[test]
    fn test_placeholders_include_session_extension_fields() {
        let mut ctx = test_context();
        ctx.memory_enabled = true;
        let p = ctx.placeholders();
        assert_eq!(p["memory_enabled"], true);
    }
    #[test]
    fn test_placeholders_memory_enabled() {
        let mut ctx = test_context();
        ctx.memory_enabled = true;
        let p = ctx.placeholders();
        assert_eq!(p["memory_enabled"], true);
    }
    #[test]
    fn test_default_context() {
        let ctx = PromptContext::default();
        assert!(matches!(ctx.prompt_composition, PromptComposition::Extend));
        assert!(ctx.prompt_body.is_none());
        assert!(ctx.agents_md_files.is_empty());
    }
    #[test]
    fn test_format_agents_md_section_empty() {
        let ctx = test_context();
        assert!(ctx.format_agents_md_section().is_none());
    }
    #[test]
    fn test_format_agents_md_section_non_empty() {
        let mut ctx = test_context();
        ctx.agents_md_files = vec![AgentConfigFile {
            file_name: "AGENTS.md".to_string(),
            file_path: "/repo/AGENTS.md".to_string(),
            content: "# Instructions".to_string(),
        }];
        let section = ctx.format_agents_md_section().unwrap();
        assert!(section.contains("# Instructions"));
        assert!(section.contains("<system-reminder>"));
    }
    /// AGENTS.md must reach the system prompt for the default template even
    /// AGENTS.md user reminder must be present for the default template
    /// when files are present.
    #[test]
    fn agents_md_user_reminder_included_for_default_template() {
        let mut ctx = test_context();
        ctx.agents_md_files = vec![AgentConfigFile {
            file_name: "AGENTS.md".to_string(),
            file_path: "/repo/AGENTS.md".to_string(),
            content: "# XYZZY_AGENTS_MD_MARKER".to_string(),
        }];
        let section = ctx
            .agents_md_user_reminder()
            .expect("default template must include AGENTS.md user reminder when files exist");
        assert!(section.contains("<system-reminder>"));
        assert!(section.contains("XYZZY_AGENTS_MD_MARKER"));
    }
    fn child_general_purpose_context() -> PromptContext {
        use crate::prompt::subagent_prompts;
        PromptContext {
            prompt_composition: PromptComposition::Extend,
            audience: PromptAudience::Subagent,
            prompt_body: Some(subagent_prompts::GENERAL_PURPOSE_PROMPT.to_string()),
            agents_md_files: vec![],
            memory_enabled: true,
            is_non_interactive: false,
            system_prompt_label: default_system_prompt_label(),
        }
    }
    #[test]
    fn child_prompt_uses_subagent_audience() {
        let ctx = child_general_purpose_context();
        assert_eq!(ctx.audience, super::PromptAudience::Subagent);
    }
    #[test]
    fn child_prompt_includes_agents_md_when_present() {
        let mut ctx = child_general_purpose_context();
        ctx.agents_md_files = vec![AgentConfigFile {
            file_name: "AGENTS.md".to_string(),
            file_path: "/workspace/AGENTS.md".to_string(),
            content: "Build with `cargo build`".to_string(),
        }];
        let section = ctx.format_agents_md_section();
        assert!(
            section.is_some(),
            "child prompt should include AGENTS.md when files are discovered"
        );
    }
    #[test]
    fn child_prompt_no_agents_md_when_empty() {
        let ctx = child_general_purpose_context();
        let section = ctx.format_agents_md_section();
        assert!(
            section.is_none(),
            "child prompt has no AGENTS.md when none discovered"
        );
    }
    #[test]
    fn child_prompt_delivers_full_agents_md() {
        use crate::prompt::agents_md::AgentConfigFile;
        let mut ctx = child_general_purpose_context();
        ctx.agents_md_files = vec![AgentConfigFile {
            file_name: "AGENTS.md".to_string(),
            file_path: "/repo/AGENTS.md".to_string(),
            content: "X".repeat(5000),
        }];
        assert_eq!(ctx.audience, super::PromptAudience::Subagent);
        let reminder = ctx.agents_md_user_reminder().unwrap();
        assert!(
            reminder.contains(&"X".repeat(5000)),
            "child must receive full AGENTS.md content"
        );
        assert!(
            !reminder.contains("truncated"),
            "child AGENTS.md must not be truncated"
        );
    }
    #[test]
    fn child_prompt_uses_extend_mode() {
        let ctx = child_general_purpose_context();
        assert!(
            matches!(ctx.prompt_composition, PromptComposition::Extend),
            "CURRENT: child uses Extend mode (inherits full base template)"
        );
    }
    #[test]
    fn child_prompt_has_prompt_body() {
        let ctx = child_general_purpose_context();
        assert!(
            ctx.prompt_body.is_some(),
            "CURRENT: child has a prompt body (GENERAL_PURPOSE_PROMPT)"
        );
        let body = ctx.prompt_body.as_deref().unwrap();
        assert!(
            body.contains("Strengths") && body.contains("Guidelines"),
            "body should contain structured general-purpose guidance sections"
        );
    }
    #[test]
    fn child_prompt_placeholders_include_memory_and_workspace() {
        let ctx = child_general_purpose_context();
        let placeholders = ctx.placeholders();
        assert_eq!(
            placeholders.get("memory_enabled").and_then(|v| v.as_bool()),
            Some(true)
        );
    }
    #[test]
    fn parent_vs_child_section_differences() {
        let parent = test_context();
        let child = child_general_purpose_context();
        assert_eq!(parent.audience, super::PromptAudience::Primary);
        assert_eq!(child.audience, super::PromptAudience::Subagent);
        assert!(child.memory_enabled);
        assert!(child.prompt_body.is_some());
        assert!(parent.prompt_body.is_none());
    }
    #[test]
    fn child_prompt_context_is_complete() {
        let ctx = child_general_purpose_context();
        assert!(ctx.prompt_body.is_some());
        assert!(matches!(ctx.prompt_composition, PromptComposition::Extend));
        assert_eq!(ctx.audience, super::PromptAudience::Subagent);
        assert!(ctx.memory_enabled);
        let p = ctx.placeholders();
        assert!(p.get("memory_enabled").is_some());
    }
    fn render_subagent_template(ctx: minijinja::Value) -> String {
        let mut env = minijinja::Environment::new();
        env.set_syntax(
            minijinja::syntax::SyntaxConfig::builder()
                .block_delimiters("${%", "%}")
                .variable_delimiters("${{", "}}")
                .comment_delimiters("${#", "#}")
                .build()
                .unwrap(),
        );
        let prompt = format!(
            "{}\n\n{}\n\n{}\n\n{}",
            crate::prompt::template::MANDATORY_CORE_PROMPT,
            crate::prompt::template::SUBAGENT_AUDIENCE_PROMPT,
            crate::prompt::template::STANDARD_PROMPT,
            crate::prompt::template::SESSION_EXTENSIONS_PROMPT,
        );
        env.add_template_owned("prompt", prompt).unwrap();
        env.get_template("prompt").unwrap().render(ctx).unwrap()
    }
    fn base_template_ctx() -> minijinja::Value {
        minijinja::context! {
            memory_enabled => true,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "hashline_read",
                    edit => "hashline_edit",
                    search => "hashline_grep",
                    execute => "run_terminal_cmd",
                    background_task_action => "get_task_output",
                    memory_search => "memory_search",
                    memory_get => "memory_get",
                }
            },
        }
    }
    #[test]
    fn child_rendered_prompt_includes_memory_section() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.contains("<memory>"),
            "should contain <memory> section"
        );
        assert!(
            rendered.contains("memory_search"),
            "should reference memory_search"
        );
    }
    #[test]
    fn child_rendered_prompt_includes_project_instructions_like_main_agent() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.contains("<project_instructions_spec>"),
            "subagent must include project_instructions_spec"
        );
        assert!(
            rendered.contains("Each `AGENTS.md` applies"),
            "subagent project instructions must match the mandatory core"
        );
        assert!(
            rendered.contains("Check for an applicable nested `AGENTS.md`"),
            "subagent must be told to proactively check nested AGENTS.md"
        );
    }
    #[test]
    fn child_rendered_prompt_excludes_parent_only_sections() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(!rendered.contains("## Task Management"));
        assert!(!rendered.contains("## No time estimates"));
    }
    #[test]
    fn child_rendered_prompt_has_hashline_guidance() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.contains("For hashline file tools"),
            "should include hashline guidance"
        );
        assert!(
            rendered.contains("Edit batches are atomic"),
            "should include atomic batch semantics"
        );
    }
    #[test]
    fn child_rendered_prompt_has_background_tasks_when_execute_available() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.contains("<background_tasks>"),
            "should include background_tasks section when execute tool exists"
        );
        assert!(
            rendered.contains("background"),
            "background_tasks should mention background flag"
        );
    }
    #[test]
    fn child_rendered_prompt_has_code_change_rules_when_edit_available() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.contains("Use `hashline_edit` for ordinary file creation and editing"),
            "should include edit-tool guidance when edit is available"
        );
    }
    #[test]
    fn child_rendered_prompt_omits_background_tasks_without_execute() {
        let ctx = minijinja::context! {
            memory_enabled => false,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "hashline_read",
                    edit => "hashline_edit",
                    search => "hashline_grep",
                }
            },
        };
        let rendered = render_subagent_template(ctx);
        assert!(
            !rendered.contains("<background_tasks>"),
            "background_tasks should be absent without execute tool"
        );
        assert!(
            rendered.contains("For hashline file tools"),
            "hashline guidance should still be present"
        );
    }
    #[test]
    fn child_rendered_template_is_compact() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.len() < 5000,
            "rendered child template too large: {} chars",
            rendered.len()
        );
    }
    #[test]
    fn child_rendered_prompt_omits_code_change_rules_without_edit_tools() {
        let ctx = minijinja::context! {
            memory_enabled => false,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "hashline_read",
                    search => "hashline_grep",
                    execute => "run_terminal_cmd",
                    background_task_action => "get_task_output",
                }
            },

        };
        let rendered = render_subagent_template(ctx);
        assert!(!rendered.contains("for ordinary file creation and editing"));
        assert!(rendered.contains("<tool_calling>"));
        assert!(rendered.contains("<background_tasks>"));
        assert!(rendered.contains("<output>"));
    }
    #[test]
    fn rendered_prompt_size_general_purpose() {
        let rendered = render_subagent_template(base_template_ctx());
        assert!(
            rendered.len() < 5000,
            "general-purpose rendered prompt: {} chars (ceiling 5000)",
            rendered.len()
        );
    }
    #[test]
    fn rendered_prompt_size_read_only() {
        let ctx = minijinja::context! {
            memory_enabled => false,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "hashline_read",
                    search => "hashline_grep",
                    execute => "run_terminal_cmd",
                    background_task_action => "get_task_output",
                }
            },

        };
        let rendered = render_subagent_template(ctx);
        assert!(
            rendered.len() < 4500,
            "read-only rendered prompt: {} chars (ceiling 4500)",
            rendered.len()
        );
        let full = render_subagent_template(base_template_ctx());
        assert!(
            rendered.len() < full.len(),
            "read-only prompt ({}) should be smaller than general-purpose ({})",
            rendered.len(),
            full.len()
        );
    }
    #[test]
    fn child_rendered_prompt_omits_edit_references_without_edit_tool() {
        let ctx = minijinja::context! {
            memory_enabled => false,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "read_file",
                    search => "grep",
                    execute => "run_terminal_cmd",
                    background_task_action => "get_task_output",
                }
            },

        };
        let rendered = render_subagent_template(ctx);
        assert!(
            !rendered.contains("for editing"),
            "should not mention editing when edit tool is absent"
        );
    }
    #[test]
    fn child_rendered_prompt_omits_execute_references_without_execute_tool() {
        let ctx = minijinja::context! {
            memory_enabled => false,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "read_file",
                    edit => "search_replace",
                    search => "grep",
                }
            },
        };
        let rendered = render_subagent_template(ctx);
        assert!(
            !rendered.contains("system commands"),
            "should not mention system commands when execute tool is absent"
        );
        assert!(
            !rendered.contains("Reserve"),
            "should not mention Reserve (bash) when execute tool is absent"
        );
    }
    #[test]
    fn child_rendered_prompt_omits_both_edit_and_execute_references() {
        let ctx = minijinja::context! {
            memory_enabled => false,
            tools => minijinja::context! {
                by_kind => minijinja::context! {
                    read => "read_file",
                    search => "grep",
                }
            },
        };
        let rendered = render_subagent_template(ctx);
        assert!(
            !rendered.contains("for editing"),
            "should not mention editing"
        );
        assert!(
            !rendered.contains("system commands"),
            "should not mention system commands"
        );
        assert!(!rendered.contains("Reserve"), "should not mention Reserve");
        assert!(
            rendered.contains("Use `read_file` for file reading"),
            "tool_calling line should end cleanly after read reference"
        );
    }
    #[test]
    fn agent_role_is_separate_from_the_stable_system_head() {
        let mut ctx = test_context();
        ctx.prompt_body = Some("ROLE_LAYER_SENTINEL".to_string());
        let renderer = tools::types::template_renderer::TemplateRenderer::new(
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        );
        let prompt = ctx.render_with_renderer(&renderer).unwrap();
        assert!(!prompt.contains("ROLE_LAYER_SENTINEL"), "{prompt}");
        assert!(!prompt.contains("Work with the user"), "{prompt}");
        assert!(!prompt.contains("<runtime_context>"), "{prompt}");
        assert!(!prompt.contains("<behavior-context>"), "{prompt}");
        let role = ctx.render_role_with_renderer(&renderer).unwrap();
        assert!(role.contains("Work with the user"), "{role}");
        assert!(role.contains("ROLE_LAYER_SENTINEL"), "{role}");
    }

    #[test]
    fn full_role_composition_does_not_mutate_the_stable_head() {
        let renderer = tools::types::template_renderer::TemplateRenderer::new(
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        );
        let mut extend = test_context();
        extend.prompt_body = Some("ROLE_LAYER_SENTINEL".to_string());
        let mut full = extend.clone();
        full.prompt_composition = PromptComposition::Full;

        assert_eq!(
            extend.render_with_renderer(&renderer),
            full.render_with_renderer(&renderer)
        );
        let extend_role = extend.render_role_with_renderer(&renderer).unwrap();
        let full_role = full.render_role_with_renderer(&renderer).unwrap();
        assert!(extend_role.contains("Work with the user"));
        assert!(!full_role.contains("Work with the user"));
        assert!(full_role.contains("ROLE_LAYER_SENTINEL"));
    }
    /// Verify that AGENTS.md file paths rewritten to the display cwd are
    /// rendered into the system prompt correctly. When `AgentConfigFile.file_path`
    /// uses the display path, the rendered `## From:` line must not contain
    /// the overlay/worktree path.
    #[test]
    fn test_agents_md_paths_use_display_cwd_in_rendered_section() {
        let display_path = "/home/user/my-project";
        let overlay_path = "/root/.grow/worktrees/my-project/ab-123-a-overlay";
        let ctx = PromptContext {
            agents_md_files: vec![AgentConfigFile {
                file_name: "AGENTS.md".to_string(),
                file_path: format!("{display_path}/AGENTS.md"),
                content: "# Project rules".to_string(),
            }],
            ..test_context()
        };
        let section = ctx.format_agents_md_section().unwrap();
        assert!(
            section.contains(&format!("## From: {display_path}/AGENTS.md")),
            "rendered AGENTS section must show the display path"
        );
        assert!(
            !section.contains(overlay_path),
            "rendered AGENTS section must not contain the overlay path"
        );
    }
    #[test]
    fn built_in_prompts_do_not_contain_user_info_block() {
        let gp = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        let explore = super::super::subagent_prompts::EXPLORE_PROMPT;
        assert!(
            !gp.contains("OS: linux"),
            "prompt text should not contain actual OS value"
        );
        assert!(
            !explore.contains("Workspace Path:"),
            "prompt text should not contain Workspace Path field"
        );
    }
    #[test]
    fn workspace_boundary_in_general_purpose_prompt() {
        let prompt = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        assert!(
            prompt.contains("Workspace boundary"),
            "general-purpose prompt should contain workspace boundary guidance"
        );
        assert!(
            prompt.contains("<user_info>"),
            "general-purpose prompt should reference <user_info>"
        );
    }
    #[test]
    fn workspace_boundary_in_explore_prompt() {
        let prompt = super::super::subagent_prompts::EXPLORE_PROMPT;
        assert!(
            prompt.contains("Workspace boundary"),
            "explore prompt should contain workspace boundary guidance"
        );
        assert!(
            prompt.contains("default search scope"),
            "explore should mention default search scope"
        );
    }
    #[test]
    fn general_purpose_prompt_specialization_keywords() {
        let prompt = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        let keywords = [
            "broad searches",
            "Multi-file analysis",
            "NEVER create files",
            "documentation files",
            "absolute file paths",
        ];
        for kw in &keywords {
            assert!(
                prompt.contains(kw),
                "general-purpose prompt missing specialization keyword: {kw}"
            );
        }
    }
    #[test]
    fn explore_prompt_specialization_keywords() {
        let prompt = super::super::subagent_prompts::EXPLORE_PROMPT;
        let keywords = [
            "read-only",
            "READ-ONLY MODE",
            "glob patterns",
            "regex",
            "parallel tool calls",
            "thoroughness level",
        ];
        for kw in &keywords {
            assert!(
                prompt.contains(kw),
                "explore prompt missing specialization keyword: {kw}"
            );
        }
    }
    #[test]
    fn trimmed_prompts_are_compact() {
        let gp = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        let explore = super::super::subagent_prompts::EXPLORE_PROMPT;
        assert!(
            gp.len() < 1200,
            "general-purpose prompt too large: {} chars",
            gp.len()
        );
        assert!(
            explore.len() < 1050,
            "explore prompt too large: {} chars",
            explore.len()
        );
    }
    #[test]
    fn trimmed_prompts_no_redundant_identity() {
        let gp = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        let explore = super::super::subagent_prompts::EXPLORE_PROMPT;
        for (name, prompt) in [("general-purpose", gp), ("explore", explore)] {
            assert!(
                !prompt.contains("You are a Grow agent"),
                "{name} prompt should not duplicate base template identity"
            );
        }
    }
    #[test]
    fn trimmed_prompts_no_redundant_formatting_rules() {
        let gp = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        let explore = super::super::subagent_prompts::EXPLORE_PROMPT;
        for (name, prompt) in [("general-purpose", gp), ("explore", explore)] {
            assert!(
                !prompt.contains("avoid using emojis"),
                "{name} prompt should not duplicate formatting rules from base template"
            );
        }
    }
    #[test]
    fn all_prompts_reference_tool_templates() {
        let gp = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        let explore = super::super::subagent_prompts::EXPLORE_PROMPT;
        for (name, prompt) in [("general-purpose", gp), ("explore", explore)] {
            assert!(
                prompt.contains("${{ tools.by_kind."),
                "{name} prompt should reference tool template variables"
            );
        }
    }
    #[test]
    fn explore_prompt_declares_read_only_constraint() {
        let explore = super::super::subagent_prompts::EXPLORE_PROMPT;
        assert!(explore.contains("NO file editing tools"));
        assert!(explore.contains("Do not create, modify, or delete"));
        let gp = super::super::subagent_prompts::GENERAL_PURPOSE_PROMPT;
        assert!(
            !gp.contains("READ-ONLY MODE"),
            "general-purpose should not be read-only"
        );
    }
}
