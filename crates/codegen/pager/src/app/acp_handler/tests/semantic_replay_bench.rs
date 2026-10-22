use super::*;

fn text_content(update: &acp::SessionUpdate) -> Option<&str> {
    let chunk = match update {
        acp::SessionUpdate::UserMessageChunk(chunk)
        | acp::SessionUpdate::AgentMessageChunk(chunk) => chunk,
        _ => return None,
    };
    match &chunk.content {
        acp::ContentBlock::Text(text) => Some(text.text.as_str()),
        _ => None,
    }
}

fn source_assistant_texts(directory: &std::path::Path, turns: usize) -> Vec<String> {
    let path = directory.join("updates.jsonl");
    let contents = std::fs::read_to_string(path).expect("read generated ACP source fixture");
    let mut assistants = Vec::with_capacity(turns);
    let mut current_assistant: Option<String> = None;

    for line in contents.lines() {
        let envelope: serde_json::Value =
            serde_json::from_str(line).expect("parse generated source envelope");
        let update: acp::SessionUpdate =
            serde_json::from_value(envelope["params"]["update"].clone())
                .expect("parse generated ACP source update");
        match update {
            acp::SessionUpdate::UserMessageChunk(_) => {
                if let Some(previous) = current_assistant.take() {
                    assistants.push(previous);
                }
                current_assistant = Some(String::new());
            }
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                let text = match chunk.content {
                    acp::ContentBlock::Text(text) => text.text,
                    other => panic!("unexpected non-text generated assistant content: {other:?}"),
                };
                current_assistant
                    .as_mut()
                    .expect("assistant chunks follow a user source")
                    .push_str(&text);
            }
            _ => {}
        }
    }
    if let Some(previous) = current_assistant {
        assistants.push(previous);
    }
    assert_eq!(assistants.len(), turns, "generated assistant source turns");
    for (index, assistant) in assistants.iter_mut().enumerate() {
        assistant.push_str(&format!("\nGROW_SEQ_{:06}", index + 1));
    }
    assistants
}

fn synthetic_spec(turns: usize) -> shell::session::testkit::synth::SessionSpec {
    shell::session::testkit::synth::SessionSpec {
        turns,
        acu_per_turn: 8,
        catalog_commands: 16,
        catalog_desc_len: 96,
        agent_chunks_per_turn: 8,
        agent_chunk_len: 4096,
        rewind_points: 2,
        files_per_rewind: 2,
        file_content_len: 256,
    }
}

fn selected_turn_counts() -> Vec<usize> {
    match std::env::var("SEMANTIC_REPLAY_TURNS") {
        Ok(value) => {
            let turns = value
                .parse::<usize>()
                .expect("SEMANTIC_REPLAY_TURNS must be an integer");
            assert!(
                [128, 512, 2048].contains(&turns),
                "SEMANTIC_REPLAY_TURNS must be 128, 512, or 2048"
            );
            vec![turns]
        }
        Err(std::env::VarError::NotPresent) => vec![128, 512, 2048],
        Err(error) => panic!("read SEMANTIC_REPLAY_TURNS: {error}"),
    }
}

#[tokio::test]
#[ignore = "manual semantic replay baseline; synthesizes large admitted histories"]
async fn semantic_replay_without_drawing_scales_at_128_512_2048_turns() {
    for turns in selected_turn_counts() {
        let root = tempfile::tempdir().expect("create synthetic grow home");
        let cwd = tempfile::tempdir().expect("create synthetic project");
        let spec = synthetic_spec(turns);
        let (info, directory) = shell::session::testkit::synth::replay::prepare_legacy_session(
            root.path(),
            cwd.path(),
            &spec,
        )
        .await;
        let expected_assistants = source_assistant_texts(&directory, turns);
        shell::session::testkit::synth::write_admitted_history(
            &info,
            &directory,
            &spec,
            "test-model",
            0,
            None,
        );
        let timeline_bytes = std::fs::metadata(directory.join("timeline.jsonl"))
            .expect("stat unified Timeline")
            .len();
        let assistant_source_bytes = expected_assistants.iter().map(String::len).sum::<usize>();

        // Measure the physical Timeline decode and full HistorySnapshot
        // projection separately from AppView setup and semantic application.
        let decode_project_started = std::time::Instant::now();
        let updates =
            shell::session::storage::load_updates_for_replay_at(info.id.0.as_ref(), root.path())
                .expect("load typed replay from Timeline")
                .expect("synthetic session exists");
        let decode_project_elapsed = decode_project_started.elapsed();
        assert_eq!(
            updates.len(),
            turns * 2,
            "projected replay notification count"
        );

        let projected_assistants = updates
            .iter()
            .filter_map(|update| match update {
                acp::SessionUpdate::AgentMessageChunk(_) => Some(
                    text_content(update)
                        .expect("projected assistant source is text")
                        .to_owned(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected_assistants, expected_assistants,
            "projector must preserve every assistant source byte and its order"
        );
        assert!(
            matches!(
                updates.last(),
                Some(acp::SessionUpdate::AgentMessageChunk(_))
            ),
            "last projected history source must be assistant"
        );

        // App setup is outside the handler timer. The replay window is opened
        // before dispatch, just as during reconnect replay.
        let mut app = make_app_with_agent(info.id.0.as_ref());
        app.agents
            .get_mut(&AgentId(0))
            .expect("root agent exists")
            .begin_session_reload(1);

        let mut acknowledgements = Vec::with_capacity(updates.len());
        let messages = updates
            .into_iter()
            .enumerate()
            .map(|(index, update)| {
                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                let request = acp::SessionNotification::new(info.id.clone(), update).meta(
                    serde_json::json!({
                        "eventId": format!("semantic-replay-{turns}-{index}"),
                        "isReplay": true,
                    })
                    .as_object()
                    .cloned(),
                );
                (
                    AcpClientMessage::SessionNotification(acp_transport::AcpArgs {
                        request,
                        response_tx,
                    }),
                    response_rx,
                )
            })
            .collect::<Vec<_>>();

        let handler_started = std::time::Instant::now();
        for (message, response_rx) in messages {
            let _ = handle(message, &mut app);
            acknowledgements.push(response_rx);
        }
        let handler_elapsed = handler_started.elapsed();
        for mut response_rx in acknowledgements {
            response_rx
                .try_recv()
                .expect("replayed ACP update ACK received")
                .expect("replayed ACP update ACK succeeded");
        }

        // This closes AgentView's replay batch, finishes the current turn
        // (including tracker-owned assistant/thinking state) and running
        // scrollback entries. It does not run the full
        // handle_session_loaded dispatcher (session binding, restored model,
        // code-restore notices, descendant restoration, or foreground adoption).
        let finish_started = std::time::Instant::now();
        assert!(
            app.agents
                .get_mut(&AgentId(0))
                .expect("root agent exists")
                .finish_session_reload(1, true),
            "replay load window completes"
        );
        let finish_elapsed = finish_started.elapsed();

        let agent = &app.agents[&AgentId(0)];
        let (user_messages, assistant_messages, actual_assistants) = agent
            .scrollback
            .iter_entries()
            .fold((0usize, 0usize, Vec::new()), |mut counts, (_, entry)| {
                match &entry.block {
                    RenderBlock::UserPrompt(_) => counts.0 += 1,
                    RenderBlock::AgentMessage(block) => {
                        counts.1 += 1;
                        counts.2.push(block.text());
                    }
                    _ => {}
                }
                counts
            });
        assert_eq!(user_messages, turns, "committed user message count");
        assert_eq!(
            assistant_messages, turns,
            "committed assistant message count"
        );
        assert_eq!(
            actual_assistants, expected_assistants,
            "handler must retain every assistant source byte and its order"
        );
        assert!(
            matches!(
                agent
                    .scrollback
                    .get(agent.scrollback.len().saturating_sub(1))
                    .map(|entry| &entry.block),
                Some(RenderBlock::AgentMessage(_))
            ),
            "last committed scrollback source must be assistant"
        );
        assert!(!agent.session.loading_replay, "replay window is finalized");

        eprintln!(
            "[semantic-replay-no-draw] turns={turns} notifications={} user_messages={user_messages} assistant_messages={assistant_messages} assistant_source_bytes={assistant_source_bytes} timeline_bytes={timeline_bytes} decode_project_ms={:.3} handler_ms={:.3} finish_reload_ms={:.3}",
            turns * 2,
            decode_project_elapsed.as_secs_f64() * 1000.0,
            handler_elapsed.as_secs_f64() * 1000.0,
            finish_elapsed.as_secs_f64() * 1000.0,
        );
    }
}
