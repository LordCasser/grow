//! `/behavior` and the direct Normal/Clarify Behavior shortcuts.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};
use tools::types::BehaviorId;

pub(crate) fn available_modes(ctx: &AppCtx<'_>) -> Vec<(BehaviorId, &'static str, &'static str)> {
    let mut modes = vec![
        (
            BehaviorId::Normal,
            "Normal",
            "Direct execution without a specialized collaboration protocol",
        ),
        (
            BehaviorId::Clarify,
            "Clarify",
            "Keep asking until requirements and constraints are sufficient",
        ),
        (
            BehaviorId::Plan,
            "Plan",
            "Draft a complete plan, wait for approval, then execute it",
        ),
    ];
    if ctx.workflows_available {
        modes.push((
            BehaviorId::Workflow,
            "Workflow",
            "Author and run one deterministic scripted workflow per phase, without approval",
        ));
    }
    if ctx.deep_research_available {
        modes.push((
            BehaviorId::DeepResearch,
            "Deep Research",
            "Run read-only evidence research and always deliver a terminal report",
        ));
    }
    if ctx.goal_available {
        modes.push((
            BehaviorId::Goal,
            "Goal",
            "Keep pursuing one long-term objective across idle continuations",
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
        // Hyphen→underscore normalization lets both the documented
        // "deep-research" spelling and the canonical wire id
        // "deep_research" resolve to the same mode; parsing goes through
        // `BehaviorId::try_from_id` so unknown ids (including the legacy
        // "default") are strictly rejected instead of silently switching.
        let normalized = id.to_ascii_lowercase().replace('-', "_");
        let mode = match normalized.as_str() {
            // `clarify` is the documented command word for Ask (wire id:
            // `ask`) and stays accepted so the usage string remains accurate.
            "clarify" => BehaviorId::Clarify,
            _ => match BehaviorId::try_from_id(&normalized) {
                Some(mode) => mode,
                None => {
                    return CommandResult::Error(format!("Unknown or unavailable Behavior: {id}"));
                }
            },
        };
        let unavailable = match mode {
            BehaviorId::Workflow => !ctx.pager_state.workflows_available,
            BehaviorId::DeepResearch => !ctx.pager_state.deep_research_available,
            BehaviorId::Goal => !ctx.pager_state.goal_available,
            _ => false,
        };
        if unavailable {
            return CommandResult::Error(format!("Unknown or unavailable Behavior: {id}"));
        }
        CommandResult::Action(Action::SetBehaviorMode(mode))
    }
}

pub struct BehaviorShortcutCommand {
    name: &'static str,
    mode: BehaviorId,
    description: &'static str,
}

impl BehaviorShortcutCommand {
    pub const fn normal() -> Self {
        Self {
            name: "normal",
            mode: BehaviorId::Normal,
            description: "[behavior] Switch to Normal",
        }
    }
    pub const fn clarify() -> Self {
        Self {
            name: "clarify",
            mode: BehaviorId::Clarify,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;

    fn ctx_with_all_behaviors(models: &ModelState) -> AppCtx<'_> {
        AppCtx {
            models,
            agents: &[],
            current_agent: None,
            behavior_mode: tools::types::BehaviorId::Normal,
            deep_research_available: true,
            goal_available: true,
            current_goal_objective: None,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Inline,
        }
    }

    #[test]
    fn behavior_items_labels_workflow_as_workflow() {
        let state = ModelState::default();
        let ctx = ctx_with_all_behaviors(&state);
        let items = behavior_items(&ctx, None, false);

        let by_wire_id = |id: &str| {
            items
                .iter()
                .find(|i| i.insert_text == id)
                .unwrap_or_else(|| panic!("missing behavior item with wire id {id}"))
        };

        // Workflow display and wire id intentionally use the same product name.
        let workflow = by_wire_id("workflow");
        assert_eq!(workflow.display, "Workflow");
        assert!(workflow.insert_text.ends_with("workflow"));

        // Other behavior labels are untouched.
        assert_eq!(by_wire_id("normal").display, "Normal (current)");
        assert_eq!(by_wire_id("ask").display, "Clarify");
        assert_eq!(by_wire_id("plan").display, "Plan");
        assert_eq!(by_wire_id("deep_research").display, "Deep Research");
        assert_eq!(by_wire_id("goal").display, "Goal");
        assert_eq!(items.len(), 6);
    }

    #[test]
    fn workflow_item_absent_when_workflows_unavailable() {
        let state = ModelState::default();
        let mut ctx = ctx_with_all_behaviors(&state);
        ctx.workflows_available = false;
        let items = behavior_items(&ctx, None, false);
        assert!(
            !items.iter().any(|i| i.insert_text == "workflow"),
            "workflow item must be hidden when workflows_available is false"
        );
    }

    fn exec_ctx<'a>(
        models: &'a ModelState,
        bundle: &'a crate::app::bundle::BundleState,
        workflows_available: bool,
        deep_research_available: bool,
        goal_available: bool,
    ) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: crate::settings::PagerLocalSnapshot {
                workflows_available,
                deep_research_available,
                goal_available,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        }
    }

    fn run_in_ctx(
        models: &ModelState,
        bundle: &crate::app::bundle::BundleState,
        args: &str,
    ) -> CommandResult {
        let mut ctx = exec_ctx(models, bundle, true, true, true);
        BehaviorCommand.run(&mut ctx, args)
    }

    #[test]
    fn behavior_run_parses_canonical_wire_ids() {
        let state = ModelState::default();
        let bundle = crate::app::bundle::BundleState::default();
        assert!(matches!(
            run_in_ctx(&state, &bundle, "normal"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::Normal))
        ));
        assert!(matches!(
            run_in_ctx(&state, &bundle, "ask"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::Clarify))
        ));
        assert!(matches!(
            run_in_ctx(&state, &bundle, "clarify"), // documented command word
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::Clarify))
        ));
        assert!(matches!(
            run_in_ctx(&state, &bundle, "PLAN"), // case-insensitive, as before
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::Plan))
        ));
        assert!(matches!(
            run_in_ctx(&state, &bundle, "workflow"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::Workflow))
        ));
        assert!(matches!(
            run_in_ctx(&state, &bundle, "goal"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::Goal))
        ));
    }

    #[test]
    fn behavior_run_accepts_hyphen_and_underscore_deep_research() {
        let state = ModelState::default();
        let bundle = crate::app::bundle::BundleState::default();
        assert!(matches!(
            run_in_ctx(&state, &bundle, "deep-research"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::DeepResearch))
        ));
        assert!(matches!(
            run_in_ctx(&state, &bundle, "deep_research"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::DeepResearch))
        ));
    }

    #[test]
    fn behavior_run_rejects_legacy_default_id() {
        let state = ModelState::default();
        let bundle = crate::app::bundle::BundleState::default();
        assert!(matches!(
            run_in_ctx(&state, &bundle, "default"),
            CommandResult::Error(msg) if msg.contains("Unknown or unavailable Behavior: default")
        ));
    }

    #[test]
    fn behavior_run_rejects_unavailable_modes() {
        let state = ModelState::default();
        let bundle = crate::app::bundle::BundleState::default();
        let mut ctx = exec_ctx(&state, &bundle, false, false, false);
        for arg in ["workflow", "deep-research", "goal"] {
            assert!(
                matches!(
                    BehaviorCommand.run(&mut ctx, arg),
                    CommandResult::Error(msg) if msg.contains("Unknown or unavailable Behavior")
                ),
                "unavailable mode {arg} must be rejected"
            );
        }
        // Available-modes still resolve when other modes are gated off.
        let mut ctx = exec_ctx(&state, &bundle, false, true, false);
        assert!(matches!(
            BehaviorCommand.run(&mut ctx, "deep_research"),
            CommandResult::Action(Action::SetBehaviorMode(BehaviorId::DeepResearch))
        ));
    }
}
