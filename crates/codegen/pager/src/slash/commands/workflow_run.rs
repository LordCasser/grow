use crate::app::actions::Action;
use crate::slash::command::{AppCtx, CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::{ModeSupport, Remedy};

/// Bare `/workflow-run` is a selector, never an implicit launch. Explicit
/// management arguments remain Shell-owned and are re-gated there.
pub struct WorkflowRunCommand;

impl SlashCommand for WorkflowRunCommand {
    fn name(&self) -> &str {
        "workflow-run"
    }

    fn description(&self) -> &str {
        "Choose a Definition to run or a Run to manage"
    }

    fn usage(&self) -> &str {
        "/workflow-run [<name> [args] | pause|resume|stop [<run>]]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn visible(&self, ctx: &AppCtx) -> bool {
        ctx.workflows_available && ctx.behavior_mode == tools::types::BehaviorId::Workflow
    }

    fn mode_support(&self) -> ModeSupport {
        ModeSupport::FullscreenOnly(Remedy::SwitchMode {
            why: "the Workflow selector needs fullscreen",
        })
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty()
            || matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "pause" | "resume" | "stop"
            )
        {
            CommandResult::Action(Action::ToggleWorkflows)
        } else {
            CommandResult::HostCommand(format!("/workflow-run {trimmed}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_or_targetless_management_op_opens_selector() {
        let mut ctx = CommandExecCtx {
            models: &crate::acp::model_state::ModelState::default(),
            session_id: None,
            bundle_state: &crate::app::bundle::BundleState::default(),
            screen_mode: crate::app::ScreenMode::Fullscreen,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        };
        for args in ["", "pause", "RESUME", " stop "] {
            assert!(matches!(
                WorkflowRunCommand.run(&mut ctx, args),
                CommandResult::Action(Action::ToggleWorkflows)
            ));
        }
        assert!(matches!(
            WorkflowRunCommand.run(&mut ctx, "pause review-2"),
            CommandResult::HostCommand(command) if command == "/workflow-run pause review-2"
        ));
        assert!(matches!(
            WorkflowRunCommand.run(&mut ctx, "deep-research compiler design"),
            CommandResult::HostCommand(command)
                if command == "/workflow-run deep-research compiler design"
        ));
        assert_eq!(
            WorkflowRunCommand.usage(),
            "/workflow-run [<name> [args] | pause|resume|stop [<run>]]"
        );
    }
}
