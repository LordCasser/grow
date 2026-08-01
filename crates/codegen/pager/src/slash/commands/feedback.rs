//! `/feedback` -- open Grow's GitHub issue creation page.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

pub const FEEDBACK_ISSUES_URL: &str = "https://github.com/LordCasser/grow/issues/new";

/// Open the public issue tracker in the system browser.
pub struct FeedbackCommand;

impl SlashCommand for FeedbackCommand {
    fn name(&self) -> &str {
        "feedback"
    }

    fn description(&self) -> &str {
        "Report an issue or suggest an improvement on GitHub"
    }

    fn usage(&self) -> &str {
        "/feedback"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::OpenUrl(FEEDBACK_ISSUES_URL.into()))
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
    fn opens_grow_issue_creation_page() {
        let models = ModelState::default();
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Minimal,
            pager_state: PagerLocalSnapshot::default(),
        };
        match FeedbackCommand.run(&mut ctx, "") {
            CommandResult::Action(Action::OpenUrl(url)) => {
                assert_eq!(url, FEEDBACK_ISSUES_URL);
            }
            other => panic!("expected OpenUrl, got {other:?}"),
        }
        assert!(!FeedbackCommand.takes_args());
    }
}
