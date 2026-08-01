//! `/permission` and the direct Ask permission shortcut.

use crate::app::actions::{Action, PermissionModeKind};
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};

pub(crate) fn permission_items(ctx: &AppCtx<'_>) -> Vec<ArgItem> {
    let mut modes = vec![("ask", "Ask", "Prompt before tool actions")];
    if ctx.auto_permission_available {
        modes.push(("auto", "Auto", "Use the classifier for risky tool actions"));
    }
    modes.push((
        "always-approve",
        "Always Approve",
        "Run tool actions without permission prompts",
    ));
    modes
        .into_iter()
        .map(|(id, label, description)| ArgItem {
            display: if id == ctx.current_permission {
                format!("{label} (current)")
            } else {
                label.to_string()
            },
            match_text: format!("{id} {label} {description}"),
            insert_text: id.to_string(),
            description: description.to_string(),
        })
        .collect()
}

pub struct PermissionCommand;

impl SlashCommand for PermissionCommand {
    fn name(&self) -> &str {
        "permission"
    }
    fn description(&self) -> &str {
        "Choose the current session permission policy"
    }
    fn usage(&self) -> &str {
        "/permission [ask|auto|always-approve]"
    }
    fn takes_args(&self) -> bool {
        true
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn offered_when_session_less(&self) -> bool {
        true
    }
    fn arg_placeholder(&self) -> Option<&str> {
        Some("[permission]")
    }
    fn suggest_args(&self, ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        Some(permission_items(ctx))
    }
    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let id = args.trim().to_ascii_lowercase();
        if id.is_empty() {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "permission".to_string(),
                args_query: String::new(),
            });
        }
        let kind = match id.as_str() {
            "ask" => PermissionModeKind::Ask,
            "auto" if ctx.pager_state.auto_mode_gate => PermissionModeKind::Auto,
            "always-approve" => PermissionModeKind::AlwaysApprove,
            _ => return CommandResult::Error(format!("Unknown or unavailable Permission: {id}")),
        };
        CommandResult::Action(Action::SetPermissionMode(kind))
    }
}

pub struct AskCommand;
impl SlashCommand for AskCommand {
    fn name(&self) -> &str {
        "ask"
    }
    fn description(&self) -> &str {
        "[permission] Switch to Ask"
    }
    fn usage(&self) -> &str {
        "/ask"
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn offered_when_session_less(&self) -> bool {
        true
    }
    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::SetPermissionMode(PermissionModeKind::Ask))
    }
}
