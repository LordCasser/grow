//! `/agent [agent]` — switch the current session's Agent (session-scoped).
//!
//! Agent IDs come from discovery: builtins and top-level user agents use a bare
//! name; nested user/project definitions use a path-style id
//! (e.g. `software-engineering/software-architect`). Switching Agent never
//! changes Behavior — use `/behavior` or Ctrl+X b for that.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};

pub struct AgentCommand;

/// Map discovery scope to the short label shown at the end of agent rows
/// (skills-menu style: `(system)` / `(user)` / …).
pub(crate) fn agent_scope_label(scope: &str) -> &'static str {
    match scope {
        "built-in" | "builtin" | "system" => "system",
        "user" => "user",
        "project" => "project",
        "bundled" => "bundled",
        _ => "user",
    }
}

impl SlashCommand for AgentCommand {
    fn name(&self) -> &str {
        "agent"
    }
    fn description(&self) -> &str {
        "Switch the current session's Agent"
    }
    fn usage(&self) -> &str {
        "/agent [agent]"
    }
    fn takes_args(&self) -> bool {
        true
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn arg_placeholder(&self) -> Option<&str> {
        Some("[agent]")
    }

    fn suggest_args(&self, ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        Some(
            ctx.agents
                .iter()
                .map(|agent| {
                    let scope = agent_scope_label(&agent.scope);
                    let display = if ctx.current_agent == Some(agent.name.as_str()) {
                        format!("{} (current) ({scope})", agent.name)
                    } else {
                        format!("{} ({scope})", agent.name)
                    };
                    ArgItem {
                        display,
                        match_text: format!("{} {} {scope}", agent.name, agent.description),
                        // Bare discovery id (path-style when nested); no trailing
                        // space — agent is single-phase, no Behavior follow-up.
                        insert_text: agent.name.clone(),
                        description: agent.description.clone(),
                    }
                })
                .collect(),
        )
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let args = args.trim();
        if args.is_empty() {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "agent".to_string(),
                args_query: String::new(),
            });
        }
        // Agent ids may contain `/` (nested discovery names). Only whitespace
        // separates tokens; reject any second token.
        let mut parts = args.split_whitespace();
        let Some(agent_name) = parts.next() else {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "agent".to_string(),
                args_query: String::new(),
            });
        };
        if let Some(extra) = parts.next() {
            return CommandResult::Error(format!("Unexpected argument: {extra}"));
        }
        CommandResult::Action(Action::SwitchAgent {
            agent_name: agent_name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::slash::command::AgentArg;
    use std::sync::LazyLock;

    static BUNDLE: LazyLock<BundleState> = LazyLock::new(BundleState::default);

    fn make_exec_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &BUNDLE,
            screen_mode: crate::app::ScreenMode::Fullscreen,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        }
    }

    fn sample_agents() -> Vec<AgentArg> {
        vec![
            AgentArg {
                name: "grow".into(),
                description: "Default".into(),
                scope: "built-in".into(),
            },
            AgentArg {
                name: "software-engineering/software-architect".into(),
                description: "Architect".into(),
                scope: "user".into(),
            },
        ]
    }

    #[test]
    fn run_empty_opens_picker() {
        let models = ModelState::default();
        let mut ctx = make_exec_ctx(&models);
        match AgentCommand.run(&mut ctx, "") {
            CommandResult::Action(Action::OpenCommandPicker { command, .. }) => {
                assert_eq!(command, "agent");
            }
            other => panic!("expected OpenCommandPicker, got {other:?}"),
        }
    }

    #[test]
    fn run_path_id_switches_without_behavior() {
        let models = ModelState::default();
        let mut ctx = make_exec_ctx(&models);
        match AgentCommand.run(&mut ctx, "software-engineering/software-architect") {
            CommandResult::Action(Action::SwitchAgent { agent_name }) => {
                assert_eq!(agent_name, "software-engineering/software-architect");
            }
            other => panic!("expected SwitchAgent, got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_extra_args() {
        let models = ModelState::default();
        let mut ctx = make_exec_ctx(&models);
        match AgentCommand.run(&mut ctx, "grow plan") {
            CommandResult::Error(msg) => assert!(msg.contains("Unexpected")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn suggest_args_marks_scope_and_uses_path_insert() {
        let agents = sample_agents();
        let models = ModelState::default();
        let ctx = AppCtx {
            models: &models,
            agents: &agents,
            current_agent: Some("grow"),
            behavior_mode: tools::types::SessionMode::Default,
            deep_research_available: false,
            goal_available: false,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        let items = AgentCommand.suggest_args(&ctx, "").expect("items");
        let grow = items.iter().find(|i| i.insert_text == "grow").unwrap();
        assert!(grow.display.contains("(current)"));
        assert!(grow.display.contains("(system)"));
        assert_eq!(grow.insert_text, "grow");

        let nested = items
            .iter()
            .find(|i| i.insert_text == "software-engineering/software-architect")
            .unwrap();
        assert!(nested.display.contains("(user)"));
        assert_eq!(
            nested.insert_text,
            "software-engineering/software-architect"
        );
        assert!(!nested.insert_text.ends_with(' '));
    }
}
