//! `/plan` -- enter plan mode.
//!
//! `/plan` enters plan mode. `/plan <description>` enters plan mode and starts
//! a turn with the description after the mode switch completes.
//!
//! Use `/view-plan` to open the current saved plan preview.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Enter plan mode.
pub struct PlanCommand;

impl SlashCommand for PlanCommand {
    fn name(&self) -> &str {
        "plan"
    }

    fn description(&self) -> &str {
        "[behavior] Switch to Plan"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn offered_when_session_less(&self) -> bool {
        // The dashboard stages Plan for the next spawned Agent.
        true
    }

    fn usage(&self) -> &str {
        "/plan [description]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[description]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Action(Action::SetBehaviorMode(tools::types::BehaviorId::Plan));
        }
        CommandResult::Action(Action::SetBehaviorThenPrompt {
            mode: tools::types::BehaviorId::Plan,
            prompt: Some(trimmed.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    fn make_ctx_inactive_plan_mode<'a>(
        models: &'a ModelState,
        bundle: &'a BundleState,
    ) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot {
                behavior_mode: tools::types::BehaviorId::Normal,
                ..PagerLocalSnapshot::default()
            },
        }
    }

    fn make_ctx_active_plan_mode<'a>(
        models: &'a ModelState,
        bundle: &'a BundleState,
    ) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot {
                behavior_mode: tools::types::BehaviorId::Plan,
                ..PagerLocalSnapshot::default()
            },
        }
    }

    /// `/plan` (no args) selects the Plan Behavior.
    #[test]
    fn no_args_not_in_plan_dispatches_set_plan_mode_on() {
        let cmd = PlanCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx_inactive_plan_mode(&models, &bundle);
        match cmd.run(&mut ctx, "") {
            CommandResult::Action(Action::SetBehaviorMode(mode)) => {
                assert_eq!(mode, tools::types::BehaviorId::Plan);
            }
            other => panic!("expected Action::SetBehaviorMode(Plan), got {other:?}"),
        }
    }

    /// `/plan` remains an idempotent Plan Behavior selection.
    #[test]
    fn no_args_already_in_plan_dispatches_set_plan_mode_on() {
        let cmd = PlanCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx_active_plan_mode(&models, &bundle);
        match cmd.run(&mut ctx, "") {
            CommandResult::Action(Action::SetBehaviorMode(mode)) => {
                assert_eq!(mode, tools::types::BehaviorId::Plan);
            }
            other => panic!("expected Action::SetBehaviorMode(Plan), got {other:?}"),
        }
    }

    /// Whitespace-only → treated as no args.
    #[test]
    fn whitespace_only_arg_not_in_plan_dispatches_set_plan_mode_on() {
        let cmd = PlanCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx_inactive_plan_mode(&models, &bundle);
        match cmd.run(&mut ctx, "   ") {
            CommandResult::Action(Action::SetBehaviorMode(mode)) => {
                assert_eq!(mode, tools::types::BehaviorId::Plan);
            }
            other => panic!("expected SetBehaviorMode(Plan), got {other:?}"),
        }
    }

    /// `/plan <description>` selects Plan before sending the description.
    #[test]
    fn with_description_orders_behavior_before_prompt() {
        let cmd = PlanCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx_inactive_plan_mode(&models, &bundle);
        match cmd.run(&mut ctx, "Refactor the auth flow") {
            CommandResult::Action(Action::SetBehaviorThenPrompt { mode, prompt }) => {
                assert_eq!(mode, tools::types::BehaviorId::Plan);
                assert_eq!(
                    prompt.as_deref(),
                    Some("Refactor the auth flow"),
                    "`/plan <desc>` must defer the prompt until Plan is applied"
                );
            }
            other => panic!("expected Action::SetBehaviorThenPrompt, got {other:?}"),
        }
    }

    /// `/plan <description>` when already in plan mode still emits
    /// the same ordered Behavior transition action.
    #[test]
    fn with_description_is_idempotent_when_already_in_plan() {
        let cmd = PlanCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx_active_plan_mode(&models, &bundle);
        match cmd.run(&mut ctx, "something") {
            CommandResult::Action(Action::SetBehaviorThenPrompt { mode, prompt }) => {
                assert_eq!(mode, tools::types::BehaviorId::Plan);
                assert_eq!(prompt.as_deref(), Some("something"));
            }
            other => panic!("expected SetBehaviorThenPrompt, got {other:?}"),
        }
    }

    /// Whitespace is trimmed from the description.
    #[test]
    fn with_description_trims_whitespace() {
        let cmd = PlanCommand;
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = make_ctx_inactive_plan_mode(&models, &bundle);
        match cmd.run(&mut ctx, "  hello world  ") {
            CommandResult::Action(Action::SetBehaviorThenPrompt { mode, prompt }) => {
                assert_eq!(mode, tools::types::BehaviorId::Plan);
                assert_eq!(prompt.as_deref(), Some("hello world"));
            }
            other => panic!("expected SetBehaviorThenPrompt, got {other:?}"),
        }
    }
}
