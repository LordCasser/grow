use super::*;

fn checkpoint_image_send(app: &mut AppView) -> Effect {
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.prompt.set_text("describe ");
    agent.prompt.set_cursor(agent.prompt.text().len());
    agent
        .prompt
        .insert_image(crate::prompt_images::from_clipboard_data(
            &crate::clipboard::ImageData {
                data: vec![1, 2, 3, 4],
                mime_type: "image/png".into(),
            },
        ))
        .unwrap();
    let text = agent.prompt.text().to_owned();
    let mut effects = dispatch(Action::SendPrompt(text), app);
    assert_eq!(effects.len(), 1);
    let send = effects.remove(0);
    assert!(matches!(send, Effect::SendPromptBlocks { .. }));
    send
}

fn checkpoint_result(app: &AppView, send: Effect, result: Result<(), String>) -> Action {
    let (agent_id, session_id, prompt_id, _) = send.prompt_rpc_identity().unwrap();
    Action::TaskComplete(TaskResult::InputIntentCheckpointed {
        agent_id,
        session_id: session_id.clone(),
        binding_epoch: app.agents[&agent_id].session_binding_epoch,
        prompt_id: prompt_id.to_owned(),
        send: Box::new(send),
        result,
    })
}

#[tokio::test]
async fn image_checkpoint_failure_releases_submission_and_allows_exact_retry() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::image_checkpoint_failure_releases_submission_and_allows_exact_retry"
    )) {
        return;
    }
    let mut app = test_app_with_agent();
    let send = checkpoint_image_send(&mut app);
    let id = AgentId(0);
    let prompt_id = send.prompt_rpc_identity().unwrap().2.to_owned();
    let original = app.agents[&id].session.in_flight_prompt.clone().unwrap();
    let agent = app.agents.get_mut(&id).unwrap();
    assert!(agent.session.state.is_turn_submitting());
    agent.session.begin_prompt_status_query(&prompt_id);

    // Exercise the production checkpoint rejection for an image snapshot.
    let checkpoint = app.checkpoint_input_intent(id, prompt_id.clone(), None, None);
    let result = match checkpoint {
        Ok(done) => done.await.unwrap().map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    assert!(result.is_err());
    let action = checkpoint_result(&app, send, result);
    assert!(dispatch(action, &mut app).is_empty());
    let agent = &app.agents[&id];
    assert!(agent.session.state.is_idle());
    assert!(agent.session.current_prompt_id.is_none());
    assert!(agent.session.turn_started_at.is_none());
    assert!(!agent.session.prompt_status_query_matches(&prompt_id));
    assert_eq!(agent.prompt.text(), original.text);
    assert_eq!(agent.prompt.images.len(), 1);
    assert_eq!(
        agent.prompt.images[0].encoded_bytes,
        original.images[0].encoded_bytes
    );
    assert!(
        poll_stalled_prompt_submissions(
            &mut app,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .is_none()
    );

    let mut effects = dispatch(Action::SendPrompt(original.text), &mut app);
    assert_eq!(effects.len(), 1);
    let retry = effects.remove(0);
    assert!(
        matches!(&retry, Effect::SendPromptBlocks { prompt_id: retry_id, images, .. }
        if retry_id == &prompt_id && images[0].encoded_bytes == original.images[0].encoded_bytes)
    );
    app.checkpoint_input_intent(id, prompt_id, None, None)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let action = checkpoint_result(&app, retry, Ok(()));
    assert!(matches!(
        dispatch(action, &mut app).as_slice(),
        [Effect::ReadyInputIntent { .. }]
    ));
    assert!(app.agents[&id].session.state.is_turn_submitting());
}

#[test]
fn image_checkpoint_failure_preserves_new_edits_without_auto_retry() {
    let mut app = test_app_with_agent();
    let send = checkpoint_image_send(&mut app);
    let prompt_id = send.prompt_rpc_identity().unwrap().2.to_owned();
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .prompt
        .set_text("new draft");
    let action = checkpoint_result(&app, send, Err("checkpoint failed".into()));
    assert!(dispatch(action, &mut app).is_empty());
    let agent = &app.agents[&AgentId(0)];
    assert!(agent.session.state.is_idle());
    assert_eq!(agent.prompt.text(), "new draft");
    let queued = agent.session.pending_prompts.front().unwrap();
    assert_eq!(queued.prompt_id.as_deref(), Some(prompt_id.as_str()));
    assert_eq!(
        queued.images[0].encoded_bytes.as_deref(),
        Some(&[1, 2, 3, 4][..])
    );
    assert!(queued.requires_review);
    assert!(dispatch(Action::DrainQueue, &mut app).is_empty());
}

#[test]
fn image_checkpoint_failure_does_not_release_another_or_running_foreground() {
    for (same_prompt, state) in [
        (false, AgentState::TurnSubmitting),
        (false, AgentState::TurnRunning),
        (true, AgentState::TurnRunning),
    ] {
        let mut app = test_app_with_agent();
        let send = checkpoint_image_send(&mut app);
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        if !same_prompt {
            agent.session.current_prompt_id = Some("other-prompt".into());
        }
        agent.session.state = state.clone();
        let foreground = agent.session.current_prompt_id.clone();
        let started = agent.session.turn_started_at;
        let action = checkpoint_result(&app, send, Err("checkpoint failed".into()));
        assert!(dispatch(action, &mut app).is_empty());
        let agent = &app.agents[&AgentId(0)];
        assert_eq!(
            std::mem::discriminant(&agent.session.state),
            std::mem::discriminant(&state),
        );
        assert_eq!(agent.session.current_prompt_id, foreground);
        assert_eq!(agent.session.turn_started_at, started);
    }
}

fn failed_image_submission(app: &mut AppView) -> Vec<Effect> {
    let id = AgentId(0);
    let agent = app.agents.get_mut(&id).unwrap();
    let image = crate::prompt_images::from_clipboard_data(&crate::clipboard::ImageData {
        data: vec![1, 2, 3, 4],
        mime_type: "image/png".into(),
    });
    let mut composer = crate::views::prompt_widget::PromptWidget::new();
    composer.set_text("describe ");
    composer.set_cursor(composer.text().len());
    composer.insert_image(image).unwrap();
    let (text, images, chip_elements) = composer.stash().into_submission();
    agent.session.state = AgentState::TurnSubmitting;
    agent.session.current_prompt_id = Some("failed-image".into());
    agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
        prompt_id: Some("failed-image".into()),
        admission_resolved: None,
        frozen_skill_selection: None,
        expected_turn_id: None,
        text,
        images,
        scrollback_entry: crate::scrollback::EntryId::new(1),
        combined_scrollback_entries: vec![],
        chip_elements,
    });
    dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: id,
            result: Err("input artifact write failed".into()),
            http_status: None,
            prompt_id: Some("failed-image".into()),
        }),
        app,
    )
}

#[test]
fn failed_image_admission_restores_structured_draft_for_explicit_retry() {
    let mut app = test_app_with_agent();
    assert!(matches!(
        failed_image_submission(&mut app).as_slice(),
        [Effect::QueryInputAdmissionStatus { prompt_id, .. }] if prompt_id == "failed-image"
    ));
    let agent = &app.agents[&AgentId(0)];
    assert!(agent.prompt.text().starts_with("describe [Image #1]"));
    assert_eq!(agent.prompt.images.len(), 1);
    assert_eq!(
        agent.prompt.images[0].encoded_bytes.as_deref(),
        Some(&[1, 2, 3, 4][..])
    );
    let text = agent.prompt.text().to_owned();
    let effects = dispatch(Action::SendPrompt(text), &mut app);
    assert!(matches!(
        effects.as_slice(),
        [Effect::SendPromptBlocks { images, prompt_id, .. }]
            if images.len() == 1 && prompt_id == "failed-image"
    ));
}

#[test]
fn failed_image_admission_preserves_newer_draft_and_holds_retry() {
    let mut app = test_app_with_agent();
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .prompt
        .set_text("new draft");
    assert!(matches!(
        failed_image_submission(&mut app).as_slice(),
        [Effect::QueryInputAdmissionStatus { .. }]
    ));
    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.prompt.text(), "new draft");
    let queued = agent.session.pending_prompts.front().unwrap();
    assert!(queued.text.starts_with("describe [Image #1]"));
    assert_eq!(queued.images.len(), 1);
    assert!(queued.requires_review);
    assert!(
        dispatch(Action::DrainQueue, &mut app).is_empty(),
        "failed input must not loop automatically"
    );
}

#[test]
fn confirmed_admission_keeps_draft_and_prevents_duplicate_send() {
    let mut app = test_app_with_agent();
    let effects = failed_image_submission(&mut app);
    assert!(matches!(
        effects.as_slice(),
        [Effect::QueryInputAdmissionStatus { .. }]
    ));
    let session_id = app.agents[&AgentId(0)].session.session_id.clone().unwrap();
    dispatch(
        Action::TaskComplete(TaskResult::InputAdmissionStatusResolved {
            agent_id: AgentId(0),
            session_id: session_id.clone(),
            binding_epoch: app.agents[&AgentId(0)].session_binding_epoch,
            prompt_id: "failed-image".into(),
            response_session_id: Some(session_id.0.to_string()),
            response_prompt_id: Some("failed-image".into()),
            response_source: Some("durable".into()),
            status: Ok(crate::app::actions::InputAdmissionStatusWire::Allowed {
                route: chat_state::InputRoute::Fifo,
            }),
        }),
        &mut app,
    );
    let text = app.agents[&AgentId(0)].prompt.text().to_owned();
    assert!(dispatch(Action::SendPrompt(text), &mut app).is_empty());
    assert!(
        app.agents[&AgentId(0)]
            .prompt
            .text()
            .starts_with("describe [Image #1]")
    );
    let edited = format!("{} please", app.agents[&AgentId(0)].prompt.text());
    let effects = dispatch(Action::SendPrompt(edited), &mut app);
    assert!(matches!(
        effects.as_slice(),
        [Effect::SendPromptBlocks { prompt_id, .. }] if prompt_id != "failed-image"
    ));
}

#[test]
fn failed_image_retry_does_not_attach_images_to_new_placeholder_text() {
    let mut app = test_app_with_agent();
    let effects = dispatch(
        Action::SendPrompt("[Image #1: /tmp/previous.png]".into()),
        &mut app,
    );
    assert!(
        matches!(effects.as_slice(), [Effect::SendPrompt { text, .. }] if text == "[Image #1: /tmp/previous.png]")
    );
}

#[test]
fn failed_image_interjection_keeps_attachments_and_requires_review() {
    let mut app = test_app_with_agent();
    let image = crate::prompt_images::from_clipboard_data(&crate::clipboard::ImageData {
        data: vec![1, 2, 3],
        mime_type: "image/png".into(),
    });
    let effects = dispatch(
        Action::TaskComplete(TaskResult::InterjectFailed {
            agent_id: AgentId(0),
            error: "admission failed".into(),
            expected_turn_id: "turn-1".into(),
            interjection_id: "interjection-1".into(),
            text: "image".into(),
            blocks: None,
            images: vec![image],
        }),
        &mut app,
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::QueryInputAdmissionStatus { prompt_id, .. }]
            if prompt_id == "interjection-1"
    ));
    let draft = app.agents[&AgentId(0)]
        .session
        .pending_prompts
        .front()
        .unwrap();
    assert_eq!(draft.images.len(), 1);
    assert!(draft.requires_review);
    // Queue editing restores precisely this snapshot. A missing image chip
    // would make the next stash/drain silently discard its attachment.
    let mut composer = crate::views::prompt_widget::PromptWidget::new();
    composer.restore(crate::views::prompt_widget::StashedPrompt::from_submission(
        draft.text.clone(),
        draft.images.clone(),
        draft.chip_elements.clone(),
    ));
    composer.textarea.insert_str(" retry");
    let (_, images, chips) = composer.stash().into_submission();
    assert_eq!(images.len(), 1);
    assert_eq!(chips.len(), 1);
    assert_eq!(images[0].encoded_bytes.as_deref(), Some(&[1, 2, 3][..]));
    assert!(dispatch(Action::DrainQueue, &mut app).is_empty());
    let (text, images, expected_turn_id, prompt_id) = {
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.state = AgentState::TurnRunning;
        agent.session.current_prompt_id = Some("newer-turn".into());
        let row = agent.session.pending_prompts.front().unwrap();
        (
            row.text.clone(),
            row.images.clone(),
            row.expected_turn_id.clone(),
            row.prompt_id.clone(),
        )
    };
    let effects = dispatch(
        Action::SteerPrompt {
            text,
            images,
            expected_turn_id,
            prompt_id,
        },
        &mut app,
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::SendInterject {
            expected_turn_id,
            interjection_id,
            ..
        }] if expected_turn_id == "turn-1" && interjection_id == "interjection-1"
    ));
}

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
            expected_turn_id: None,
            prompt_id: None,
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
            expected_turn_id: None,
            prompt_id: None,
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
