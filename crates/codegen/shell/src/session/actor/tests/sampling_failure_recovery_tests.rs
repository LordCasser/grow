//! Real session command/foreground settlement with a budgeted child's context.
//! The shared actor fixture substitutes persistence acknowledgements; the
//! built-binary protocol recovery test also exercises the actual disk writer.
use super::support::*;
use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};

fn malformed(known_usage: bool) -> ScriptedResponse {
    let mut response = json!({
        "id":"bad-budgeted-response", "object":"chat.completion.chunk", "created":0,
        "model":"test-model", "choices":[{"index":0,"delta":{"tool_calls":[{
            "index":0,"id":"call_invalid","type":"function",
            "function":{"name":"todo_write","arguments":"{"}
        }]},"finish_reason":"tool_calls"}]
    });
    if known_usage {
        response["usage"] = json!({"prompt_tokens":10,"completion_tokens":5,"total_tokens":15});
    }
    ScriptedResponse::sse(vec![
        SseEvent::data(response.to_string()),
        SseEvent::data("[DONE]"),
    ])
}

fn queue_user(
    commands: &tokio::sync::mpsc::UnboundedSender<SessionCommand>,
    id: &str,
) -> tokio::sync::oneshot::Receiver<crate::session::commands::PromptTurnResult> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    commands
        .send(SessionCommand::QueuePrompt {
            prompt_id: id.into(),
            prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new("继续"))],
            origin: crate::session::PromptOrigin::User,
            turn_kind: crate::session::TurnKind::User,
            client_identifier: None,
            screen_mode: None,
            verbatim: false,
            json_schema: None,
            respond_to,
            persist_ack: None,
        })
        .unwrap();
    response
}

async fn finish_user(
    response: tokio::sync::oneshot::Receiver<crate::session::commands::PromptTurnResult>,
) -> crate::session::commands::PromptTurnResult {
    tokio::time::timeout(Duration::from_secs(30), response)
        .await
        .expect("normal actor settlement must finish the prompt")
        .expect("actor must retain its prompt responder")
}

#[test]
fn child_grant_survives_known_sampling_failure_but_closes_for_exhaustion_or_unknown_usage() {
    // Match production session threads' debug stack without altering the test
    // process environment or relying on the default libtest stack size.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(tokio::task::LocalSet::new().run_until(async {
                for (grant, known_usage, attempts, lose_terminal) in [
                    (80, true, 3, false),
                    (10, true, 2, false),
                    (80, false, 1, false),
                    (80, true, 3, true),
                ] {
                    let server = MockInferenceServer::start().await.unwrap();
                    for _ in 0..attempts {
                        server.enqueue_response("/v1/chat/completions", malformed(known_usage));
                    }
                    server.enqueue_response(
                        "/v1/chat/completions",
                        ScriptedResponse::sse(test_support::sse::chat_completion_script_exact(
                            "recovered",
                            "test-model",
                        )),
                    );
                    let (gateway, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                    tokio::task::spawn_local(async move {
                        while let Some(message) = gateway_rx.recv().await {
                            match message {
                                acp_transport::AcpClientMessage::SessionNotification(args) => {
                                    let _ = args.response_tx.send(Ok(()));
                                }
                                acp_transport::AcpClientMessage::ExtNotification(args) => {
                                    let _ = args.response_tx.send(Ok(()));
                                }
                                _ => {}
                            }
                        }
                    });
                    let (persistence, mut persisted) = tokio::sync::mpsc::unbounded_channel();
                    tokio::task::spawn_local(async move {
                        while let Some(message) = persisted.recv().await {
                            match message {
                                PersistenceMsg::CommitResponseProjection { respond_to, .. } => {
                                    let _ = respond_to.send(Ok(()));
                                }
                                PersistenceMsg::FlushAndAck { respond_to } => {
                                    let _ = respond_to.send(());
                                }
                                _ => {}
                            }
                        }
                    });
                    let (mut actor, events) =
                        create_test_actor_ex(0, 256_000, 85, gateway, persistence).await;
                    actor.startup_hints.is_subagent = true;
                    actor.session_info.cwd = actor.session_dir.to_string_lossy().into_owned();
                    actor.max_retries.set(5);
                    let budget = crate::tools::tool_context::TaskOutputTokenBudget::limited(grant);
                    actor.tool_context.task_output_token_budget = Some(budget.clone());
                    actor.tool_context.sampling_output_delivery =
                        sampler::OutputDelivery::Retractable;
                    let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                    config.base_url = server.url();
                    config.model = "test-model".into();
                    let (timeline_persistence, mut timeline_acks) =
                        chat_state::MockTimelinePersistence::new_with_manual_timeline_ack_after(1);
                    let (chat_events, chat_rx) = tokio::sync::mpsc::unbounded_channel();
                    actor.chat_state_handle = chat_state::ChatStateActor::spawn(
                        vec![sampling_types::ConversationItem::system(
                            "test system prompt",
                        )],
                        config,
                        Box::new(timeline_persistence),
                        chat_events,
                        tokio_util::sync::CancellationToken::new(),
                    );
                    actor.events =
                        crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
                    let (terminal_held, held) = tokio::sync::oneshot::channel();
                    let release_terminal = Arc::new(tokio::sync::Notify::new());
                    let release = release_terminal.clone();
                    let state = actor.chat_state_handle.clone();
                    let timeline_ack_worker = tokio::task::spawn_local(async move {
                        let mut terminal_held = Some(terminal_held);
                        loop {
                            let Some(ack) = timeline_acks.next_timeline_ack().await else {
                                continue;
                            };
                            if terminal_held.is_some()
                                && state.pending_timeline_commit().is_some_and(|pending| {
                                    matches!(
                                        pending.event.kind,
                                        chat_state::TimelineEventKind::Turn(
                                            chat_state::TurnEvent::Ended { .. }
                                        )
                                    )
                                })
                            {
                                let _ = terminal_held.take().unwrap().send(());
                                release.notified().await;
                            }
                            let _ = ack.send(Ok(()));
                            timeline_acks.drain();
                        }
                    });
                    let config = actor.reconstruct_full_config().await;
                    let (sampler_events, mut sampler_rx) = tokio::sync::mpsc::unbounded_channel();
                    let owner = sampler::SamplerActor::spawn_owned(
                        config,
                        sampler::RetryPolicy {
                            output_delivery: sampler::OutputDelivery::Retractable,
                            ..Default::default()
                        },
                        sampler_events,
                    );
                    actor.sampler_handle = owner.handle();
                    *actor.sampler_owner.borrow_mut() = Some(owner);
                    let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
                    actor.goal_usage_window =
                        goal_support::GoalUsageWindow::new(commands.clone(), None);
                    let actor = Arc::new(actor);
                    let drainer = actor.clone();
                    actor
                        .sampler_event_drainer
                        .arm(tokio::task::spawn_local(async move {
                            while let Some(event) = sampler_rx.recv().await {
                                if lose_terminal
                                    && matches!(event, sampler::SamplingEvent::Failed { .. })
                                {
                                    continue;
                                }
                                drainer.handle_sampling_event(event).await;
                            }
                        }));
                    let session = actor.clone();
                    let loop_task = tokio::task::spawn_local(async move {
                        let root = session.session_dir.clone();
                        super::super::run_loop::run_session(
                            session,
                            command_rx,
                            chat_rx,
                            events,
                            None,
                            Arc::new(parking_lot::Mutex::new(CodebaseIndexManager::default())),
                            root,
                            Default::default(),
                        )
                        .await;
                    });
                    let first = queue_user(&commands, "bad-sample");
                    tokio::time::timeout(Duration::from_secs(30), held)
                        .await
                        .expect("failed Turn must reach its durable terminal")
                        .expect("terminal acknowledgement must be held");
                    assert!(matches!(
                        actor.state.lock().await.foreground,
                        ForegroundState::Settling { .. }
                    ));
                    assert_eq!(server.requests().len(), attempts);
                    assert_eq!(
                        budget.usage(),
                        if known_usage && !lose_terminal {
                            (attempts as u64 * 5, false)
                        } else {
                            (grant, true)
                        },
                        "only confirmed usage, not a sampling error, determines grant completeness"
                    );
                    let before_continue = server.requests().len();
                    let second = queue_user(&commands, "manual-continue");
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    assert_eq!(
                        server.requests().len(),
                        before_continue,
                        "new input cannot sample before its predecessor's terminal ACK"
                    );
                    release_terminal.notify_one();
                    assert!(
                        finish_user(first).await.is_err(),
                        "malformed samples must end this Turn"
                    );
                    let second = finish_user(second).await;
                    if known_usage && !lose_terminal && grant > attempts as u64 * 5 {
                        second.expect("same actor must admit work within its remaining grant");
                        assert_eq!(server.requests().len(), before_continue + 1);
                        let body = server.request_bodies().pop().unwrap();
                        assert!(
                            body["max_tokens"].as_u64().unwrap() <= grant - attempts as u64 * 5
                        );
                        assert!(!body["messages"].to_string().contains("call_invalid"));
                    } else {
                        assert!(
                            second.is_err(),
                            "new input must not replenish the child grant"
                        );
                        assert_eq!(server.requests().len(), before_continue);
                    }
                    let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
                    let first_end = timeline
                        .iter()
                        .position(|event| {
                            matches!(
                                event.kind,
                                chat_state::TimelineEventKind::Turn(
                                    chat_state::TurnEvent::Ended { .. }
                                )
                            )
                        })
                        .unwrap();
                    let successor_start = timeline
                        .iter()
                        .enumerate()
                        .filter_map(|(index, event)| {
                            matches!(
                                event.kind,
                                chat_state::TimelineEventKind::Turn(
                                    chat_state::TurnEvent::Started { .. }
                                )
                            )
                            .then_some(index)
                        })
                        .nth(1)
                        .unwrap();
                    assert!(
                        first_end < successor_start,
                        "durable predecessor terminal must precede the next Turn"
                    );
                    assert_eq!(
                        timeline
                            .iter()
                            .filter(|event| matches!(
                                event.kind,
                                chat_state::TimelineEventKind::Turn(
                                    chat_state::TurnEvent::Ended { .. }
                                )
                            ))
                            .count(),
                        2
                    );
                    assert!(
                        !timeline.iter().any(|event| matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Tool(_)
                        ))
                    );
                    assert!(actor.turn_continuation.borrow().is_none());
                    assert!(actor.sampling_continuation.borrow().is_none());
                    commands.send(SessionCommand::Shutdown).unwrap();
                    tokio::time::timeout(Duration::from_secs(10), loop_task)
                        .await
                        .unwrap()
                        .unwrap();
                    timeline_ack_worker.abort();
                }
            }));
        })
        .unwrap()
        .join()
        .unwrap();
}
