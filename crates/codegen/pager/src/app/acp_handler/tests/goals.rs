#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;

fn send_goal_update(app: &mut AppView, status: &str, phase: &str) -> bool {
    send_goal_update_at_revision(app, status, phase, 2)
}

fn send_goal_update_at_revision(
    app: &mut AppView,
    status: &str,
    phase: &str,
    objective_revision: u64,
) -> bool {
    let payload = serde_json::json!({
        "sessionId": "sess-A",
        "update": {
            "sessionUpdate": "goal_updated",
            "goal_id": "g1",
            "objective": "ship it",
            "objective_revision": objective_revision,
            "status": status,
            "phase": phase,
            "plan_revision": 4,
            "board_revision": 7,
            "tasks": [
                {
                    "id": "T1",
                    "parent_id": null,
                    "depth": 1,
                    "status": "done",
                    "summary": "implement",
                    "completed_descendants": 0,
                    "total_descendants": 0
                },
                {
                    "id": "T2",
                    "parent_id": null,
                    "depth": 1,
                    "status": "pending",
                    "summary": "verify",
                    "completed_descendants": 0,
                    "total_descendants": 0
                }
            ],
            "plan_markdown": "# Goal\n\n> ship it\n\n## Plan\n\n- [x] **T1** `done` — implement\n- [ ] **T2** `pending` — verify\n\n## Goal acceptance\n\n- verified\n\n## Verification evidence\n\n- pending\n\n## Open gaps\n\n- verify",
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
fn goal_update_maps_canonical_blackboard_and_background_phase() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_goal_update(&mut app, "active", "verifying"));
    let goal = app.agents[&AgentId(0)].goal_state.as_ref().unwrap();
    assert_eq!(goal.status, GoalDisplayStatus::Active);
    assert_eq!(goal.phase, GoalDisplayPhase::Verifying);
    assert_eq!(goal.objective_revision, 2);
    assert_eq!(goal.plan_revision, 4);
    assert_eq!(goal.board_revision, 7);
    assert_eq!(goal.tasks.len(), 2);
    assert!(goal.plan_markdown.starts_with("# Goal\n"));
    assert_eq!(goal.verifier_feedback.as_deref(), Some("missing restart evidence"));
}

#[test]
fn obsolete_goal_wire_state_is_rejected() {
    let mut app = make_app_with_agent("sess-A");
    assert!(!send_goal_update(&mut app, "user_paused", "idle"));
    assert!(app.agents[&AgentId(0)].goal_state.is_none());
}

#[test]
fn goal_phase_transitions_append_deduplicated_session_events() {
    let mut app = make_app_with_agent("sess-A");

    assert!(send_goal_update(&mut app, "active", "planning"));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalAccepted)));
    let accepted_len = app.agents[&AgentId(0)].scrollback.len();

    assert!(send_goal_update(&mut app, "active", "planning"));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), accepted_len, "a live progress tick in the same state must not append another event");

    assert!(send_goal_update(&mut app, "active", "executing"));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalExecutionStarted)));

    assert!(send_goal_update(&mut app, "active", "verifying"));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalVerificationStarted)));

    assert!(send_goal_update(&mut app, "active", "executing"));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalExecutionResumed)));

    assert!(send_goal_update(&mut app, "active", "verifying"));
    assert!(send_goal_update(&mut app, "active", "summarizing"));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalFinalizationStarted)));

    assert!(send_goal_update(&mut app, "complete", "summarizing"));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalCompleted { .. })));
}

#[test]
fn goal_status_and_revision_transitions_append_session_events() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_goal_update(&mut app, "active", "planning"));

    assert!(send_goal_update_at_revision(&mut app, "active", "planning", 3));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalPlanningRestarted)));

    assert!(send_goal_update_at_revision(&mut app, "paused", "planning", 3));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalPaused)));

    assert!(send_goal_update_at_revision(&mut app, "active", "planning", 3));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalResumed)));

    assert!(send_goal_update_at_revision(&mut app, "blocked", "planning", 3));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalBlocked)));

    assert!(send_goal_update_at_revision(&mut app, "budget_limited", "planning", 3));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalBudgetLimited)));

    assert!(send_goal_update_at_revision(&mut app, "cleared", "planning", 3));
    assert!(matches!(last_session_event(&app.agents[&AgentId(0)].scrollback), Some(SessionEvent::GoalCleared)));
    assert!(app.agents[&AgentId(0)].goal_state.is_none());
}
