use super::*;

#[test]
fn steer_targets_the_existing_turn_and_emits_no_new_prompt() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    {
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.state = AgentState::TurnRunning;
        agent.session.current_prompt_id = Some("turn-1".into());
    }

    let effects = dispatch(
        Action::SteerPrompt {
            text: "additional evidence".into(),
            images: vec![],
        },
        &mut app,
    );

    assert!(matches!(
        effects.as_slice(),
        [Effect::SendInterject { expected_turn_id, text, .. }]
            if expected_turn_id == "turn-1" && text == "additional evidence"
    ));
    assert_eq!(
        app.agents[&id].session.current_prompt_id.as_deref(),
        Some("turn-1")
    );
}

#[test]
fn steer_without_foreground_is_rejected_locally() {
    let mut app = test_app_with_agent();
    let effects = dispatch(
        Action::SteerPrompt {
            text: "late".into(),
            images: vec![],
        },
        &mut app,
    );
    assert!(effects.is_empty());
    assert_eq!(
        app.agents[&AgentId(0)]
            .toast
            .as_ref()
            .map(|(text, _)| text.as_str()),
        Some("No active turn to steer")
    );
}
