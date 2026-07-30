//! `/agent [agent] [behavior]` — staged Agent and Behavior selection.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::commands::behavior::behavior_items;
use grow_tools::types::SessionMode;

pub struct AgentCommand;

fn agent_phase<'a>(ctx: &'a AppCtx<'_>, query: &str) -> Option<&'a str> {
    let token = query.split_whitespace().next()?;
    ctx.agents
        .iter()
        .find(|agent| agent.name.eq_ignore_ascii_case(token))
        .filter(|_| query[token.len()..].starts_with(char::is_whitespace))
        .map(|agent| agent.name.as_str())
}

impl SlashCommand for AgentCommand {
    fn name(&self) -> &str {
        "agent"
    }
    fn description(&self) -> &str {
        "Switch Agent and optionally choose its Behavior"
    }
    fn usage(&self) -> &str {
        "/agent [agent] [behavior]"
    }
    fn takes_args(&self) -> bool {
        true
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn arg_placeholder(&self) -> Option<&str> {
        Some("[agent] [behavior]")
    }

    fn suggest_args(&self, ctx: &AppCtx, args_query: &str) -> Option<Vec<ArgItem>> {
        if let Some(agent) = agent_phase(ctx, args_query) {
            return Some(behavior_items(ctx, Some(agent), true));
        }
        Some(
            ctx.agents
                .iter()
                .map(|agent| ArgItem {
                    display: if ctx.current_agent == Some(agent.name.as_str()) {
                        format!("{} (current)", agent.name)
                    } else {
                        agent.name.clone()
                    },
                    match_text: format!("{} {}", agent.name, agent.description),
                    insert_text: format!("{} ", agent.name),
                    description: agent.description.clone(),
                })
                .collect(),
        )
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let mut parts = args.split_whitespace();
        let Some(agent_name) = parts.next() else {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "agent".to_string(),
                args_query: String::new(),
            });
        };
        let behavior = match parts.next() {
            None => None,
            Some("normal") => Some(SessionMode::Default),
            Some("clarify") => Some(SessionMode::Ask),
            Some("plan") => Some(SessionMode::Plan),
            Some("workflow") => Some(SessionMode::Workflow),
            Some("deep-research") => Some(SessionMode::DeepResearch),
            Some("goal") => Some(SessionMode::Goal),
            Some(other) => return CommandResult::Error(format!("Unknown Behavior: {other}")),
        };
        if let Some(extra) = parts.next() {
            return CommandResult::Error(format!("Unexpected argument: {extra}"));
        }
        CommandResult::Action(Action::SwitchAgent {
            agent_name: agent_name.to_string(),
            behavior,
        })
    }
}
