use chat_state::{
    HookAggregateDecision, HookCause, HookEvent, HookEventType, HookGateDecision,
    InputAdmissionDecision, InputEvent, InputIntent, InputRoute, UserDisplayEcho,
};
use sampling_types::ConversationItem;

fn user_chunk(
    info: &Info,
    text: &str,
    chunk_meta: serde_json::Value,
    text_meta: Option<serde_json::Value>,
) -> SessionUpdate {
    let mut body = acp::TextContent::new(text);
    body.meta = text_meta.and_then(|meta| meta.as_object().cloned());
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::UserMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(body))
                .meta(chunk_meta.as_object().cloned()),
        ),
    )))
}

async fn publish(
    storage: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut chat_state::Timeline,
    label: &str,
) {
    let event = timeline.record(observation(label)).unwrap();
    storage
        .append_timeline_event_durable(info, &event)
        .await
        .unwrap();
}

fn previews(page: &history::HistoryTimelinePage) -> Vec<&str> {
    page.entries
        .iter()
        .map(|entry| entry.preview.as_str())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn fixed_cut_prompt_catalog_keeps_identity_and_rejects_illegal_ranks() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    for (text, meta, text_meta) in [
        (
            "same",
            serde_json::json!({"messageId": "a", "promptIndex": 0}),
            None,
        ),
        (
            "later chunk",
            serde_json::json!({"messageId": "a", "promptIndex": 0}),
            None,
        ),
        (
            "same",
            serde_json::json!({"messageId": "b", "promptIndex": 1}),
            None,
        ),
        ("steer me", serde_json::json!({}), None),
        (
            "secret",
            serde_json::json!({"hideFromScrollback": true, "messageId": "hidden"}),
            None,
        ),
        ("", serde_json::json!({"messageId": "empty"}), None),
        (
            "joined",
            serde_json::json!({"messageId": "combo", "promptIndex": 2}),
            Some(serde_json::json!({"combinedDisplayTexts": ["alpha", "beta"]})),
        ),
    ] {
        storage
            .append_update(&info, &user_chunk(&info, text, meta, text_meta))
            .await
            .unwrap();
    }
    publish(&storage, &info, &mut timeline, "catalog").await;

    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let page = index.timeline_newest(10_000).unwrap();
    assert_eq!(
        previews(&page),
        ["same", "same", "steer me", "alpha", "beta"]
    );
    assert_eq!(page.total_entries, 5);
    assert_eq!(page.entries[0].message_id.as_deref(), Some("a"));
    assert_eq!(page.entries[1].message_id.as_deref(), Some("b"));
    assert_eq!(page.entries[2].message_id, None);
    assert_ne!(page.entries[0].source, page.entries[1].source);
    assert_eq!(page.entries[0].source.part, 0);
    assert_eq!(page.entries[3].source, page.entries[4].source);
    assert_eq!(page.entries[3].display_segment, Some(0));
    assert_eq!(page.entries[4].display_segment, Some(1));
    assert_eq!(page.entries[0].display_segment, None);
    assert!(page.entries[0].preview.chars().count() <= 160);
    let cut = index.execution_frontier().bytes;
    let around = index.timeline_around_ordinal(3, 2).unwrap();
    assert!(
        around
            .entries
            .iter()
            .any(|entry| entry.ordinal == 3 && entry.preview == "alpha")
    );
    assert!(index.timeline_before(0, 4).is_err());
    assert!(
        index
            .timeline_around_ordinal(page.total_entries, 4)
            .is_err()
    );
    assert!(
        index
            .timeline_at(cut + 1, history::TimelineQuery::Newest, 4)
            .is_err()
    );
    drop(loaded);

    storage
        .append_update(
            &info,
            &user_chunk(
                &info,
                "after cut",
                serde_json::json!({"messageId": "later"}),
                None,
            ),
        )
        .await
        .unwrap();
    publish(&storage, &info, &mut timeline, "after").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let frozen = index
        .timeline_at(cut, history::TimelineQuery::Newest, 10)
        .unwrap();
    assert_eq!(
        previews(&frozen),
        ["same", "same", "steer me", "alpha", "beta"]
    );
    assert_eq!(frozen.total_entries, 5);
    let newest = index.timeline_newest(10).unwrap();
    assert_eq!(newest.entries.last().unwrap().preview, "after cut");
    assert_eq!(newest.total_entries, 6);
}

/// Drive one admitted prompt through the same durable lifecycle the session
/// actor records between `Submitted` and the turn that reserves it.
async fn admit_prompt(
    storage: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut chat_state::Timeline,
    input_id: &str,
    turn: TurnId,
    text: &str,
    prompt_index: usize,
) {
    let directory = storage.open_session(info).unwrap().directory_handle();
    let payload_ref = crate::session::input_inbox::write_payload(
        &directory,
        &crate::session::input_inbox::tests::text_payload("prompt", text),
    )
    .unwrap();
    let mut start = turn_start(turn, text, 0, prompt_index);
    let TimelineEventKind::Turn(TurnEvent::Started {
        input_ids,
        identity,
        ..
    }) = &mut start
    else {
        unreachable!()
    };
    input_ids.push(input_id.into());
    identity.turn_kind = "user".into();
    for kind in [
        TimelineEventKind::Input(InputEvent::Submitted {
            input_id: input_id.into(),
            intent: InputIntent::Prompt,
            payload_ref,
        }),
        TimelineEventKind::Hook(HookEvent::Triggered {
            occurrence_id: format!("hook-{input_id}"),
            event: HookEventType::UserPromptSubmit,
            gate: HookEventType::UserPromptSubmit.gate(),
            cause: HookCause::Input {
                input_id: input_id.into(),
            },
            config_generation: 1,
            handlers: Vec::new(),
        }),
        TimelineEventKind::Hook(HookEvent::Completed {
            occurrence_id: format!("hook-{input_id}"),
            decision: HookAggregateDecision::Prompt {
                decision: HookGateDecision::Allow,
            },
        }),
        TimelineEventKind::Input(InputEvent::AdmissionResolved {
            input_id: input_id.into(),
            decision: InputAdmissionDecision::Allow,
            route: Some(InputRoute::Fifo),
            supersedes: Vec::new(),
        }),
        start,
    ] {
        let event = timeline.record(kind).unwrap();
        storage
            .append_timeline_event_durable(info, &event)
            .await
            .unwrap();
    }
}

async fn append_provisional_echo(
    storage: &JsonlStorageAdapter,
    info: &Info,
    text: &str,
    message_id: &str,
    prompt_index: usize,
) {
    let block = acp::ContentBlock::Text(acp::TextContent::new(text));
    storage.append_update(info, &SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(block).meta(Some(
            serde_json::json!({"messageId": message_id, "promptIndex": prompt_index, "grow/consumedInputEcho": true}).as_object().unwrap().clone(),
        ))),
    )))).await.unwrap();
}

async fn consume_prompt(
    storage: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut chat_state::Timeline,
    input_id: &str,
    turn: TurnId,
    text: &str,
    message_id: &str,
    prompt_index: usize,
) {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(prompt_index);
    let mut canonical = serde_json::json!([acp::ContentBlock::Text(acp::TextContent::new(text))]);
    canonical.sort_all_objects();
    let consumed = timeline
        .record(TimelineEventKind::Input(InputEvent::Consumed {
            input_ids: vec![input_id.into()],
            turn,
            item,
            skill_loads: Vec::new(),
            display_echo: Some(UserDisplayEcho {
                message_id: message_id.into(),
                prompt_index,
                block_count: 1,
                blake3: blake3::hash(&serde_json::to_vec(&canonical).unwrap())
                    .to_hex()
                    .to_string(),
            }),
        }))
        .unwrap();
    storage
        .append_timeline_event_durable(info, &consumed)
        .await
        .unwrap();
}

async fn end_turn(
    storage: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut chat_state::Timeline,
    turn: TurnId,
) {
    let finish = timeline
        .record(TimelineEventKind::Turn(TurnEvent::Ended {
            id: turn,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 0,
            terminal: chat_state::TurnTerminal {
                source: chat_state::TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }))
        .unwrap();
    storage
        .append_timeline_event_durable(info, &finish)
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn provisional_echo_enters_the_catalog_only_after_consume() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    admit_prompt(
        &storage,
        &info,
        &mut timeline,
        "input",
        TurnId(1),
        "hello",
        0,
    )
    .await;
    append_provisional_echo(&storage, &info, "hello", "echo", 0).await;
    publish(&storage, &info, &mut timeline, "before-consume").await;
    let hidden = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let hidden_index = hidden.history.indexed.as_ref().unwrap();
    assert!(hidden_index.timeline_newest(8).unwrap().entries.is_empty());
    let hidden_cut = hidden_index.execution_frontier().bytes;
    drop(hidden);

    consume_prompt(
        &storage,
        &info,
        &mut timeline,
        "input",
        TurnId(1),
        "hello",
        "echo",
        0,
    )
    .await;
    // The consumed suffix only enters a queryable catalog through the next
    // indexed publication; journal appends below the publication thresholds
    // stay outside every published span.
    publish(&storage, &info, &mut timeline, "consume").await;
    let shown = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let shown_index = shown.history.indexed.as_ref().unwrap();
    let page = shown_index.timeline_newest(8).unwrap();
    assert_eq!(previews(&page), ["hello"]);
    assert_eq!(page.entries[0].message_id.as_deref(), Some("echo"));
    assert_eq!(page.entries[0].source.part, 0);
    let frozen = shown_index
        .timeline_at(hidden_cut, history::TimelineQuery::Newest, 10)
        .unwrap();
    assert!(frozen.entries.is_empty());
    let consume_generation = shown_index
        .timeline_generation(shown_index.execution_frontier().bytes)
        .unwrap();
    drop(shown);

    // The next publication keeps the revealed prompt ranked while a fresh
    // provisional echo stays hidden until its own consume.
    end_turn(&storage, &info, &mut timeline, TurnId(1)).await;
    admit_prompt(
        &storage,
        &info,
        &mut timeline,
        "input2",
        TurnId(2),
        "steer",
        1,
    )
    .await;
    append_provisional_echo(&storage, &info, "steer", "echo2", 1).await;
    publish(&storage, &info, &mut timeline, "next").await;
    let next = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let next_index = next.history.indexed.as_ref().unwrap();
    assert_eq!(previews(&next_index.timeline_newest(8).unwrap()), ["hello"]);
    drop(next);

    consume_prompt(
        &storage,
        &info,
        &mut timeline,
        "input2",
        TurnId(2),
        "steer",
        "echo2",
        1,
    )
    .await;
    publish(&storage, &info, &mut timeline, "consume2").await;
    let shown = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let shown_index = shown.history.indexed.as_ref().unwrap();
    let page = shown_index.timeline_newest(8).unwrap();
    assert_eq!(previews(&page), ["hello", "steer"]);
    assert_eq!(page.entries[0].message_id.as_deref(), Some("echo"));
    assert_eq!(page.entries[1].message_id.as_deref(), Some("echo2"));
    // Revealing a later source extends the published generation instead of
    // rebuilding the ranks for every consecutive consume.
    assert_eq!(
        shown_index
            .timeline_generation(shown_index.execution_frontier().bytes)
            .unwrap(),
        consume_generation
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rewind_drops_the_discarded_prompt_and_keeps_ranks_contiguous() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline =
        chat_state::Timeline::from_seed(vec![ConversationItem::system("fixed seed")]).unwrap();
    storage
        .append_timeline_event(&info, &timeline.events()[0])
        .await
        .unwrap();
    for (id, index, text) in [
        (10, 0, "selected user prompt"),
        (11, 1, "discarded user prompt"),
    ] {
        let start = timeline
            .record(turn_start(TurnId(id), text, timeline.surface_len(), index))
            .unwrap();
        let mut input = ConversationItem::user(text);
        input.set_prompt_index(index);
        let message = timeline.append(input, MessageCause::User).unwrap();
        let finish = timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: TurnId(id),
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: chat_state::TurnTerminal {
                    source: chat_state::TurnTerminalSource::Host,
                    stop_reason: "end_turn".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        for event in [&start, &message, &finish] {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        storage
            .append_update(
                &info,
                &user_chunk(
                    &info,
                    text,
                    serde_json::json!({"messageId": format!("echo-{id}"), "promptIndex": index}),
                    None,
                ),
            )
            .await
            .unwrap();
    }
    publish(&storage, &info, &mut timeline, "before-rewind").await;
    let rewind = timeline
        .replace_all(timeline.rewind_surface(1).unwrap(), MessageCause::Rewind)
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &rewind)
        .await
        .unwrap();
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let page = loaded
        .history
        .indexed
        .as_ref()
        .unwrap()
        .timeline_newest(8)
        .unwrap();
    assert_eq!(previews(&page), ["selected user prompt"]);
    assert_eq!(page.entries[0].ordinal, 0);
    assert_eq!(page.total_entries, 1);
}
