use crate::app::actions::Action;
use crate::slash::command::{AppCtx, CommandExecCtx, CommandResult, SlashCommand};

pub struct WorkflowCommand;

impl SlashCommand for WorkflowCommand {
    fn name(&self) -> &str {
        "workflow"
    }
    fn description(&self) -> &str {
        "[behavior] Switch to Static Workflow"
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
    fn arg_placeholder(&self) -> Option<&str> {
        Some("[prompt]")
    }
    fn visible(&self, ctx: &AppCtx) -> bool {
        ctx.workflows_available
    }
    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        CommandResult::Action(Action::SetBehaviorThenPrompt {
            mode: tools::types::BehaviorId::Workflow,
            prompt: (!args.trim().is_empty()).then(|| args.trim().to_owned()),
        })
    }
}
