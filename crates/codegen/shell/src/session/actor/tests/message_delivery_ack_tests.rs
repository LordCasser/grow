//! An ambiguous durable consumption ACK must not duplicate context or dispatch early.
use super::*;

struct HoldFirstNotificationAck {
    received: bool,
    stage: Option<tokio::sync::oneshot::Sender<tokio::sync::oneshot::Sender<std::io::Result<()>>>>,
    writes: Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
}
impl chat_state::TimelinePersistence for HoldFirstNotificationAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (ack, receive) = tokio::sync::oneshot::channel();
        self.writes.lock().unwrap().push(event.clone());
        let selected = matches!(
            (&event.kind, self.received),
            (
                chat_state::TimelineEventKind::Notification(
                    chat_state::NotificationEvent::Received { .. }
                ),
                true
            ) | (
                chat_state::TimelineEventKind::Notification(
                    chat_state::NotificationEvent::Consumed { .. }
                ),
                false
            )
        );
        if selected && let Some(stage) = self.stage.take() {
            stage.send(ack).unwrap();
        } else {
            let _ = ack.send(Ok(()));
        }
        receive
    }
    fn flush(&mut self) {}
}

async fn actor_with_notification_ack(
    server: &MockInferenceServer,
    received: bool,
) -> (
    Arc<SessionActor>,
    tokio::sync::oneshot::Receiver<tokio::sync::oneshot::Sender<std::io::Result<()>>>,
    Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            if let PersistenceMsg::SidebandDurablyAndAck { respond_to, .. } = message {
                let _ = respond_to.send(Ok(()));
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
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let history = actor.chat_state_handle.timeline_events().await.unwrap();
    let writes = Arc::new(std::sync::Mutex::new(history.clone()));
    let (stage, staged) = tokio::sync::oneshot::channel();
    let (event_tx, _) = mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
        history,
        config.clone(),
        Box::new(HoldFirstNotificationAck {
            received,
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
        &server,
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

#[test]
fn lost_consumed_ack_retries_same_fact_before_provider_and_replay_keeps_one_input() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, staged, writes) = actor_with_notification_ack(&server, false).await;
            let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            communication_agent(&actor).await;
            let answer = answer_live_consultation(&actor, &server, "ambiguous-consumption", "What evidence survives an unconfirmed consumption ACK?", "ONE_DURABLE_CONSULTATION_ANSWER").await;
            assert_eq!(answer.status, crate::coordination::InquiryStatus::Answered);
            let receipt = actor.chat_state_handle.pending_notifications().await.unwrap()[0].id.clone();
            server.enqueue_response("/v1/messages", messages_turn(&[text_block("One request sees the durable consultation.")], END_TURN));
            let (result, recovered) = tokio::join!(run_user_turn(&actor, "after-ambiguous-consumption"), async {
                let lost_ack = tokio::time::timeout(std::time::Duration::from_secs(10), staged).await.unwrap().unwrap();
                assert_eq!(server.requests().len(), 1, "unconfirmed consumption cannot admit the dependent provider");
                let persisted = writes.lock().unwrap().clone();
                let consumed = persisted.last().unwrap();
                assert!(matches!(&consumed.kind, chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, input: Some(_), .. }) if notification_ids == &[receipt.clone()]));
                // Crash-equivalent canonical replay is independent of the live ACK.
                let (events, _) = mpsc::unbounded_channel();
                let restored = chat_state::ChatStateActor::spawn_from_timeline(persisted, config,
                    Box::new(chat_state::NullTimelinePersistence), events, Default::default()).await.unwrap();
                assert!(restored.pending_notifications().await.unwrap().is_empty());
                let items = restored.get_conversation().await;
                assert_eq!(serde_json::to_string(&items).unwrap().matches("ONE_DURABLE_CONSULTATION_ANSWER").count(), 1);
                drop(lost_ack); // The original writer retries the same fact, not a new receipt/input.
                restored
            });
            result.unwrap();
            assert!(recovered.pending_notifications().await.unwrap().is_empty());
            assert_eq!(server.requests().len(), 2);
            assert_eq!(server.request_bodies()[1].to_string().matches("ONE_DURABLE_CONSULTATION_ANSWER").count(), 1);
            let attempts = writes.lock().unwrap().iter().filter(|event| matches!(event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { .. }))).cloned().collect::<Vec<_>>();
            assert_eq!(attempts.len(), 2);
            assert_eq!(serde_json::to_value(&attempts[0]).unwrap(), serde_json::to_value(&attempts[1]).unwrap());
            assert!(actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(timeline.iter().filter(|event| matches!(event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { .. }))).count(), 1);
        }));
    });
}

#[test]
fn lost_received_ack_retries_same_receipt_without_rerunning_answer_or_waking_idle() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, staged, writes) = actor_with_notification_ack(&server, true).await;
            let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            communication_agent(&actor).await;
            server.enqueue_response("/v1/messages", messages_turn(&[text_block("The original foreground Turn is fully ended.")], END_TURN));
            let completed = run_user_turn(&actor, "fully-closed-before-receipt").await;
            assert!(completed.is_ok(), "{completed:?}");
            actor.handle_completion("fully-closed-before-receipt".into(), completed).await;
            let before = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(before.iter().filter(|event| matches!(event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. }))).count(), 1);

            let (answer, ()) = tokio::join!(
                answer_live_consultation(
                    &actor,
                    &server,
                    "ambiguous-receipt",
                    "Can an unconfirmed Received ACK duplicate this answer?",
                    "ONE_RECEIVED_ACK_ANSWER"
                ),
                async {
                    let lost_ack = tokio::time::timeout(std::time::Duration::from_secs(10), staged)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(server.requests().len(), 2);
                    assert!(actor.state.lock().await.foreground.is_idle());
                    let persisted = writes.lock().unwrap().clone();
                    assert!(matches!(
                        &persisted.last().unwrap().kind,
                        chat_state::TimelineEventKind::Notification(
                            chat_state::NotificationEvent::Received {
                                source: chat_state::NotificationSource::InquiryAnswered { .. },
                                ..
                            }
                        )
                    ));
                    // Canonical receipt replay recovers pending evidence, not a new Turn.
                    let (events, _) = mpsc::unbounded_channel();
                    let restored = chat_state::ChatStateActor::spawn_from_timeline(
                        persisted,
                        config,
                        Box::new(chat_state::NullTimelinePersistence),
                        events,
                        Default::default(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(restored.pending_notifications().await.unwrap().len(), 1);
                    assert!(
                        !serde_json::to_string(&restored.get_conversation().await)
                            .unwrap()
                            .contains("ONE_RECEIVED_ACK_ANSWER")
                    );
                    drop(lost_ack);
                }
            );
            assert_eq!(answer.status, crate::coordination::InquiryStatus::Answered);
            assert_eq!(server.requests().len(), 2);
            assert!(actor.state.lock().await.foreground.is_idle());
            actor.reconcile_coordination_answers().await.unwrap();
            let still_closed = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(still_closed.iter().filter(|event| matches!(event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. }))).count(), 1,
                "a receipt admitted after final close cannot append input to the ended Turn or open a Turn");

            let attempts = writes
                .lock()
                .unwrap()
                .iter()
                .filter(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Notification(
                            chat_state::NotificationEvent::Received {
                                source: chat_state::NotificationSource::InquiryAnswered { .. },
                                ..
                            }
                        )
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(attempts.len(), 2);
            assert_eq!(
                serde_json::to_value(&attempts[0]).unwrap(),
                serde_json::to_value(&attempts[1]).unwrap()
            );
            assert_eq!(
                actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .len(),
                1
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[text_block(
                        "The explicit user Turn consumes one consultation.",
                    )],
                    END_TURN,
                ),
            );
            run_user_turn(&actor, "after-ambiguous-received")
                .await
                .unwrap();
            assert_eq!(server.requests().len(), 3);
            assert_eq!(
                server.request_bodies()[2]
                    .to_string()
                    .matches("ONE_RECEIVED_ACK_ANSWER")
                    .count(),
                1
            );
            assert!(
                actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .is_empty()
            );
        }));
    });
}
