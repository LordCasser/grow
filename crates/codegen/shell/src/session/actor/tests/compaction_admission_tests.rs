//! Faults arriving at the completed-compaction / next-provider boundary.
use super::*;

#[test]
fn completed_compaction_preserves_shared_storage_admission_error() {
    storage_failure_at_compaction_completion(true);
}

#[test]
fn completed_compaction_ignores_another_sessions_local_storage_failure() {
    storage_failure_at_compaction_completion(false);
}

fn storage_failure_at_compaction_completion(shared: bool) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("partial answer")], CONTEXT_WINDOW_EXCEEDED),
            );
            let summary = format!(
                "ADMISSION_BOUNDARY_SUMMARY: {}",
                "verified facts and remaining authorized work. ".repeat(30)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block(&summary)], END_TURN),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("continued after compaction")], END_TURN),
            );
            let root = tempfile::tempdir().unwrap();
            let gate = std::sync::Arc::new(
                crate::session::storage_admission::StorageAdmission::new(root.path().into()),
            );
            let writer_gate = gate.clone();
            let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, persistence_rx) = mpsc::unbounded_channel();
            // This fixture controls ACK ordering, not disk capacity. A second
            // session's failure arrives after replacement and before the
            // Completed ACK, so no sleeps or foreground-state edits are needed.
            let writer = spawn_ack_writer(persistence_rx, move |event, respond_to| {
                if matches!(
                    event.kind,
                    chat_state::TimelineEventKind::Compaction(
                        chat_state::CompactionEvent::Completed { .. }
                    )
                ) {
                    writer_gate.record_failure(
                        "another-session",
                        "failed-writer",
                        None,
                        Some(if shared {
                            std::io::ErrorKind::StorageFull
                        } else {
                            std::io::ErrorKind::InvalidData
                        }),
                        "injected storage failure after summary replacement",
                    );
                }
                let _ = respond_to.send(Ok(()));
            });
            let (mut actor, events) = create_test_actor_ex_with_physical_projection(
                0,
                256_000,
                85,
                gateway_tx,
                persistence_tx,
            )
            .await;
            actor.tool_context.storage_admission = Some(gate.clone());
            let (actor, _gateway, _events) = configure_actor_with_sampler(
                &server,
                sampling_types::ApiBackend::Messages,
                sampler::OutputDelivery::Irreversible,
                None,
                actor,
                gateway_rx,
                events,
            )
            .await;
            seed_closed_compaction_range(&actor).await;
            let result = run_user_turn(&actor, "compaction-admission-fault").await;
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let completed = events
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Compaction(
                            chat_state::CompactionEvent::Completed { .. }
                        )
                    )
                })
                .count();
            let terminal = events.iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended {
                    outcome,
                    ..
                }) => Some(outcome.as_str()),
                _ => None,
            });
            writer.abort();
            let _ = writer.await;
            assert_eq!(completed, 1, "the replacement already committed");
            assert!(
                actor
                    .chat_state_handle
                    .get_conversation()
                    .await
                    .iter()
                    .any(|item| item.text_content().contains("ADMISSION_BOUNDARY_SUMMARY"))
            );
            assert_eq!(server.messages_request_count(), if shared { 2 } else { 3 });
            if shared {
                let error = result.expect_err("storage admission must fail, not cancel the turn");
                assert!(
                    format!("{error:?}")
                        .contains("injected storage failure after summary replacement")
                );
                assert_eq!(terminal, Some("error"));
                assert!(gate.check(actor.session_info.id.0.as_ref()).is_err());
            } else {
                assert_eq!(result.unwrap().stop_reason, acp::StopReason::EndTurn);
                assert_eq!(terminal, Some("completed"));
                assert!(gate.check(actor.session_info.id.0.as_ref()).is_ok());
            }
        }));
    });
}

// Forward the real ChatState ACK protocol while controlling one selected
// frontier. This does not claim to test filesystem persistence.
fn spawn_ack_writer(
    mut receiver: mpsc::UnboundedReceiver<PersistenceMsg>,
    mut on_timeline: impl FnMut(
        chat_state::TimelineEvent,
        tokio::sync::oneshot::Sender<std::io::Result<()>>,
    ) + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_local(async move {
        while let Some(message) = receiver.recv().await {
            match message {
                PersistenceMsg::TimelineDurablyAndAck { event, respond_to } => {
                    on_timeline(event, respond_to)
                }
                PersistenceMsg::SidebandDurablyAndAck { respond_to, .. }
                | PersistenceMsg::SamplingBarrier { respond_to, .. } => {
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::AppendUpdateBufferedAndAck { respond_to, .. }
                | PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. }
                | PersistenceMsg::PreviewIndependent { respond_to, .. } => {
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::AuxiliaryGrow {
                    delivery,
                    respond_to,
                    ..
                } => {
                    if let Some(delivery) = delivery {
                        delivery.publish();
                    }
                    if let Some(respond_to) = respond_to {
                        let _ = respond_to.send(Ok(()));
                    }
                }
                _ => {}
            }
        }
    })
}

#[test]
fn async_generation_cancel_before_replacement_preserves_foreground() {
    async_generation_cancellation_at_ack(false);
}

#[test]
fn async_generation_cancel_after_replacement_preserves_foreground() {
    async_generation_cancellation_at_ack(true);
}

fn async_generation_cancellation_at_ack(after_replacement: bool) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
            use test_support::{InferenceEndpoint, InferenceRequestMatcher};
            use tools::implementations::grow_build::{
                read_file::ReadFileTool, todo::TodoWriteTool,
            };
            use tools::registry::types::ToolConfig;

            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!(
                "ASYNC_ACK_SUMMARY: {}",
                "closed historical facts and verified work. ".repeat(40)
            );
            let mut auxiliary = server.expect_response(
                "background summary",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn(&[text_block(&summary)], END_TURN),
            );
            let mut tool = server.expect_response_blocked(
                "tool Step",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_turn(
                    &[tool_use_block(
                        "compaction-race-tool",
                        "todo_write",
                        r#"{"todos":[],"actions":[]}"#,
                    )],
                    "tool_use",
                ),
            );
            let mut continuation = server.expect_response(
                "same Turn successor",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_turn(
                    &[text_block("foreground survived generation cancellation")],
                    END_TURN,
                ),
            );
            let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, persistence_rx) = mpsc::unbounded_channel();
            let (held_tx, mut held_rx) = mpsc::unbounded_channel();
            let writer = spawn_ack_writer(persistence_rx, move |event, respond_to| {
                let hold = match event.kind {
                    chat_state::TimelineEventKind::Compaction(
                        chat_state::CompactionEvent::Summary { .. },
                    ) => !after_replacement,
                    chat_state::TimelineEventKind::Compaction(
                        chat_state::CompactionEvent::Completed { .. },
                    ) => after_replacement,
                    _ => false,
                };
                if hold {
                    held_tx.send(respond_to).unwrap();
                } else {
                    let _ = respond_to.send(Ok(()));
                }
            });
            let (actor, events) = create_test_actor_ex_with_physical_projection(
                0,
                100_000,
                80,
                gateway_tx,
                persistence_tx,
            )
            .await;
            let (actor, _gateway, _events) = configure_actor_with_sampler(
                &server,
                sampling_types::ApiBackend::Messages,
                sampler::OutputDelivery::Irreversible,
                None,
                actor,
                gateway_rx,
                events,
            )
            .await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                ToolConfig::for_tool::<TodoWriteTool>(),
                ToolConfig::for_tool::<ReadFileTool>(),
            ])
            .await;
            actor.compaction.pre_prune.set(false);
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::system("test system"),
                    prompt("old task", 0),
                    ConversationItem::assistant("x".repeat(200_000)),
                    prompt("recent task", 1),
                    ConversationItem::assistant("y".repeat(88_000)),
                ],
            )
            .await;
            install_test_planning_resources(&actor, "async-cancel-ack").await;
            let turn = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { run_user_turn(&actor, "async-cancel-ack").await }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), tool.wait_blocked())
                .await
                .unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                auxiliary.wait_satisfied(),
            )
            .await
            .unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.session_activities.wait_idle(),
            )
            .await
            .unwrap();
            tool.release();
            let ack = tokio::time::timeout(std::time::Duration::from_secs(5), held_rx.recv())
                .await
                .unwrap()
                .unwrap();
            // The publisher has taken the slot; cancellation must signal its
            // original generation token without waiting on this held ACK.
            assert!(actor.compaction.background.borrow().is_none());
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                actor.cancel_background_compaction("test_generation_invalidated"),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(
                actor.compaction.cancel.is_cancelled(),
                "the publisher must receive the generation cancellation while its ACK is held"
            );
            assert!(
                actor
                    .state
                    .lock()
                    .await
                    .can_continue_regular_turn("async-cancel-ack")
            );
            ack.send(Ok(())).unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(10), turn)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
            continuation.wait_satisfied().await;
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                    ))
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Compaction(
                            chat_state::CompactionEvent::Completed { .. }
                        )
                    ))
                    .count(),
                usize::from(after_replacement)
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                conversation
                    .iter()
                    .any(|item| item.text_content().contains("ASYNC_ACK_SUMMARY")),
                after_replacement
            );
            assert!(conversation.iter().any(|item| {
                item.text_content()
                    .contains("foreground survived generation cancellation")
            }));
            assert_eq!(server.messages_request_count(), 3);
            assert!(
                !actor.compaction.cancel.is_cancelled(),
                "the completed scope must not leak a cancelled token"
            );
            writer.abort();
            let _ = writer.await;
        }));
    });
}

#[test]
fn suspension_retires_foreground_after_compaction_drains() {
    suspension_at_summary_ack(false);
}

#[test]
fn suspension_preserves_unacknowledged_compaction_for_retry() {
    suspension_at_summary_ack(true);
}

fn suspension_at_summary_ack(expire_first: bool) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("partial")], CONTEXT_WINDOW_EXCEEDED),
            );
            let mut summary = server.expect_response_blocked(
                "suspend summary",
                test_support::InferenceRequestMatcher::auxiliary(
                    test_support::InferenceEndpoint::Messages,
                ),
                messages_turn(
                    &[text_block(&format!(
                        "SUSPEND_SUMMARY {}",
                        "verified history. ".repeat(100)
                    ))],
                    END_TURN,
                ),
            );
            let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, persistence_rx) = mpsc::unbounded_channel();
            let (held_tx, mut held_rx) = mpsc::unbounded_channel();
            let writer = spawn_ack_writer(persistence_rx, move |event, respond_to| {
                if matches!(
                    event.kind,
                    chat_state::TimelineEventKind::Compaction(
                        chat_state::CompactionEvent::Failed { .. }
                    )
                ) {
                    held_tx.send(respond_to).unwrap();
                } else {
                    let _ = respond_to.send(Ok(()));
                }
            });
            let (actor, events) = create_test_actor_ex_with_physical_projection(
                0,
                256_000,
                85,
                gateway_tx,
                persistence_tx,
            )
            .await;
            let (actor, _gateway, _events) = configure_actor_with_sampler(
                &server,
                sampling_types::ApiBackend::Messages,
                sampler::OutputDelivery::Irreversible,
                None,
                actor,
                gateway_rx,
                events,
            )
            .await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::todo::TodoWriteTool,
                >(),
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::read_file::ReadFileTool,
                >(),
            ])
            .await;
            seed_closed_compaction_range(&actor).await;
            let prompt_id = "suspend-compaction";
            install_test_planning_resources(&actor, prompt_id).await;
            let input_ids =
                crate::session::actor::tests::support::admit_test_human_input(&actor, prompt_id)
                    .await;
            let (start_tx, start_rx) = tokio::sync::oneshot::channel();
            let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
            let turn = tokio::task::spawn_local({
                let actor = actor.clone();
                async move {
                    // Match AgentTask's physical stopped ownership; unlike
                    // running_task_stub, this sender belongs to handle_prompt.
                    let _stopped = stopped_tx;
                    start_rx.await.unwrap();
                    actor
                        .handle_prompt(
                            prompt_id,
                            input_ids,
                            crate::session::PromptOrigin::User,
                            vec![],
                            crate::session::TurnKind::User,
                            vec![acp::ContentBlock::Text(acp::TextContent::new(
                                "continue work",
                            ))],
                            tool_types::BehaviorId::Normal,
                            None,
                            None,
                            false,
                            None,
                            None,
                        )
                        .await
                }
            });
            actor.state.lock().await.foreground = ForegroundState::RegularTurn(AgentTask {
                stopped: futures_util::FutureExt::shared(stopped_rx),
                prompt_id: prompt_id.into(),
                origin: crate::session::PromptOrigin::User,
                turn_kind: crate::session::TurnKind::User,
                turn_start_ms: 0,
                usage_epoch: 0,
                handle: turn.abort_handle(),
                steering_open: true,
            });
            start_tx.send(()).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), summary.wait_blocked())
                .await
                .unwrap();
            assert!(actor.compaction.lease.is_in_flight());
            let suspend = |actor: std::sync::Arc<SessionActor>, millis| async move {
                let (tx, rx) = tokio::sync::oneshot::channel();
                actor
                    .request_node_suspension(
                        "suspend-compaction-op".into(),
                        actor.session_id_string(),
                        tokio::time::Instant::now() + std::time::Duration::from_millis(millis),
                        tx,
                    )
                    .await;
                rx.await.unwrap()
            };
            let first = tokio::task::spawn_local(suspend(
                actor.clone(),
                if expire_first { 100 } else { 2000 },
            ));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !actor.compaction.cancel.is_cancelled() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let ack = tokio::time::timeout(std::time::Duration::from_secs(5), held_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                !turn.is_finished(),
                "unconfirmed compaction terminal must retain its owner"
            );
            if expire_first {
                let error = first
                    .await
                    .unwrap()
                    .expect_err("held ACK must prevent suspension");
                assert!(error.contains("compaction"), "{error}");
                assert!(actor.compaction.lease.is_in_flight());
                assert!(!turn.is_finished());
                ack.send(Ok(())).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while actor.compaction.lease.is_in_flight() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert!(
                    !turn.is_finished(),
                    "handle_prompt is parked for runtime handoff"
                );
                suspend(actor.clone(), 2000)
                    .await
                    .expect("same operation retry must finish");
            } else {
                ack.send(Ok(())).unwrap();
                first
                    .await
                    .unwrap()
                    .expect("drained compaction must not wait on parked foreground");
            }
            assert!(
                turn.await.unwrap_err().is_cancelled(),
                "suspension physically retires the parked task"
            );
            assert_eq!(
                actor.chat_state_handle.runtime_state().await.unwrap().phase,
                chat_state::session_runtime::RuntimePhase::Suspended
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(
                !events.iter().any(|event| matches!(
                    event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. })
                )),
                "suspension must not manufacture a logical Turn terminal"
            );
            assert_eq!(server.messages_request_count(), 2);
            writer.abort();
            let _ = writer.await;
        }));
    });
}
