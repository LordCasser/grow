use super::support::build_actor;
use super::*;
#[test]
fn prompt_mode_from_session_mode_id_uses_acp_session_mode() {
    assert_eq!(
        PromptMode::Ask,
        prompt_mode_from_session_mode_id(&acp::SessionModeId::new("ask"))
    );
    assert_eq!(
        PromptMode::Plan,
        prompt_mode_from_session_mode_id(&acp::SessionModeId::new("plan"))
    );
    assert_eq!(
        PromptMode::Workflow,
        prompt_mode_from_session_mode_id(&acp::SessionModeId::new("workflow"))
    );
    assert_eq!(
        PromptMode::Agent,
        prompt_mode_from_session_mode_id(&acp::SessionModeId::new("default"))
    );
    assert_eq!(
        PromptMode::Agent,
        prompt_mode_from_session_mode_id(&acp::SessionModeId::new("browser_use"))
    );
}

#[tokio::test]
async fn behavior_gateway_rejects_agent_role_ids_instead_of_switching_to_normal() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor
                .behavior
                .lock()
                .select_behavior(Some(xai_tool_types::BehaviorId::Clarify));

            let outcome = actor
                .request_behavior_change(acp::SessionModeId::new("browser_use"))
                .await;

            assert!(matches!(
                outcome,
                crate::session::behavior::BehaviorChangeOutcome::Rejected { .. }
            ));
            assert_eq!(
                actor.behavior.lock().behavior(),
                Some(xai_tool_types::BehaviorId::Clarify)
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
    use crate::session::behavior::{BehaviorController, BehaviorState, PlanPhase};
    use std::path::PathBuf;
    fn reconcile(tracker: &mut BehaviorController, mode: PromptMode) {
        tracker.select_behavior(mode.behavior());
    }
    let mut tracker = BehaviorController::new(PathBuf::from("/tmp/test"));
    assert_eq!(tracker.state(), BehaviorState::Normal);
    reconcile(&mut tracker, PromptMode::Plan);
    assert_eq!(tracker.state(), BehaviorState::Plan(PlanPhase::Drafting));
    reconcile(&mut tracker, PromptMode::Plan);
    assert_eq!(tracker.state(), BehaviorState::Plan(PlanPhase::Drafting));
    reconcile(&mut tracker, PromptMode::Agent);
    assert_eq!(tracker.state(), BehaviorState::Normal);
    reconcile(&mut tracker, PromptMode::Plan);
    assert_eq!(tracker.state(), BehaviorState::Plan(PlanPhase::Drafting));
    reconcile(&mut tracker, PromptMode::Ask);
    assert_eq!(tracker.state(), BehaviorState::Clarify);
}
