use super::*;

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
    assert!(failed_image_submission(&mut app).is_empty());
    let agent = &app.agents[&AgentId(0)];
    assert!(agent.prompt.text().starts_with("describe [Image #1]"));
    assert_eq!(agent.prompt.images.len(), 1);
    assert_eq!(
        agent.prompt.images[0].encoded_bytes.as_deref(),
        Some(&[1, 2, 3, 4][..])
    );
    let text = agent.prompt.text().to_owned();
    let effects = dispatch(Action::SendPrompt(text), &mut app);
    assert!(
        matches!(effects.as_slice(), [Effect::SendPromptBlocks { images, .. }] if images.len() == 1)
    );
}

#[test]
fn failed_image_admission_preserves_newer_draft_and_holds_retry() {
    let mut app = test_app_with_agent();
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .prompt
        .set_text("new draft");
    assert!(failed_image_submission(&mut app).is_empty());
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
    assert!(
        dispatch(
            Action::TaskComplete(TaskResult::InterjectFailed {
                agent_id: AgentId(0),
                error: "admission failed".into(),
                text: "image".into(),
                blocks: None,
                images: vec![image],
            }),
            &mut app
        )
        .is_empty()
    );
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
