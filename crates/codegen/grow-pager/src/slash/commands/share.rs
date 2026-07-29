//! `/share` -- prepare the current session for an optional share service.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Prepare the current session snapshot for a separately configured service.
pub struct ShareCommand;

impl SlashCommand for ShareCommand {
    fn name(&self) -> &str {
        "share"
    }

    fn description(&self) -> &str {
        "Prepare this session for sharing"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/share"
    }

    fn run(&self, ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        // Check if we have an active session
        if ctx.session_id.is_none() {
            return CommandResult::Error("No active session to share".to_string());
        }

        CommandResult::Action(Action::ShareSession)
    }
}
