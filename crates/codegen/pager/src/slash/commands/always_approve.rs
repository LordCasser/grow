//! `/always-approve` -- select Always Approve for the current session.
//!
//! No scrollback turn — visible effects are the prompt-line chip and
//! a toast (destructive-styled when enabling).

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Select always-approve (YOLO / `permission_mode`).
pub struct AlwaysApproveCommand;

impl SlashCommand for AlwaysApproveCommand {
    fn name(&self) -> &str {
        "always-approve"
    }

    fn description(&self) -> &str {
        "[permission] Switch to Always Approve"
    }

    fn usage(&self) -> &str {
        "/always-approve"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn offered_when_session_less(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::SetPermissionMode(
            crate::app::actions::PermissionModeKind::AlwaysApprove,
        ))
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
        yolo_mode: bool,
    ) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot {
                multiline_mode: false,
                yolo_mode,
                ..PagerLocalSnapshot::default()
            },
        }
    }

    #[test]
    fn off_turns_always_approve_on() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle, false);
        assert!(matches!(
            AlwaysApproveCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::SetPermissionMode(
                crate::app::actions::PermissionModeKind::AlwaysApprove
            ))
        ));
    }

    #[test]
    fn on_selects_always_approve_idempotently() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle, true);
        assert!(matches!(
            AlwaysApproveCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::SetPermissionMode(
                crate::app::actions::PermissionModeKind::AlwaysApprove
            ))
        ));
    }

    #[test]
    fn ignores_args() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx(&models, &bundle, false);
        assert!(matches!(
            AlwaysApproveCommand.run(&mut ctx, "extra"),
            CommandResult::Action(Action::SetPermissionMode(
                crate::app::actions::PermissionModeKind::AlwaysApprove
            ))
        ));
    }
}
