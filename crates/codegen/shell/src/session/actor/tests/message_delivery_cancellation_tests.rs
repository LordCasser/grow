//! Inquiry cancellation competes with success admission, not provider completion.
use super::*;

struct HeldSuccessAck {
    stage: Option<tokio::sync::oneshot::Sender<tokio::sync::oneshot::Sender<std::io::Result<()>>>>,
    writes: Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
}

impl chat_state::TimelinePersistence for HeldSuccessAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (ack, receiver) = tokio::sync::oneshot::channel();
        self.writes.lock().unwrap().push(event.clone());
        if matches!(crate::coordination::InquiryEvent::from_timeline(event).unwrap(),
            Some(crate::coordination::InquiryEvent::Incoming { audit, .. }) if audit.answer_context.is_some())
        {
            self.stage.take().unwrap().send(ack).unwrap();
        } else {
            let _ = ack.send(Ok(()));
        }
        receiver
    }
    fn flush(&mut self) {}
}

async fn actor_at_success_ack(
    server: &MockInferenceServer,
) -> (
    Arc<SessionActor>,
    tokio::sync::oneshot::Receiver<tokio::sync::oneshot::Sender<std::io::Result<()>>>,
    Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
    let (persistence_tx, mut observations) = mpsc::unbounded_channel();
    tokio::task::spawn_local(async move { while observations.recv().await.is_some() {} });
    let (mut actor, events) = create_test_actor_ex_with_projection_error(
        0,
        256_000,
        85,
        gateway_tx,
        persistence_tx,
        None,
    )
    .await;
    let history = actor.chat_state_handle.timeline_events().await.unwrap();
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let (event_tx, _) = mpsc::unbounded_channel();
    let (stage, staged) = tokio::sync::oneshot::channel();
    let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
        history,
        config,
        Box::new(HeldSuccessAck {
            stage: Some(stage),
            writes: writes.clone(),
        }),
        event_tx,
        Default::default(),
    )
    .await
    .unwrap();
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    let (actor, _, _) = configure_actor_with_sampler(
        server,
        sampling_types::ApiBackend::Messages,
        sampler::OutputDelivery::Irreversible,
        None,
        actor,
        gateway_rx,
        events,
    )
    .await;
    (actor, staged, writes)
}

pub(super) async fn actor_at_result_ack(
    server: &MockInferenceServer,
) -> (
    Arc<SessionActor>,
    tokio::sync::oneshot::Receiver<tokio::sync::oneshot::Sender<()>>,
    Arc<std::sync::Mutex<Vec<chat_state::SidebandEvent>>>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
    let (persistence_tx, mut observations) = mpsc::unbounded_channel();
    tokio::task::spawn_local(async move { while observations.recv().await.is_some() {} });
    let (mut actor, events) = create_test_actor_ex_with_projection_error(
        0,
        256_000,
        85,
        gateway_tx,
        persistence_tx,
        None,
    )
    .await;
    let original = actor.notifications.persistence_tx.clone();
    let (intercept, mut writes) = mpsc::unbounded_channel();
    let (stage_tx, stage_rx) = tokio::sync::oneshot::channel();
    let records = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = records.clone();
    actor.notifications.persistence_tx = intercept;
    tokio::task::spawn_local(async move {
        let mut stage = Some(stage_tx);
        while let Some(message) = writes.recv().await {
            if let PersistenceMsg::SidebandDurablyAndAck { event, .. } = &message {
                recorded.lock().unwrap().push(event.clone());
                if matches!(event.kind, chat_state::SidebandEventKind::Result(_)) {
                    let (release, wait) = tokio::sync::oneshot::channel();
                    stage.take().unwrap().send(release).unwrap();
                    wait.await.unwrap();
                }
            }
            if original.send(message).is_err() {
                break;
            }
        }
    });
    let (actor, _, _) = configure_actor_with_sampler(
        server,
        sampling_types::ApiBackend::Messages,
        sampler::OutputDelivery::Irreversible,
        None,
        actor,
        gateway_rx,
        events,
    )
    .await;
    (actor, stage_rx, records)
}

#[test]
fn inquiry_cancel_before_success_admission_during_result_ack_has_no_success_context() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let expected = server.expect_response("answer before held Result ACK",
                test_support::InferenceRequestMatcher::auxiliary(test_support::InferenceEndpoint::Messages),
                messages_turn(&[text_block("A completed provider answer is not yet admitted success.")], END_TURN));
            let (actor, staged, records) = actor_at_result_ack(&server).await;
            let (inquiry, response) = live_consultation(&actor, "cancel-before-terminal", "Can this answer still be cancelled?");
            let cancellation = inquiry.cancellation.clone();
            actor.enqueue_coordination_inquiry(inquiry).await;
            let release = tokio::time::timeout(std::time::Duration::from_secs(5), staged).await.unwrap().unwrap();
            assert!(expected.is_satisfied());
            cancellation.cancel(crate::coordination::InquiryCancellationReason::Explicit);
            release.send(()).unwrap();
            let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), response).await.unwrap().unwrap();
            assert_eq!(outcome.status, crate::coordination::InquiryStatus::Cancelled, "{outcome:?}");
            assert!(actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
            assert!(!actor.chat_state_handle.timeline_events().await.unwrap().iter().filter_map(|event|
                crate::coordination::InquiryEvent::from_timeline(event).unwrap()).any(|event| matches!(event,
                    crate::coordination::InquiryEvent::Incoming { audit, .. } if audit.answer_context.is_some())));
            assert!(matches!(&records.lock().unwrap().last().unwrap().kind,
                chat_state::SidebandEventKind::End(end) if end.outcome == chat_state::SidebandOutcome::Completed));
            assert_eq!(server.requests().len(), 1);
        }));
    });
}

#[test]
fn success_admission_wins_late_cancel_but_only_durable_success_creates_context() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for fail_ack in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let expected = server.expect_response(
                    "answer admitted before held Timeline ACK",
                    test_support::InferenceRequestMatcher::auxiliary(
                        test_support::InferenceEndpoint::Messages,
                    ),
                    messages_turn(
                        &[text_block(
                            "Success admission still needs its durable acknowledgement.",
                        )],
                        END_TURN,
                    ),
                );
                let (actor, staged, writes) = actor_at_success_ack(&server).await;
                let (inquiry, response) =
                    live_consultation(&actor, "cancel-after-admission", "Which control won?");
                let cancellation = inquiry.cancellation.clone();
                actor.enqueue_coordination_inquiry(inquiry).await;
                let release = tokio::time::timeout(std::time::Duration::from_secs(5), staged)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(expected.is_satisfied());
                assert!(
                    !cancellation.cancel(crate::coordination::InquiryCancellationReason::Explicit)
                );
                assert!(!cancellation.is_cancelled());
                if fail_ack {
                    release
                        .send(Err(std::io::Error::new(
                            std::io::ErrorKind::StorageFull,
                            "injected canonical success ACK failure",
                        )))
                        .unwrap();
                } else {
                    release.send(Ok(())).unwrap();
                }
                let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), response)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    outcome.status,
                    if fail_ack {
                        crate::coordination::InquiryStatus::Failed
                    } else {
                        crate::coordination::InquiryStatus::Answered
                    },
                    "{outcome:?}"
                );
                if fail_ack {
                    assert_eq!(
                        writes
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|event| matches!(
                                event.kind,
                                chat_state::TimelineEventKind::Notification(
                                    chat_state::NotificationEvent::Received { .. }
                                )
                            ))
                            .count(),
                        0
                    );
                    assert_eq!(
                        outcome.error.unwrap().code,
                        crate::coordination::CoordinationErrorCode::AuditFailure
                    );
                    let failed = actor.chat_state_handle.pending_timeline_commit().unwrap();
                    assert_eq!(failed.error_kind, Some(std::io::ErrorKind::StorageFull));
                    assert!(
                        failed
                            .error_message
                            .unwrap()
                            .contains("injected canonical success ACK failure")
                    );
                    assert!(
                        actor
                            .chat_state_handle
                            .pending_notifications()
                            .await
                            .is_none(),
                        "failed writer must be unavailable before any receipt can commit"
                    );
                } else {
                    assert_eq!(
                        actor
                            .chat_state_handle
                            .pending_notifications()
                            .await
                            .unwrap()
                            .len(),
                        1
                    );
                }
                assert_eq!(server.requests().len(), 1);
            }
        }));
    });
}

#[test]
fn peer_cancel_intent_keeps_original_stream_and_respects_target_admission() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for success_already_admitted in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let _expected = server.expect_response(
                    "real peer cancellation race",
                    test_support::InferenceRequestMatcher::auxiliary(
                        test_support::InferenceEndpoint::Messages,
                    ),
                    messages_turn(
                        &[text_block("The original target response is authoritative.")],
                        END_TURN,
                    ),
                );
                let (actor, before_success, after_success) = if success_already_admitted {
                    let (actor, staged, _) = actor_at_success_ack(&server).await;
                    (actor, None, Some(staged))
                } else {
                    let (actor, staged, _) = actor_at_result_ack(&server).await;
                    (actor, Some(staged), None)
                };
                let home = tempfile::tempdir().unwrap();
                let source = crate::coordination::CoordinationRuntime::new(home.path().to_owned());
                let target = crate::coordination::CoordinationRuntime::new(home.path().to_owned());
                source.ensure_started().await.unwrap();
                target.ensure_started().await.unwrap();
                let snapshot = |id: String| crate::coordination::LocalSessionSnapshot {
                    session_id: id,
                    canonical_cwd: actor.session_info.cwd.clone(),
                    main_agent: "test".into(),
                    activity: crate::agent::roster::RosterActivity::Idle,
                    subagents: Default::default(),
                };
                source.publish_sessions(vec![snapshot("source-canceller".into())]);
                target.publish_sessions(vec![snapshot(actor.session_id_string())]);
                let mut incoming = target.take_inquiry_receiver().unwrap();
                let receiving = actor.clone();
                let (cancel_tx, mut cancellations) = mpsc::unbounded_channel();
                tokio::task::spawn_local(async move {
                    while let Some(inquiry) = incoming.recv().await {
                        cancel_tx.send(inquiry.cancellation.clone()).unwrap();
                        receiving.enqueue_coordination_inquiry(inquiry).await;
                    }
                });
                let id = uuid::Uuid::now_v7().to_string();
                let caller = source.handle();
                let query_id = id.clone();
                let target_id = actor.session_id_string();
                let ask = tokio::spawn(async move {
                    caller
                        .ask_session(
                            &query_id,
                            "source-canceller",
                            &target_id,
                            "Did target cancellation or success admission win?",
                            None,
                            Default::default(),
                        )
                        .await
                });
                let target_cancel = cancellations.recv().await.unwrap();
                let release_before = if let Some(staged) = before_success {
                    Some(
                        tokio::time::timeout(std::time::Duration::from_secs(5), staged)
                            .await
                            .unwrap()
                            .unwrap(),
                    )
                } else {
                    None
                };
                let release_after = if let Some(staged) = after_success {
                    Some(
                        tokio::time::timeout(std::time::Duration::from_secs(5), staged)
                            .await
                            .unwrap()
                            .unwrap(),
                    )
                } else {
                    None
                };
                assert!(
                    source
                        .handle()
                        .cancel_session(&id, "source-canceller", &actor.session_id_string())
                        .await
                        .unwrap(),
                    "local bool accepts intent, not target outcome"
                );
                if let Some(release) = release_before {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        target_cancel.cancelled(),
                    )
                    .await
                    .unwrap();
                    release.send(()).unwrap();
                }
                if let Some(release) = release_after {
                    assert!(
                        !target_cancel
                            .cancel(crate::coordination::InquiryCancellationReason::Explicit)
                    );
                    release.send(Ok(())).unwrap();
                }
                let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), ask)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    outcome.status,
                    if success_already_admitted {
                        crate::coordination::InquiryStatus::Answered
                    } else {
                        crate::coordination::InquiryStatus::Cancelled
                    },
                    "{outcome:?}"
                );
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while actor.coordination_inquiry_active.get() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()
                        .len(),
                    usize::from(success_already_admitted)
                );
                assert_eq!(
                    server.requests().len(),
                    1,
                    "cancel/reconnect never reruns the Sideband"
                );
            }
        }));
    });
}
