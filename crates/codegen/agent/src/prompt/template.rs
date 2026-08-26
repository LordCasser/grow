//! Built-in system prompt sources and rendering tests.
//!
//! Markdown files under `prompts/` are the single source of truth. Rust embeds
//! them directly so packaged binaries do not depend on runtime prompt files.

pub(crate) const MANDATORY_CORE_PROMPT: &str =
    include_str!("../../prompts/foundation/mandatory-core.md");
pub(crate) const STANDARD_PROMPT: &str = include_str!("../../prompts/foundation/standard.md");
pub(crate) const PRIMARY_AUDIENCE_PROMPT: &str = include_str!("../../prompts/audience/primary.md");
pub(crate) const SUBAGENT_AUDIENCE_PROMPT: &str =
    include_str!("../../prompts/audience/subagent.md");
pub(crate) const SESSION_EXTENSIONS_PROMPT: &str =
    include_str!("../../prompts/extensions/session.md");
#[cfg(test)]
pub(crate) const DEFAULT_SYSTEM_PROMPT: &str = MANDATORY_CORE_PROMPT;
#[cfg(test)]
pub(crate) const SUBAGENT_SYSTEM_PROMPT: &str = SUBAGENT_AUDIENCE_PROMPT;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tools::types::template_renderer::TemplateRenderer;
    use tools::types::tool::ToolKind;

    /// Build a TemplateRenderer with the standard grow-build tool kinds.
    fn default_renderer() -> TemplateRenderer {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Read, "read_file"),
            (ToolKind::Edit, "search_replace"),
            (ToolKind::Execute, "run_terminal_command"),
            (ToolKind::Search, "grep"),
            (ToolKind::List, "list_dir"),
            (ToolKind::Plan, "todo_write"),
            (ToolKind::Skill, "skill"),
            (
                ToolKind::BackgroundTaskAction,
                "get_command_or_subagent_output",
            ),
            (ToolKind::KillTaskAction, "kill_command_or_subagent"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect();
        TemplateRenderer::new(tools, HashMap::new())
    }

    fn default_placeholders() -> serde_json::Value {
        serde_json::json!({
            "memory_enabled": false,
            "is_non_interactive": false,
            "system_prompt_label": crate::prompt::context::DEFAULT_SYSTEM_PROMPT_LABEL,
        })
    }

    fn render_base(renderer: &TemplateRenderer, placeholders: &serde_json::Value) -> String {
        renderer
            .render_with_extra(DEFAULT_SYSTEM_PROMPT, placeholders)
            .expect("base template render failed")
    }

    fn render_subagent(renderer: &TemplateRenderer, placeholders: &serde_json::Value) -> String {
        renderer
            .render_with_extra(SUBAGENT_SYSTEM_PROMPT, placeholders)
            .expect("subagent template render failed")
    }

    fn render_extend_layer(
        renderer: &TemplateRenderer,
        placeholders: &serde_json::Value,
    ) -> String {
        [STANDARD_PROMPT, SESSION_EXTENSIONS_PROMPT]
            .into_iter()
            .filter_map(|template| renderer.render_with_extra(template, placeholders).ok())
            .filter(|section| !section.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    // ── Variable substitution ───────────────────────────────────────

    #[test]
    fn test_variable_substitution_tool_kind() {
        let r = default_renderer();
        let p = default_placeholders();
        let result = r
            .render_with_extra("Use ${{ tools.by_kind.read }} to read files.", &p)
            .unwrap();
        assert_eq!(result, "Use read_file to read files.");
    }

    // ── Conditionals ────────────────────────────────────────────────

    #[test]
    fn test_conditional_tool_present() {
        let r = default_renderer();
        let p = default_placeholders();
        let result = r
            .render_with_extra("${%- if tools.by_kind.plan %}show${%- endif %}", &p)
            .unwrap();
        assert_eq!(result, "show");
    }

    #[test]
    fn test_conditional_tool_absent() {
        // Renderer without plan tool
        let tools: HashMap<ToolKind, String> = [(ToolKind::Read, "read_file".to_string())].into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let p = default_placeholders();
        let result = r
            .render_with_extra("${%- if tools.by_kind.plan %}show${%- endif %}", &p)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_literal_braces_pass_through() {
        let r = default_renderer();
        let p = default_placeholders();
        let result = r
            .render_with_extra("Use {{ literal_braces }} in prose.", &p)
            .unwrap();
        assert_eq!(result, "Use {{ literal_braces }} in prose.");
    }

    // ── Tool name overrides ─────────────────────────────────────────

    #[test]
    fn test_tool_name_override() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Read, "view_file".to_string()),
            (ToolKind::Edit, "Edit".to_string()),
        ]
        .into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let p = default_placeholders();
        let result = r
            .render_with_extra(
                "Use ${{ tools.by_kind.read }} and ${{ tools.by_kind.edit }}.",
                &p,
            )
            .unwrap();
        assert_eq!(result, "Use view_file and Edit.");
    }

    // ── Base template rendering ─────────────────────────────────────

    #[test]
    fn test_base_template_renders() {
        let prompt = render_base(&default_renderer(), &default_placeholders());
        assert!(prompt.contains("<instruction_priority>"));
        assert!(prompt.contains("<tool_calling>"));
    }

    #[test]
    fn stable_head_is_independent_of_agent_tool_names() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Read, "view_file".to_string()),
            (ToolKind::Edit, "edit".to_string()),
            (ToolKind::Execute, "run_terminal_cmd".to_string()),
            (ToolKind::Search, "grep".to_string()),
            (ToolKind::Plan, "todo_write".to_string()),
            (
                ToolKind::BackgroundTaskAction,
                "get_task_output".to_string(),
            ),
        ]
        .into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let standard = render_base(&default_renderer(), &default_placeholders());
        let renamed = render_base(&r, &default_placeholders());
        assert_eq!(standard, renamed);
        assert!(!renamed.contains("view_file"));
        assert!(!renamed.contains("edit"));
    }

    #[test]
    fn test_base_template_execute_absent_omits_background_tasks() {
        // Renderer without Execute tool
        let tools: HashMap<ToolKind, String> = [(ToolKind::Plan, "todo_write".to_string())].into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let prompt = render_extend_layer(&r, &default_placeholders());
        assert!(
            !prompt.contains("background_tasks"),
            "background_tasks section should be omitted"
        );
    }

    #[test]
    fn test_monitor_tool_renders_watch_section() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Execute, "run_command".to_string()),
            (ToolKind::BackgroundTaskAction, "get_output".to_string()),
            (ToolKind::KillTaskAction, "kill_task".to_string()),
            (ToolKind::Monitor, "monitor".to_string()),
        ]
        .into_iter()
        .collect();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let prompt = render_extend_layer(&r, &default_placeholders());
        assert!(
            prompt.contains("For watch processes"),
            "monitor section should render when Monitor tool is present"
        );
        assert!(
            prompt.contains("streams each stdout line back as a chat notification"),
            "monitor section should describe streaming stdout as notifications"
        );
        assert!(
            prompt.contains("use `monitor`"),
            "monitor section should resolve the Monitor tool name"
        );
    }

    #[test]
    fn test_no_monitor_tool_omits_watch_section() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Execute, "run_command".to_string()),
            (ToolKind::BackgroundTaskAction, "get_output".to_string()),
            (ToolKind::KillTaskAction, "kill_task".to_string()),
        ]
        .into_iter()
        .collect();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let prompt = render_extend_layer(&r, &default_placeholders());
        assert!(
            !prompt.contains("For watch processes"),
            "monitor section should NOT render without Monitor tool"
        );
        assert!(!prompt.contains("`monitor`"));
        assert!(
            prompt.contains("<background_tasks>"),
            "background execution guidance remains available without Monitor"
        );
    }

    // ── Required sections regression ────────────────────────────────

    #[test]
    fn test_base_template_contains_required_sections() {
        let p = default_placeholders();
        let prompt = render_base(&default_renderer(), &p);
        assert!(prompt.contains("<instruction_priority>"));
        assert!(prompt.contains("<action_safety>"));
        assert!(prompt.contains("<tool_calling>"));
        assert!(prompt.contains("<project_instructions_spec>"));
    }

    #[test]
    fn test_mid_session_switch_preserves_tool_names() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Read, "view".to_string()),
            (ToolKind::Edit, "edit".to_string()),
            (ToolKind::Execute, "run_terminal_cmd".to_string()),
            (ToolKind::Plan, "todo_write".to_string()),
            (
                ToolKind::BackgroundTaskAction,
                "get_task_output".to_string(),
            ),
        ]
        .into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let prompt = render_extend_layer(&r, &default_placeholders());
        assert!(prompt.contains("`edit`"), "Should use overridden 'edit'");
        assert!(prompt.contains("`view`"), "Should use overridden 'view'");
        assert!(
            !prompt.contains("`read_file`"),
            "Should not contain original 'read_file'"
        );
        assert!(
            !prompt.contains("`search_replace`"),
            "Should not contain original 'search_replace'"
        );
    }

    // ── Audience ownership contract ────────────────────────────────

    #[test]
    fn primary_audience_retains_task_wide_synthesis() {
        assert!(PRIMARY_AUDIENCE_PROMPT.contains("task-wide understanding"));
        assert!(PRIMARY_AUDIENCE_PROMPT.contains("not to hand off the problem as a whole"));
        assert!(PRIMARY_AUDIENCE_PROMPT.contains("While delegated work runs"));
        assert!(PRIMARY_AUDIENCE_PROMPT.contains("Wait only when"));
    }

    #[test]
    fn subagent_audience_returns_evidence_without_expanding_scope() {
        assert!(SUBAGENT_AUDIENCE_PROMPT.contains("supporting evidence and paths"));
        assert!(SUBAGENT_AUDIENCE_PROMPT.contains("instead of silently expanding"));
    }

    // ── Determinism ─────────────────────────────────────────────────

    #[test]
    fn test_prompt_deterministic_across_renders() {
        let r = default_renderer();
        let p = default_placeholders();
        let a = render_base(&r, &p);
        let b = render_base(&r, &p);
        assert_eq!(a, b, "Prompt rendering must be deterministic");
    }

    #[test]
    fn test_full_mode_deterministic() {
        let r = default_renderer();
        let p = default_placeholders();
        let body = "Agent: ${{ tools.by_kind.read }}, label: ${{ system_prompt_label }}";
        let a = r.render_with_extra(body, &p).unwrap();
        let b = r.render_with_extra(body, &p).unwrap();
        assert_eq!(a, b, "Full mode rendering must be deterministic");
    }

    // ── Disabled tools ──────────────────────────────────────────────

    #[test]
    fn test_disabled_tools_omit_sections() {
        // No plan, no execute
        let tools: HashMap<ToolKind, String> = [(ToolKind::Read, "read_file".to_string())].into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let prompt = render_extend_layer(&r, &default_placeholders());
        assert!(
            !prompt.contains("Task Management"),
            "Task Management must be omitted"
        );
        assert!(
            !prompt.contains("background_tasks"),
            "background_tasks must be omitted"
        );
    }

    // ── Memory section ──────────────────────────────────────────────

    #[test]
    fn memory_capability_renders_only_in_the_agent_layer() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Read, "read_file".to_string()),
            (ToolKind::MemorySearch, "memory_search".to_string()),
            (ToolKind::MemoryGet, "memory_get".to_string()),
        ]
        .into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let mut p = default_placeholders();
        p["memory_enabled"] = serde_json::json!(true);
        let head = render_base(&r, &p);
        let layer = render_extend_layer(&r, &p);
        assert!(!head.contains("<memory>"));
        assert!(!head.contains("memory_search"));
        assert!(layer.contains("<memory>"));
        assert!(layer.contains("memory_search"));
        assert!(layer.contains("memory_get"));
    }

    #[test]
    fn test_memory_disabled_omits_memory_section() {
        let prompt = render_base(&default_renderer(), &default_placeholders());
        assert!(
            !prompt.contains("<memory>"),
            "Memory section must be omitted"
        );
    }

    // ── Optional tools absent ───────────────────────────────────────

    #[test]
    fn test_optional_tools_absent_renders_without_crash() {
        let tools: HashMap<ToolKind, String> = [
            (ToolKind::Read, "read_file".to_string()),
            (ToolKind::Plan, "todo_write".to_string()),
        ]
        .into();
        let r = TemplateRenderer::new(tools, HashMap::new());
        let result = r.render_with_extra(DEFAULT_SYSTEM_PROMPT, &default_placeholders());
        assert!(
            result.is_ok(),
            "Must render without crash: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_subagent_template_deterministic_across_renders() {
        let r = default_renderer();
        let p = default_placeholders();
        let a = render_subagent(&r, &p);
        let b = render_subagent(&r, &p);
        assert_eq!(a, b, "Subagent template rendering must be deterministic");
    }

    // ── Task completion discipline ─────────────────────────────────
    //
    // The `<task_completion_discipline>` block was removed from both
    // base and subagent templates. These tests pin the deletion so the
    // block doesn't accidentally come back.

    #[test]
    fn task_completion_discipline_block_is_not_rendered() {
        let prompt = render_base(&default_renderer(), &default_placeholders());
        assert!(
            !prompt.contains("<task_completion_discipline>"),
            "discipline block was removed from the base template"
        );
        let subagent = render_subagent(&default_renderer(), &default_placeholders());
        assert!(
            !subagent.contains("<task_completion_discipline>"),
            "discipline block was removed from the subagent template"
        );
    }

    /// Soft byte ceiling shared by both prompt-size budget tests.
    /// Forward-budget guard against runaway growth, not a tight target.
    const PROMPT_SIZE_SOFT_CEILING_BYTES: usize = 16384;

    fn assert_template_size_under(prompt: &str, label: &str) {
        assert!(
            prompt.len() < PROMPT_SIZE_SOFT_CEILING_BYTES,
            "{label} prompt is {} bytes, exceeding soft ceiling of {} bytes",
            prompt.len(),
            PROMPT_SIZE_SOFT_CEILING_BYTES,
        );
    }

    #[test]
    fn test_base_template_size_budget() {
        let prompt = render_base(&default_renderer(), &default_placeholders());
        assert_template_size_under(&prompt, "base");
    }

    #[test]
    fn test_subagent_template_size_budget() {
        let prompt = render_subagent(&default_renderer(), &default_placeholders());
        assert_template_size_under(&prompt, "subagent");
    }

    // ── Guard invariant ─────────────────────────────────────────────
    // Every `${{ tools.by_kind.X }}` must sit inside a `${%- if ... %}`
    // whose condition requires X (contains `tools.by_kind.X` at a word
    // boundary, with no top-level ` or `). If violated, X could render
    // as empty string at runtime.

    fn word_bounded(hay: &str, needle: &str) -> bool {
        let mut s = 0;
        while let Some(i) = hay[s..].find(needle) {
            let end = s + i + needle.len();
            match hay[end..].chars().next() {
                None => return true,
                Some(c) if !(c.is_alphanumeric() || c == '_') => return true,
                _ => s += i + 1,
            }
        }
        false
    }

    fn guarantees(cond: &str, kind: &str) -> bool {
        if word_bounded(cond, &format!("tools.by_kind.{kind}")) && !cond.contains(" or ") {
            return true;
        }
        false
    }

    fn assert_guards(template: &str, label: &str) {
        let bytes = template.as_bytes();
        let mut stack: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut i = 0;
        while i + 2 < bytes.len() {
            let three = &bytes[i..i + 3];
            if three == b"${%" {
                let end = bytes[i + 3..]
                    .windows(2)
                    .position(|w| w == b"%}")
                    .map(|e| i + 3 + e + 2)
                    .unwrap_or(bytes.len());
                let body = std::str::from_utf8(&bytes[i + 3..end - 2])
                    .unwrap()
                    .trim_matches(['-', ' ']);
                if let Some(c) = body.strip_prefix("if ") {
                    stack.push(c.trim().into());
                } else if let Some(c) = body.strip_prefix("elif ") {
                    stack.pop();
                    stack.push(c.trim().into());
                } else if body == "else" {
                    stack.pop();
                    stack.push("<else>".into());
                } else if body == "endif" {
                    stack.pop();
                }
                i = end;
            } else if three == b"${{" {
                let end = bytes[i + 3..]
                    .windows(2)
                    .position(|w| w == b"}}")
                    .map(|e| i + 3 + e + 2)
                    .unwrap_or(bytes.len());
                let body = std::str::from_utf8(&bytes[i + 3..end - 2]).unwrap().trim();
                // search_tool and use_tool are always built-in, so they
                // never need a guard.
                const ALWAYS_BUILTIN: &[&str] = &["search_tool", "use_tool"];
                if let Some(kind) = body.strip_prefix("tools.by_kind.")
                    && kind.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !ALWAYS_BUILTIN.contains(&kind)
                    && !stack.iter().any(|c| guarantees(c, kind))
                {
                    let line = template[..i].lines().count() + 1;
                    errors.push(format!(
                        "{label}:{line}: unguarded `${{{{ tools.by_kind.{kind} }}}}` (stack: {stack:?})"
                    ));
                }
                i = end;
            } else {
                i += 1;
            }
        }
        assert!(errors.is_empty(), "\n  {}", errors.join("\n  "));
    }

    #[test]
    fn test_template_vars_are_always_guarded() {
        assert_guards(DEFAULT_SYSTEM_PROMPT, "foundation/mandatory-core.md");
        assert_guards(SUBAGENT_SYSTEM_PROMPT, "audience/subagent.md");
    }

    // ── Combination sweep ───────────────────────────────────────────
    // Belt-and-braces: renders the base template across tool-kind subsets
    // and asserts no raw template tokens leak. The static guard test above
    // is the authoritative check; this one just catches syntax drift.

    // ── is_non_interactive gating ──────────────────────────────────
    // Headless / SDK / stdio / generic-ACP sessions have no human typing
    // into a TUI prompt, so the `! <command>` shell-prefix tip and the
    // `<user_guide>` TUI pointer are noise. Those sections must drop out
    // when `is_non_interactive=true` and remain when it's false.

    #[test]
    fn interactive_renders_grow_client_context() {
        let mut p = default_placeholders();
        p["is_non_interactive"] = serde_json::json!(false);
        let prompt = render_base(&default_renderer(), &p);
        assert!(
            prompt.contains("<grow_client>"),
            "interactive prompt must keep the Grow client context"
        );
        assert!(!prompt.contains("autonomous agent"));
    }

    #[test]
    fn non_interactive_suppresses_grow_client_context() {
        let mut p = default_placeholders();
        p["is_non_interactive"] = serde_json::json!(true);
        let prompt = render_base(&default_renderer(), &p);
        assert!(
            !prompt.contains("`! <command>`"),
            "non-interactive prompt must suppress the shell-prefix tip"
        );
        assert!(
            !prompt.contains("<grow_client>"),
            "non-interactive prompt must suppress the Grow client block"
        );
        assert!(!prompt.contains("autonomous agent"));
        assert!(prompt.contains("<instruction_priority>"));
    }

    #[test]
    fn test_combination_sweep_no_unresolved_variables() {
        let optional = [
            ToolKind::Read,
            ToolKind::Edit,
            ToolKind::Execute,
            ToolKind::Search,
            ToolKind::List,
            ToolKind::Plan,
            ToolKind::Skill,
            ToolKind::Task,
            ToolKind::AskUser,
            ToolKind::PlanControl,
            ToolKind::BackgroundTaskAction,
            ToolKind::Monitor,
            ToolKind::MemorySearch,
            ToolKind::MemoryGet,
        ];
        let mut subsets: Vec<Vec<ToolKind>> = vec![vec![], optional.to_vec()];
        for i in 0..optional.len() {
            subsets.push(vec![optional[i]]);
            for j in (i + 1)..optional.len() {
                subsets.push(vec![optional[i], optional[j]]);
            }
        }

        for memory_enabled in [false, true] {
            for subset in &subsets {
                let tools: HashMap<ToolKind, String> = subset
                    .iter()
                    .map(|k| (*k, format!("{k:?}").to_lowercase()))
                    .collect();
                let r = TemplateRenderer::new(tools, HashMap::new());
                let mut p = default_placeholders();
                p["memory_enabled"] = serde_json::json!(memory_enabled);
                let rendered = r
                    .render_with_extra(DEFAULT_SYSTEM_PROMPT, &p)
                    .unwrap_or_else(|e| {
                        panic!("render failed: {subset:?} mem={memory_enabled}: {e:?}")
                    });
                assert!(
                    !rendered.contains("${{") && !rendered.contains("${%"),
                    "unresolved token in render: {subset:?} mem={memory_enabled}",
                );
            }
        }
    }
}
