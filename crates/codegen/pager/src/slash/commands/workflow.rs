use crate::app::actions::Action;
use crate::slash::command::{AppCtx, CommandExecCtx, CommandResult, SlashCommand};

pub struct WorkflowCommand;

const WORKFLOW_GUIDE_PROMPT: &str = "Help me find or create a Workflow. First ask what I want to automate. After I answer, search existing Workflows by name, description, and when_to_use; use one clear match, ask me to choose among near-equal matches, and only if none fits guide me in creating a session draft.";

impl SlashCommand for WorkflowCommand {
    fn name(&self) -> &str {
        "workflow"
    }
    fn description(&self) -> &str {
        "[behavior] Switch to Workflow"
    }
    fn usage(&self) -> &str {
        "/workflow [prompt]"
    }
    fn takes_args(&self) -> bool {
        true
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn offered_when_session_less(&self) -> bool {
        // The dashboard can stage Workflow for the next primary session.
        true
    }
    fn arg_placeholder(&self) -> Option<&str> {
        Some("[prompt]")
    }
    fn visible(&self, ctx: &AppCtx) -> bool {
        ctx.workflows_available
    }
    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        CommandResult::Action(Action::SetBehaviorThenPrompt {
            mode: tools::types::BehaviorId::Workflow,
            prompt: Some(if args.trim().is_empty() {
                WORKFLOW_GUIDE_PROMPT.to_owned()
            } else {
                args.trim().to_owned()
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_workflow_stages_search_first_guide_prompt() {
        let mut ctx = CommandExecCtx {
            models: &crate::acp::model_state::ModelState::default(),
            session_id: None,
            bundle_state: &crate::app::bundle::BundleState::default(),
            screen_mode: crate::app::ScreenMode::Fullscreen,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        };
        let result = WorkflowCommand.run(&mut ctx, "");
        let CommandResult::Action(Action::SetBehaviorThenPrompt { mode, prompt }) = result else {
            panic!("unexpected command result");
        };
        assert_eq!(mode, tools::types::BehaviorId::Workflow);
        let prompt = prompt.expect("guide prompt");
        assert!(prompt.contains("First ask what I want to automate"));
        assert!(prompt.contains("search existing Workflows"));
        assert!(prompt.contains("only if none fits"));
        assert!(prompt.contains("creating a session draft"));
    }
}
