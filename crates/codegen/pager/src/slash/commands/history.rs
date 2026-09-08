//! `/history [query]` searches historical conversations; `--prompts` recalls input.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Search conversation content using the existing session picker.
pub struct HistoryCommand;

impl SlashCommand for HistoryCommand {
    fn name(&self) -> &str {
        "history"
    }

    fn description(&self) -> &str {
        "Search historical conversations (--prompts for prompt recall)"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/history [query] | /history --prompts"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let query = args.trim();
        if query == "--prompts" {
            CommandResult::Action(Action::OpenHistorySearch)
        } else {
            CommandResult::Action(Action::ShowSessionPicker { query: query.to_owned() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    fn make_ctx<'a>(models: &'a ModelState, bundle: &'a BundleState) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot::default(),
        }
    }

    #[test]
    fn explicit_prompt_recall_remains_available() {
        let cmd = HistoryCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle);
        let result = cmd.run(&mut ctx, "--prompts");
        assert!(matches!(
            result,
            CommandResult::Action(Action::OpenHistorySearch)
        ));
    }

    #[test]
    fn conversation_queries_reach_session_picker() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle);
        for (args, expected) in [("", ""), ("  deployment error  ", "deployment error"), ("历史结果", "历史结果")] {
            assert!(matches!(HistoryCommand.run(&mut ctx, args),
                CommandResult::Action(Action::ShowSessionPicker { query }) if query == expected));
        }
    }

    /// `/history` resolves via the real builtin registry (guards against a
    /// name collision silently dropping it).
    #[test]
    fn resolves_via_builtin_registry() {
        let reg = crate::slash::registry::CommandRegistry::new(
            crate::slash::commands::builtin_commands(),
        );
        let resolved = reg
            .get("history")
            .expect("/history must resolve to a command");
        assert_eq!(resolved.name(), "history");
    }
}
