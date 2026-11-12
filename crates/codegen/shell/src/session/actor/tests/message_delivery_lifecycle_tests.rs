//! Consultation receipts retain Session ownership through real mailbox lifecycle controls.
//! Persistence acknowledgements use the existing actor fixture; deletion uses the
//! separate real-files MvpAgent fixture.
use super::*;

async fn lifecycle_actor(
    server: &MockInferenceServer,
    previous: Option<&SessionActor>,
) -> (Arc<SessionActor>, mpsc::UnboundedReceiver<SessionEvent>) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
    let (persistence_tx, mut persisted) = mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(message) = persisted.recv().await {
            if let PersistenceMsg::FlushAndAck { respond_to } = message {
                let _ = respond_to.send(());
            }
        }
    });
    let (mut actor, events) = create_test_actor_ex_with_projection_error(
        0,
        256_000,
        85,
        gateway_tx,
        persistence_tx,
        None,
    )
    .await;
    actor.startup_hints.is_subagent = true;
    if let Some(previous) = previous {
        actor.session_info = previous.session_info.clone();
        actor.session_dir = previous.session_dir.clone();
        actor.session_directory = previous.session_directory.clone();
        actor.model_route = previous.model_route.clone();
        let (chat_events, _) = mpsc::unbounded_channel();
        actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
            previous.chat_state_handle.timeline_events().await.unwrap(),
            previous
                .chat_state_handle
                .get_sampling_config()
                .await
                .unwrap(),
            Box::new(chat_state::NullTimelinePersistence),
            chat_events,
            Default::default(),
        )
        .await
        .unwrap();
        actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
        actor
            .state
            .lock()
            .await
            .runtime_suspend
            .store(true, std::sync::atomic::Ordering::Release);
    }
    let (actor, mut gateway_rx, events) = configure_actor_with_sampler(
        server,
        sampling_types::ApiBackend::Messages,
        sampler::OutputDelivery::Irreversible,
        None,
        actor,
        gateway_rx,
        events,
    )
    .await;
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
    communication_agent(&actor).await;
    (actor, events)
}

async fn start_lifecycle_mailbox(
    actor: Arc<SessionActor>,
    events: mpsc::UnboundedReceiver<SessionEvent>,
) -> (
    mpsc::UnboundedSender<SessionCommand>,
    tokio::task::JoinHandle<()>,
) {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (chat_owner, chat_rx) = mpsc::unbounded_channel();
    let task = tokio::task::spawn_local(async move {
        let _chat_owner = chat_owner;
        let root = actor.session_dir.clone();
        crate::session::actor::run_loop::run_session(
            actor,
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
    let (responds_to, ready) = tokio::sync::oneshot::channel();
    commands
        .send(SessionCommand::GetActiveAgent { responds_to })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), ready)
        .await
        .expect("lifecycle mailbox startup must complete before provider timing")
        .unwrap();
    (commands, task)
}

fn lifecycle_user(
    commands: &mpsc::UnboundedSender<SessionCommand>,
    id: &str,
) -> tokio::sync::oneshot::Receiver<crate::session::commands::PromptTurnResult> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    commands
        .send(SessionCommand::QueuePrompt {
            prompt_id: id.into(),
            prompt_blocks: vec![acp::ContentBlock::from(format!("Explicit user task {id}"))],
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

async fn lifecycle_answer(
    actor: &SessionActor,
    commands: &mpsc::UnboundedSender<SessionCommand>,
    server: &MockInferenceServer,
    id: &str,
) -> String {
    let expected = server.expect_response(
        id,
        test_support::InferenceRequestMatcher::auxiliary(test_support::InferenceEndpoint::Messages),
        messages_turn(
            &[text_block(
                "A completed consultation remains Session evidence.",
            )],
            END_TURN,
        ),
    );
    let (inquiry, response) =
        live_consultation(actor, id, "What must survive this lifecycle control?");
    commands
        .send(SessionCommand::RunCoordinationInquiry { inquiry })
        .unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), response)
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::coordination::InquiryStatus::Answered
    );
    assert!(expected.is_satisfied());
    let pending = actor
        .chat_state_handle
        .pending_notifications()
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    pending[0].id.clone()
}

fn assert_one_lifecycle_consumption(events: &[chat_state::TimelineEvent], receipt: &str) {
    assert_eq!(events.iter().filter(|event| matches!(&event.kind,
        chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, input: Some(_), .. })
            if notification_ids.iter().any(|id| id == receipt))).count(), 1);
}

#[test]
fn mailbox_cancel_to_idle_keeps_pending_consultation_until_new_user_turn() {
    run_with_session_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tokio::task::LocalSet::new().run_until(async {
                let server = MockInferenceServer::start().await.unwrap();
                let mut original = server.expect_response_blocked(
                    "cancel original foreground",
                    test_support::InferenceRequestMatcher::foreground(
                        test_support::InferenceEndpoint::Messages,
                    ),
                    messages_turn(
                        &[text_block(
                            "This response must not reopen the cancelled Turn.",
                        )],
                        END_TURN,
                    ),
                );
                let (actor, events) = lifecycle_actor(&server, None).await;
                let (commands, loop_task) = start_lifecycle_mailbox(actor.clone(), events).await;
                let original_reply = lifecycle_user(&commands, "cancelled-original");
                tokio::time::timeout(std::time::Duration::from_secs(5), original.wait_blocked())
                    .await
                    .unwrap();
                let old_turn = actor.current_prompt_id.lock().unwrap().clone().unwrap();
                assert_eq!(old_turn, "cancelled-original");
                let receipt =
                    lifecycle_answer(&actor, &commands, &server, "pending-before-ctrl-c").await;
                commands
                    .send(SessionCommand::Cancel {
                        cancel_subagents: false,
                        kill_background_tasks: false,
                        rewind_if_pristine: false,
                        pause_goal: false,
                        trigger: Some("ctrl_c".into()),
                    })
                    .unwrap();
                let result =
                    tokio::time::timeout(std::time::Duration::from_secs(10), original_reply)
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                assert!(matches!(
                    result.completion_kind,
                    crate::session::commands::PromptCompletionKind::Cancelled { .. }
                ));
                let (responds_to, barrier) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::GetActiveAgent { responds_to })
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), barrier)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(actor.state.lock().await.foreground.is_idle());
                assert!(actor.state.lock().await.notifications_suppressed);
                assert_eq!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()[0]
                        .id,
                    receipt
                );
                let (respond_to, stale) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::SteerTurn {
                        expected_turn_id: old_turn,
                        text: "stale exact Turn".into(),
                        id: None,
                        images: vec![],
                        respond_to,
                    })
                    .unwrap();
                assert!(
                    tokio::time::timeout(std::time::Duration::from_secs(5), stale)
                        .await
                        .unwrap()
                        .unwrap()
                        .is_err()
                );
                assert_eq!(
                    server.requests().len(),
                    2,
                    "a passive receipt and Ctrl+C cannot open another Turn"
                );
                assert!(
                    !actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .iter()
                        .any(|event| matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Notification(
                                chat_state::NotificationEvent::Consumed { .. }
                            )
                        ))
                );
                original.release();
                server.enqueue_response(
                    "/v1/messages",
                    messages_turn(
                        &[text_block("The new user task sees the consultation.")],
                        END_TURN,
                    ),
                );
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    lifecycle_user(&commands, "explicit-after-cancel"),
                )
                .await
                .unwrap()
                .unwrap()
                .unwrap();
                assert_eq!(server.requests().len(), 3);
                let wire = server.request_bodies().last().unwrap().to_string();
                assert!(wire.contains("What must survive this lifecycle control?"));
                assert!(wire.contains("A completed consultation remains Session evidence."));
                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()
                        .is_empty()
                );
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_one_lifecycle_consumption(&events, &receipt);
                assert!(events.iter().any(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { prompt_id, identity, input_ids, .. })
                        if prompt_id == "explicit-after-cancel" && identity.origin == "user" && !input_ids.is_empty())));

                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Turn(
                                chat_state::TurnEvent::Started { .. }
                            )
                        ))
                        .count(),
                    2
                );
                commands.send(SessionCommand::Shutdown).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(10), loop_task)
                    .await
                    .unwrap()
                    .unwrap();
            }));
    });
}

#[test]
fn suspended_consultation_stays_pending_through_replacement_prepare_commit_and_release() {
    run_with_session_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tokio::task::LocalSet::new().run_until(async {
                use chat_state::session_runtime::RuntimePhase;
                let server = MockInferenceServer::start().await.unwrap();
                let (original, events) = lifecycle_actor(&server, None).await;
                super::super::super::support::record_test_model_route(&original).await;
                let (commands, old_loop) = start_lifecycle_mailbox(original.clone(), events).await;
                let receipt =
                    lifecycle_answer(&original, &commands, &server, "idle-before-suspend").await;
                let operation_id = "consultation-suspension".to_string();
                let (respond_to, response) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::SuspendRuntimeNode {
                        operation_id: operation_id.clone(),
                        root_session_id: original.session_id_string(),
                        deadline: Some(tokio::time::Instant::now() + std::time::Duration::from_secs(10)),
                        respond_to,
                    })
                    .unwrap();
                let node = tokio::time::timeout(std::time::Duration::from_secs(12), response)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(node.session_id, original.session_id_string());
                assert_eq!(
                    original
                        .chat_state_handle
                        .runtime_state()
                        .await
                        .unwrap()
                        .phase,
                    RuntimePhase::Suspended
                );
                let frontier = original
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .len();
                original.reconcile_coordination_answers().await.unwrap();
                assert_eq!(
                    original
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    frontier
                );
                assert_eq!(
                    original
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()[0]
                        .id,
                    receipt
                );
                let (respond_to, retired) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::RetireSuspendedRuntime {
                        operation_id: operation_id.clone(),
                        deadline: Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5)),
                        respond_to,
                    })
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(6), retired)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), old_loop)
                    .await
                    .unwrap()
                    .unwrap();
                let (restored, events) = lifecycle_actor(&server, Some(&original)).await;
                let (commands, restored_loop) = start_lifecycle_mailbox(restored.clone(), events).await;
                let (respond_to, prepared) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::PrepareRuntimeRestore {
                        operation_id: operation_id.clone(),
                        instance_id: "replacement-instance".into(),
                        respond_to,
                    })
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), prepared)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let (respond_to, committed) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::CommitRuntimeResume {
                        operation_id: operation_id.clone(),
                        instance_id: "replacement-instance".into(),
                        respond_to,
                    })
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), committed)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(!restored.state.lock().await.execution_open());
                assert_eq!(server.requests().len(), 1);
                assert_eq!(
                    restored
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()[0]
                        .id,
                    receipt
                );
                let (respond_to, released) = tokio::sync::oneshot::channel();
                commands
                    .send(SessionCommand::ReleaseRuntimeRestore {
                        instance_id: "replacement-instance".into(),
                        respond_to,
                    })
                    .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), released)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(restored.state.lock().await.execution_open());
                assert!(restored.state.lock().await.foreground.is_idle());
                assert_eq!(
                    server.requests().len(),
                    1,
                    "restoration does not wake a receipt-only Session"
                );
                assert_eq!(
                    restored
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()[0]
                        .id,
                    receipt
                );
                server.enqueue_response(
                    "/v1/messages",
                    messages_turn(
                        &[text_block("The restored user task sees pending evidence.")],
                        END_TURN,
                    ),
                );
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    lifecycle_user(&commands, "user-after-restore"),
                )
                .await
                .unwrap()
                .unwrap()
                .unwrap();
                assert_eq!(server.requests().len(), 2);
                assert!(
                    server
                        .request_bodies()
                        .last()
                        .unwrap()
                        .to_string()
                        .contains("A completed consultation remains Session evidence.")
                );
                assert!(
                    restored
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()
                        .is_empty()
                );
                assert_one_lifecycle_consumption(
                    &restored.chat_state_handle.timeline_events().await.unwrap(),
                    &receipt,
                );
                assert!(restored.chat_state_handle.timeline_events().await.unwrap().iter().any(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { prompt_id, identity, input_ids, .. })
                        if prompt_id == "user-after-restore" && identity.origin == "user" && !input_ids.is_empty())));

                assert!(
                    original
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()
                        .iter()
                        .any(|item| item.id == receipt),
                    "replacement consumes its reconstructed writer state, not the retired handle"
                );
                commands.send(SessionCommand::Shutdown).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(10), restored_loop)
                    .await
                    .unwrap()
                    .unwrap();
            }));
    });
}
