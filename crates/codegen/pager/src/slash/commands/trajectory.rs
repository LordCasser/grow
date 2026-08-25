//! `/trajectory` -- open the active session's durable Timeline debugger.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

pub struct TrajectoryCommand;

impl SlashCommand for TrajectoryCommand {
    fn name(&self) -> &str {
        "trajectory"
    }

    fn description(&self) -> &str {
        "Open this session's Trajectory debugger"
    }

    fn usage(&self) -> &str {
        "/trajectory"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if !args.trim().is_empty() {
            return CommandResult::Error("Usage: /trajectory".to_owned());
        }
        CommandResult::Action(Action::OpenTrajectory)
    }
}
