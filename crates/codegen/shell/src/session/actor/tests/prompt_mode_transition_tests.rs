use super::support::build_actor;
use super::*;

#[tokio::test]
async fn behavior_gateway_rejects_agent_role_ids_instead_of_switching_to_normal() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Clarify);

            let outcome = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                actor.request_behavior_change(acp::SessionModeId::new("browser_use")),
            )
            .await
            .expect("Behavior admission must not recursively acquire the Workflow gate");

            assert!(matches!(
                outcome,
                Ok(crate::session::behavior::BehaviorChangeOutcome::Rejected { .. })
            ));
            assert_eq!(
                actor.behavior.lock().behavior(),
                tool_types::BehaviorId::Clarify
            );
        })
        .await;
}

#[tokio::test]
async fn host_command_turn_can_apply_its_own_behavior_transition() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            let mut command = super::support::running_task_stub("host-goal-enter");
            command.origin = crate::session::PromptOrigin::HostCommand;
            actor.state.lock().await.foreground = ForegroundState::RegularTurn(command);

            let outcome = actor
                .request_behavior_change(acp::SessionModeId::new("ask"))
                .await;

            assert!(
                matches!(
                    outcome,
                    Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                ),
                "unexpected HostCommand transition outcome: {outcome:?}"
            );
            assert_eq!(
                actor.behavior.lock().behavior(),
                tool_types::BehaviorId::Clarify
            );
        })
        .await;
}
fn fn_def(name: &str) -> ToolDefinition {
    ToolDefinition::function(name, None::<&str>, serde_json::json!({"type": "object"}))
}
fn names(defs: &[ToolDefinition]) -> Vec<&str> {
    defs.iter().map(|d| d.function.name.as_str()).collect()
}
#[test]
fn cursor_filter_in_plan_mode_keeps_writes_and_shows_create_plan() {
    let defs = vec![
        fn_def("Read"),
        fn_def("Grep"),
        fn_def("Write"),
        fn_def("StrReplace"),
        fn_def("CreatePlan"),
        fn_def("SwitchMode"),
        fn_def("AskQuestion"),
    ];
    let filtered = filter_cursor_tools_by_plan_mode(defs, true);
    let kept = names(&filtered);
    assert!(kept.contains(&"Read"));
    assert!(kept.contains(&"Grep"));
    assert!(kept.contains(&"CreatePlan"));
    assert!(kept.contains(&"SwitchMode"));
    assert!(kept.contains(&"AskQuestion"));
    assert!(kept.contains(&"Write"));
    assert!(kept.contains(&"StrReplace"));
}
#[test]
fn cursor_filter_is_noop_for_non_cursor_tools() {
    let defs = vec![
        fn_def("read_file"),
        fn_def("search_replace"),
        fn_def("write"),
        fn_def("ask_user_question"),
        fn_def("plan_control"),
    ];
    let in_plan = filter_cursor_tools_by_plan_mode(defs.clone(), true);
    let out_of_plan = filter_cursor_tools_by_plan_mode(defs.clone(), false);
    assert_eq!(names(&in_plan).len(), defs.len());
    assert_eq!(names(&out_of_plan).len(), defs.len());
}
#[test]
fn plan_hides_workflow_launcher_but_default_keeps_it() {
    let defs = vec![fn_def("read_file"), fn_def("workflow")];
    let in_plan = filter_cursor_tools_by_plan_mode(defs.clone(), true);
    let out_of_plan = filter_cursor_tools_by_plan_mode(defs, false);
    assert_eq!(names(&in_plan), vec!["read_file"]);
    assert_eq!(names(&out_of_plan), vec!["read_file", "workflow"]);
}
/// Pins the direct mapping from prompt metadata to mutually exclusive Behavior.
#[test]
fn prompt_mode_selects_exactly_one_behavior() {
    use crate::session::behavior::{BehaviorCoordinator, BehaviorState, PlanPhase};
    use std::path::PathBuf;
    fn reconcile(tracker: &mut BehaviorCoordinator, mode: tool_types::BehaviorId) {
        tracker.select_behavior(mode);
    }
    let mut tracker = BehaviorCoordinator::new();
    assert_eq!(tracker.state(), BehaviorState::Normal);
    reconcile(&mut tracker, tool_types::BehaviorId::Plan);
    assert_eq!(tracker.state(), BehaviorState::Plan(PlanPhase::Drafting));
    reconcile(&mut tracker, tool_types::BehaviorId::Plan);
    assert_eq!(tracker.state(), BehaviorState::Plan(PlanPhase::Drafting));
    reconcile(&mut tracker, tool_types::BehaviorId::Normal);
    assert_eq!(tracker.state(), BehaviorState::Normal);
    reconcile(&mut tracker, tool_types::BehaviorId::Plan);
    assert_eq!(tracker.state(), BehaviorState::Plan(PlanPhase::Drafting));
    reconcile(&mut tracker, tool_types::BehaviorId::Clarify);
    assert_eq!(tracker.state(), BehaviorState::Clarify);
}
