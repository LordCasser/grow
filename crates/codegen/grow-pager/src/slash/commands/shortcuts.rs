//! `/shortcuts` and `/?` -- open the context-appropriate shortcuts modal.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Open the keyboard shortcuts cheatsheet.
pub struct ShortcutsCommand;

impl SlashCommand for ShortcutsCommand {
    fn name(&self) -> &str {
        "shortcuts"
    }

    fn aliases(&self) -> &[&str] {
        &["?"]
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
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    static DEFAULT_BUNDLE_STATE: BundleState = BundleState {
        has_cache: false,
        version: String::new(),
        personas: Vec::new(),
        roles: Vec::new(),
        agents: Vec::new(),
        skills: Vec::new(),
        persona_details: Vec::new(),
        role_details: Vec::new(),
    };

    #[test]
    fn question_mark_alias_opens_shortcuts_help() {
        let invocation = crate::slash::parse_invocation("/?").expect("/? must parse");
        assert_eq!(invocation.token, "?");
        assert!(invocation.args.is_empty());

        let models = ModelState::default();
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            pager_state: PagerLocalSnapshot::default(),
        };
        assert_eq!(ShortcutsCommand.aliases(), &["?"]);
        assert!(matches!(
            ShortcutsCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::OpenShortcutsHelp)
        ));
    }
}
