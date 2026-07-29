//! `/shortcuts` -- open the context-appropriate shortcuts modal.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Open the keyboard shortcuts cheatsheet.
pub struct ShortcutsCommand;

impl SlashCommand for ShortcutsCommand {
    fn name(&self) -> &str {
        "shortcuts"
    }

    fn description(&self) -> &str {
        "Show keyboard shortcuts"
    }

    fn usage(&self) -> &str {
        "/shortcuts"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::OpenShortcutsHelp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_has_no_aliases() {
        assert!(ShortcutsCommand.aliases().is_empty());
    }
}
