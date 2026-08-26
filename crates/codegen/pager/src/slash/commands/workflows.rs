use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::{ModeSupport, Remedy};

pub struct WorkflowsCommand;

impl SlashCommand for WorkflowsCommand {
    fn name(&self) -> &str {
        "workflows"
    }

    fn description(&self) -> &str {
        "Open the Workflow workspace (Definitions and Runs)"
    }

    fn usage(&self) -> &str {
        "/workflows"
    }

    fn visible(&self, ctx: &crate::slash::command::AppCtx) -> bool {
        ctx.workflows_available && ctx.behavior_mode == tools::types::BehaviorId::Workflow
    }

    /// The workspace is drawn from `AgentView::show_workflows` on the full-TUI
    /// path only; minimal never reads it, so the toggle would flip a flag
    /// nothing renders.
    fn mode_support(&self) -> ModeSupport {
        ModeSupport::FullscreenOnly(Remedy::SwitchMode {
            why: "the Workflow workspace needs fullscreen",
        })
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::ToggleWorkflows)
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
        agents: Vec::new(),
        skills: Vec::new(),
    };

    #[test]
    fn visibility_is_defensive_during_catalog_reload() {
        let models = ModelState::default();
        for available in [false, true] {
            let ctx = crate::slash::command::AppCtx {
                models: &models,
                agents: &[],
                current_agent: None,
                behavior_mode: tools::types::BehaviorId::Normal,
                goal_available: false,
                current_goal_objective: None,
                auto_permission_available: false,
                current_permission: "ask",
                cwd: std::path::Path::new("."),
                has_session_announcements: false,
                workflows_available: available,
                screen_mode: crate::app::ScreenMode::Fullscreen,
            };
            assert_eq!(WorkflowsCommand.visible(&ctx), false);
        }
    }

    #[test]
    fn dispatches_toggle_workflows() {
        let models = ModelState::default();
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Minimal,
            pager_state: PagerLocalSnapshot::default(),
        };
        assert!(matches!(
            WorkflowsCommand.run(&mut ctx, ""),
            CommandResult::Action(Action::ToggleWorkflows)
        ));
    }
}
