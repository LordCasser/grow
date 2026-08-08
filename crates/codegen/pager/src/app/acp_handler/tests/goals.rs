#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;

fn send_goal_update(app: &mut AppView, status: &str, phase: &str) -> bool {
    let payload = serde_json::json!({
        "sessionId": "sess-A",
        "update": {
            "sessionUpdate": "goal_updated",
            "goal_id": "g1",
            "objective": "ship it",
            "objective_revision": 2,
            "status": status,
            "phase": phase,
            "plan_revision": 4,
            "plan_markdown": "- [x] implement\n- [ ] verify",
            "verifier_feedback": "missing restart evidence",
            "tokens_used": 123,
            "elapsed_ms": 750,
            "total_worker_rounds": 2,
            "total_verify_rounds": 1,
            "token_baseline": 10,
            "finished_subagent_tokens": 20
        }
    });
    let raw = serde_json::value::to_raw_value(&payload).unwrap();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    handle(
        AcpClientMessage::ExtNotification(acp_transport::AcpArgs {
            request: acp::ExtNotification::new("grow/session_notification", raw.into()),
            response_tx: tx,
        }),
        app,
    )
}

#[test]
fn goal_update_maps_v2_blackboard_and_background_phase() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_goal_update(&mut app, "active", "verifying"));
    let goal = app.agents[&AgentId(0)].goal_state.as_ref().unwrap();
    assert_eq!(goal.status, GoalDisplayStatus::Active);
    assert_eq!(goal.phase, GoalDisplayPhase::Verifying);
    assert_eq!(goal.objective_revision, 2);
    assert_eq!(goal.plan_revision, 4);
    assert_eq!(goal.plan_markdown, "- [x] implement\n- [ ] verify");
    assert_eq!(goal.verifier_feedback.as_deref(), Some("missing restart evidence"));
}

#[test]
fn obsolete_goal_wire_state_is_rejected() {
    let mut app = make_app_with_agent("sess-A");
    assert!(!send_goal_update(&mut app, "user_paused", "idle"));
    assert!(app.agents[&AgentId(0)].goal_state.is_none());
}
