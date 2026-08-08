use super::*;

#[test]
fn durable_terminal_finalizes_the_exact_viewer_turn_once() {
    let mut app = make_app_with_agent("sess-view");
    app.agents.get_mut(&AgentId(0)).unwrap().attached_as_viewer = true;
    let _ = handle(
        make_agent_chunk_message_with_prompt("sess-view", "chunk", "turn-1", false),
        &mut app,
    );

    assert!(handle_ext_notification(
        &grow_turn_completed_notif("sess-view", "turn-1", "end_turn", false),
        &mut app,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(agent.session.state.is_idle());
    assert!(agent.session.current_prompt_id.is_none());
    assert!(matches!(
        last_session_event(&agent.scrollback),
        Some(SessionEvent::TurnCompleted { .. })
    ));

    let len = agent.scrollback.len();
    assert!(!handle_ext_notification(
        &grow_turn_completed_notif("sess-view", "turn-1", "end_turn", false),
        &mut app,
    ));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), len);
}

#[test]
fn durable_terminal_does_not_finish_a_different_foreground_turn() {
    let mut app = make_app_with_agent("sess-view");
    let _ = handle(
        make_agent_chunk_message_with_prompt("sess-view", "chunk", "turn-2", false),
        &mut app,
    );

    assert!(!handle_ext_notification(
        &grow_turn_completed_notif("sess-view", "turn-1", "end_turn", false),
        &mut app,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(agent.session.state.is_turn_running());
    assert_eq!(agent.session.current_prompt_id.as_deref(), Some("turn-2"));
}

#[test]
fn structured_internal_foreground_is_adopted_without_id_prefix_inference() {
    let mut app = make_app_with_agent("sess-view");
    let payload = serde_json::json!({
        "sessionId": "sess-view",
        "entries": [],
        "runningPromptId": "019f-turn",
        "runningOrigin": "goal_continuation",
        "runningTurnKind": "internal",
        "runningText": "continue the goal",
        "runningKind": "prompt"
    });
    let notif = acp::ExtNotification::new(
        "grow/queue/changed",
        std::sync::Arc::from(serde_json::value::to_raw_value(&payload).unwrap()),
    );

    assert!(handle_ext_notification(&notif, &mut app));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(agent.session.state.is_turn_running());
    assert_eq!(agent.session.current_prompt_id.as_deref(), Some("019f-turn"));
}

#[test]
fn running_snapshot_without_turn_kind_is_not_adopted() {
    let mut app = make_app_with_agent("sess-view");
    let payload = serde_json::json!({
        "sessionId": "sess-view",
        "entries": [],
        "runningPromptId": "legacy-hidden-turn"
    });
    let notif = acp::ExtNotification::new(
        "grow/queue/changed",
        std::sync::Arc::from(serde_json::value::to_raw_value(&payload).unwrap()),
    );

    assert!(handle_ext_notification(&notif, &mut app));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(agent.session.state.is_idle());
    assert!(agent.session.current_prompt_id.is_none());
}
