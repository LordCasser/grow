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
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .behavior_mode = tools::types::BehaviorId::Goal;

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

    let before_clear = app.agents[&AgentId(0)].scrollback.len();
    assert!(send_goal_update(&mut app, "cleared", ""));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), before_clear);
    assert!(app.agents[&AgentId(0)].session.goal_state.is_none());
}

#[test]
fn goal_controls_outside_goal_behavior_keep_state_without_duplicate_history() {
    use crate::scrollback::block::BlockContent;
    use crate::scrollback::types::{BlockContext, DisplayMode};
    use shell::extensions::notification::{UiNotice, UiNoticeCategory, UiNoticeTone};

    for behavior in [
        tools::types::BehaviorId::Normal,
        tools::types::BehaviorId::Clarify,
        tools::types::BehaviorId::Plan,
        tools::types::BehaviorId::Workflow,
    ] {
        let mut app = make_app_with_agent("sess-A");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .behavior_mode = behavior;
        for status in ["active", "paused", "blocked", "budget_limited", "complete"] {
            assert!(send_goal_update(&mut app, status, "ship it"));
            assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 0);
            assert_eq!(
                app.agents[&AgentId(0)]
                    .session
                    .goal_state
                    .as_ref()
                    .unwrap()
                    .status,
                GoalDisplayStatus::parse(status).unwrap(),
            );
        }
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .set_goal_detail_visible(true);
        assert!(send_goal_update(&mut app, "cleared", ""));
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.session.goal_state.is_none());
        assert!(!agent.show_goal_detail);
        assert_eq!(agent.session.last_cleared_goal_id.as_deref(), Some("g1"));
        assert_eq!(agent.scrollback.len(), 0);

        let notice = GrowSessionUpdate::UiNotice(UiNotice {
            correlation_id: "clear-invocation".into(),
            category: UiNoticeCategory::Command,
            subject: Some("/goal clear".into()),
            description: Some("[behavior] Set, manage, or check an autonomous goal".into()),
            message: "Goal cleared.".into(),
            tone: UiNoticeTone::Success,
            details: None,
        });
        assert!(handle(
            make_ext_session_notification("sess-A", notice),
            &mut app
        ));
        assert!(send_goal_update(&mut app, "cleared", ""));
        assert!(!send_goal_update(&mut app, "active", "stale state"));
        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.session.behavior_mode, behavior);
        assert_eq!(
            agent.scrollback.len(),
            1,
            "only the command result is visible"
        );
        let entry = agent.scrollback.entry(0).unwrap();
        let RenderBlock::Notice(notice) = &entry.block else {
            panic!("command notice")
        };
        assert!(notice.detail_text().contains("/goal clear"));
        assert_eq!(entry.display_mode, DisplayMode::Collapsed);
        let output = notice.output(&BlockContext {
            width: 100,
            mode: entry.display_mode,
            is_running: false,
            raw: false,
            max_lines: None,
            appearance: crate::appearance::AppearanceConfig::default(),
            is_selected: false,
            cwd: None,
        });
        assert_eq!(output.lines.len(), 1);
    }
}
