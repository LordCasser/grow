//! `/auto` -- select auto permission mode (LLM classifier).
//!
//! The dispatcher owns state mutation, persistence (with rollback), and toast.
//! Visibility is gated by
//! [`crate::slash::SlashController::set_auto_mode_available`]: `/auto` is
//! hard-hidden when the auto permission-mode feature is off.

use crate::app::actions::{Action, PermissionModeKind};
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Select auto permission mode (LLM classifier).
pub struct AutoCommand;

impl SlashCommand for AutoCommand {
    fn name(&self) -> &str {
        "auto"
    }

    fn description(&self) -> &str {
        "[permission] Switch to Auto"
    }

    fn usage(&self) -> &str {
        "/auto"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn offered_when_session_less(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::SetPermissionMode(PermissionModeKind::Auto))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    fn make_ctx<'a>(
        models: &'a ModelState,
        bundle: &'a BundleState,
        permission_mode: shell::util::config::PermissionMode,
    ) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot {
                permission_mode,
                auto_mode_gate: true,
                ..PagerLocalSnapshot::default()
            },
        }
    }

    #[test]
    fn off_turns_auto_on() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle, shell::util::config::PermissionMode::Ask);
        assert!(matches!(
            AutoCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::SetPermissionMode(PermissionModeKind::Auto))
        ));
    }

    #[test]
    fn on_selects_auto_idempotently() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle, shell::util::config::PermissionMode::Auto);
        assert!(matches!(
            AutoCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::SetPermissionMode(PermissionModeKind::Auto))
        ));
    }

    #[test]
    fn always_approve_switches_to_auto() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(
            &models,
            &bundle,
            shell::util::config::PermissionMode::AlwaysApprove,
        );
        assert!(matches!(
            AutoCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::SetPermissionMode(PermissionModeKind::Auto))
        ));
    }

    #[test]
    fn ignores_args() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle, shell::util::config::PermissionMode::Ask);
        assert!(matches!(
            AutoCommand.run(&mut ctx, "extra"),
            CommandResult::Action(Action::SetPermissionMode(PermissionModeKind::Auto))
        ));
    }
}
