//! `/scroll-debug` — toggle the scroll-diagnostics HUD
//! ([`crate::views::scroll_debug_hud`]).
//!
//! Hidden diagnostic (the `/gboom` pattern): typeable but never listed in
//! the dropdown, and any argument produces a local usage error.
//! Pairs with `GROW_SCROLL_DEBUG=1`, which enables the HUD from startup;
//! this command flips it live mid-session.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, CommandExecCtx, CommandResult, SlashCommand};

/// Hidden toggle for the scroll-debug HUD.
pub struct ScrollDebugCommand;

impl SlashCommand for ScrollDebugCommand {
    fn name(&self) -> &str {
        "scroll-debug"
    }

    fn description(&self) -> &str {
        // Never shown: the command is hidden from the dropdown.
        "Toggle the scroll-diagnostics HUD"
    }

    fn usage(&self) -> &str {
        "/scroll-debug"
    }

    /// Diagnostic: typeable, never listed.
    fn visible(&self, _ctx: &AppCtx) -> bool {
        false
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if !args.trim().is_empty() {
            return CommandResult::Error("Usage: /scroll-debug".to_string());
        }
        CommandResult::Action(Action::ToggleScrollDebugHud)
    }
}
