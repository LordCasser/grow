//! `/behavior` and the direct Normal/Clarify Behavior shortcuts.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};
use grow_tools::types::SessionMode;

pub(crate) fn available_modes(ctx: &AppCtx<'_>) -> Vec<(SessionMode, &'static str, &'static str)> {
    let mut modes = vec![
        (
            SessionMode::Default,
            "Normal",
            "Direct execution without a specialized collaboration protocol",
        ),
        (
            SessionMode::Ask,
            "Clarify",
            "Keep asking until requirements and constraints are sufficient",
        ),
        (
            SessionMode::Plan,
            "Plan",
            "Draft a complete plan, wait for approval, then execute it",
        ),
    ];
    if ctx.workflows_available {
        modes.push((
            SessionMode::Workflow,
            "Workflow",
            "Dynamically create and run bounded sub-plans without approval",
        ));
    }
    if ctx.deep_research_available {
        modes.push((
            SessionMode::DeepResearch,
            "Deep Research",
            "Run read-only evidence research and always deliver a terminal report",
        ));
    }
    if ctx.goal_available {
        modes.push((
            SessionMode::Goal,
            "Goal",
            "Persist until an independent verifier confirms the objective is achieved",
        ));
    }
    modes
}

pub(crate) fn behavior_items(
    ctx: &AppCtx<'_>,
    prefix: Option<&str>,
    include_keep_current: bool,
) -> Vec<ArgItem> {
    let mut items = Vec::new();
    if include_keep_current {
        let insert_text = prefix.unwrap_or_default().trim().to_string();
        items.push(ArgItem {
            display: "Keep current Behavior".to_string(),
            match_text: "keep current behavior unchanged".to_string(),
            insert_text,
            description: "Switch Agent without changing Behavior".to_string(),
        });
    }
    items.extend(
        available_modes(ctx)
            .into_iter()
            .map(|(mode, label, description)| {
                let mode_id = mode.as_id();
                ArgItem {
                    display: if mode == ctx.behavior_mode {
                        format!("{label} (current)")
                    } else {
                        label.to_string()
                    },
                    match_text: format!("{label} {mode_id} {description}"),
                    insert_text: prefix
                        .map(|prefix| format!("{} {mode_id}", prefix.trim()))
                        .unwrap_or_else(|| mode_id.to_string()),
                    description: description.to_string(),
                }
            }),
    );
    items
}

pub struct BehaviorCommand;

impl SlashCommand for BehaviorCommand {
    fn name(&self) -> &str {
        "behavior"
    }
    fn description(&self) -> &str {
        "Choose how the primary Agent advances the task"
    }
    fn usage(&self) -> &str {
        "/behavior [normal|clarify|plan|workflow|deep-research|goal]"
    }
    fn takes_args(&self) -> bool {
        true
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn offered_when_session_less(&self) -> bool {
        true
    }
    fn arg_placeholder(&self) -> Option<&str> {
        Some("[behavior]")
    }

    fn suggest_args(&self, ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        Some(behavior_items(ctx, None, false))
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let id = args.trim();
        if id.is_empty() {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "behavior".to_string(),
                args_query: String::new(),
            });
        }
        let mode = match id.to_ascii_lowercase().as_str() {
            "normal" => SessionMode::Default,
            "clarify" => SessionMode::Ask,
            "plan" => SessionMode::Plan,
            "workflow" if ctx.pager_state.workflows_available => SessionMode::Workflow,
            "deep-research" if ctx.pager_state.deep_research_available => SessionMode::DeepResearch,
            "goal" if ctx.pager_state.goal_available => SessionMode::Goal,
            _ => return CommandResult::Error(format!("Unknown or unavailable Behavior: {id}")),
        };
        CommandResult::Action(Action::SetBehaviorMode(mode))
    }
}

pub struct BehaviorShortcutCommand {
    name: &'static str,
    mode: SessionMode,
    description: &'static str,
}

impl BehaviorShortcutCommand {
    pub const fn normal() -> Self {
        Self {
            name: "normal",
            mode: SessionMode::Default,
            description: "[behavior] Switch to Normal",
        }
    }
    pub const fn clarify() -> Self {
        Self {
            name: "clarify",
            mode: SessionMode::Ask,
            description: "[behavior] Switch to Clarify",
        }
    }
}

impl SlashCommand for BehaviorShortcutCommand {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn usage(&self) -> &str {
        self.name
    }
    fn session_scoped(&self) -> bool {
        true
    }
    fn offered_when_session_less(&self) -> bool {
        true
    }
    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::SetBehaviorMode(self.mode))
    }
}
