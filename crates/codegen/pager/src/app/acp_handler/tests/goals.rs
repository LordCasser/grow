use super::*;

fn send_goal_update(app: &mut AppView, status: &str, objective: &str) -> bool {
    let payload = serde_json::json!({
        "sessionId": "sess-A",
        "update": {
            "sessionUpdate": "goal_updated",
            "goal_id": "g1",
            "objective": objective,
            "status": status,
            "token_budget": 1000,
            "tokens_used": 123,
            "elapsed_ms": 750,
            "created_at": "2026-08-24T00:00:00Z",
            "updated_at": "2026-08-24T00:01:00Z",
            "status_message": "next slice"
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
fn goal_update_maps_only_the_long_term_goal_projection() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_goal_update(&mut app, "active", "ship it"));
    let goal = app.agents[&AgentId(0)].session.goal_state.as_ref().unwrap();
    assert_eq!(goal.goal_id, "g1");
    assert_eq!(goal.objective, "ship it");
    assert_eq!(goal.status, GoalDisplayStatus::Active);
    assert_eq!(goal.token_budget, Some(1000));
    assert_eq!(goal.tokens_used, 123);
    assert_eq!(goal.elapsed_ms, 750);
    assert_eq!(goal.status_message.as_deref(), Some("next slice"));
}

#[test]
fn retired_blackboard_wire_state_is_rejected() {
    let mut app = make_app_with_agent("sess-A");
    let payload = serde_json::json!({
        "sessionId": "sess-A",
        "update": {
            "sessionUpdate": "goal_updated",
            "goal_id": "g1",
            "objective": "obsolete",
            "status": "active",
            "elapsed_ms": 0,
            "created_at": "now",
            "updated_at": "now",
            "plan_markdown": "# retired board"
        }
    });
    let raw = serde_json::value::to_raw_value(&payload).unwrap();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    assert!(!handle(
        AcpClientMessage::ExtNotification(acp_transport::AcpArgs {
            request: acp::ExtNotification::new("grow/session_notification", raw.into()),
            response_tx: tx,
        }),
        &mut app,
    ));
    assert!(app.agents[&AgentId(0)].session.goal_state.is_none());
}

#[test]
fn lifecycle_transitions_append_deduplicated_goal_events() {
    let mut app = make_app_with_agent("sess-A");

    assert!(send_goal_update(&mut app, "active", "ship it"));
    assert!(matches!(
        last_session_event(&app.agents[&AgentId(0)].scrollback),
        Some(SessionEvent::GoalCreated)
    ));
    let created_len = app.agents[&AgentId(0)].scrollback.len();

    assert!(send_goal_update(&mut app, "active", "ship it"));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), created_len);

    assert!(send_goal_update(&mut app, "active", "ship it safely"));
    assert!(matches!(
        last_session_event(&app.agents[&AgentId(0)].scrollback),
        Some(SessionEvent::GoalObjectiveUpdated)
    ));

    for (status, expected) in [
        ("paused", SessionEvent::GoalPaused),
        ("active", SessionEvent::GoalRestarted),
        ("blocked", SessionEvent::GoalBlocked),
        ("usage_limited", SessionEvent::GoalUsageLimited),
        ("budget_limited", SessionEvent::GoalBudgetLimited),
    ] {
        assert!(send_goal_update(&mut app, status, "ship it safely"));
        let actual = last_session_event(&app.agents[&AgentId(0)].scrollback)
            .expect("status transition event");
        assert_eq!(actual.message(), expected.message());
    }

    assert!(send_goal_update(&mut app, "complete", "ship it safely"));
    assert!(matches!(
        last_session_event(&app.agents[&AgentId(0)].scrollback),
        Some(SessionEvent::GoalCompleted { .. })
    ));

    assert!(send_goal_update(&mut app, "cleared", ""));
    assert!(matches!(
        last_session_event(&app.agents[&AgentId(0)].scrollback),
        Some(SessionEvent::GoalCleared)
    ));
    assert!(app.agents[&AgentId(0)].session.goal_state.is_none());
}
