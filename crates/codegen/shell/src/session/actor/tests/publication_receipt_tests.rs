use super::super::tests::support::{
    begin_test_causal_turn, create_test_actor, create_test_actor_ex_with_physical_projection,
};
use super::*;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

type Receipt = oneshot::Receiver<Result<(), String>>;

fn notification() -> GrowSessionNotification {
    GrowSessionNotification {
        session_id: acp::SessionId::new("test-actor"),
        update: GrowSessionUpdate::MemoryFlushStarted,
        meta: None,
    }
}

fn publish(
    actor: Arc<SessionActor>,
) -> (
    tokio::task::JoinHandle<Result<(), chat_state::TimelineWriteError>>,
    Receipt,
) {
    publish_notification(actor, notification())
}

fn publish_notification(
    actor: Arc<SessionActor>,
    notification: GrowSessionNotification,
) -> (
    tokio::task::JoinHandle<Result<(), chat_state::TimelineWriteError>>,
    Receipt,
) {
    let (respond_to, receipt) = oneshot::channel();
    let handle = tokio::task::spawn_local(async move {
        actor
            .handle_grow_session_notification(notification, true, false, Some(respond_to))
            .await
    });
    (handle, receipt)
}

fn spawned_notification() -> GrowSessionNotification {
    GrowSessionNotification {
        session_id: acp::SessionId::new("test-actor"),
        update: GrowSessionUpdate::SubagentSpawned {
            subagent_id: "child-1".into(),
            parent_session_id: "test-actor".into(),
            parent_prompt_id: None,
            child_session_id: "child-1".into(),
            subagent_type: "explore".into(),
            description: "inspect code".into(),
            effective_context_source: None,
            context_normalized: false,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            model: None,
            reasoning_effort: None,
            model_state: None,
            workflow_agent_names: None,
            resumed_from: None,
            workflow_run_id: None,
            goal_id: None,
        },
        meta: None,
    }
}

async fn seed_spawn(actor: &SessionActor) {
    begin_test_causal_turn(actor).await;
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(chat_state::SubagentSpawnEvent {
                subagent_id: "child-1".into(),
                child_session_id: "child-1".into(),
                origin_tool_call_id: None,
                security_parent_session_id: actor.session_info.id.to_string(),
                parent_step_id: None,
                subagent_type: "explore".into(),
                description: "inspect code".into(),
                prompt: "inspect code".into(),
                execution_recipe: serde_json::json!({}),
                context_source: chat_state::SubagentContextSource::New,
                source_ref: None,
                context_normalized: false,
                resumed_from: None,
                parent_prompt_id: None,
                capability_mode: None,
                birth_mcp_servers: Some(Default::default()),
                permission_mode: None,
                effective_permission_mode: None,
                workflow_run_id: None,
                goal_id: None,
                goal_definition_revision: None,
                surface_completion: true,
                child_cwd: actor.session_info.cwd.clone(),
                worktree_path: None,
                effective_model_id: "test-model".into(),
                model_transport_key: sampling_types::ModelImageInputKey::new(
                    "test-model",
                    "chat_completions",
                    "test-endpoint",
                ),
                reasoning_effort: None,
            })),
        ))
        .await
        .unwrap();
}

async fn ack_persistence(
    rx: &mut mpsc::UnboundedReceiver<PersistenceMsg>,
    preview: bool,
    receipt: &mut Receipt,
) {
    let message = rx.recv().await.unwrap();
    assert!(matches!(
        receipt.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    match message {
        PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. } if !preview => {
            respond_to.send(Ok(())).unwrap();
        }
        PersistenceMsg::PreviewIndependent { respond_to, .. } if preview => {
            respond_to.send(Ok(())).unwrap();
        }
        other => panic!("expected the original durable owner, got {other:?}"),
    }
}

#[tokio::test]
async fn publication_receipt_waits_for_durable_ack_and_gateway_completion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            let (timeline_tx, mut timeline_rx) = super::acking_persistence_channel();
            let (mut actor, _) = create_test_actor_ex_with_physical_projection(
                0,
                256_000,
                85,
                gateway_tx,
                timeline_tx,
            )
            .await;
            seed_spawn(&actor).await;
            while timeline_rx.try_recv().is_ok() {}
            // Keep the real ChatState Hook owner, but hold Grow's durable ACK
            // directly instead of using the fixture's automatic ACK bridge.
            actor.notifications.persistence_tx = persistence_tx;
            let actor = Arc::new(actor);
            let (handling, mut receipt) = publish_notification(actor, spawned_notification());
            let PersistenceMsg::AppendUpdateDurablyAndAck {
                update: crate::session::storage::SessionUpdate::Grow(persisted),
                respond_to,
            } = persistence_rx.recv().await.unwrap()
            else {
                panic!("receipt publication must request durable append");
            };
            let hook_events = std::iter::from_fn(|| timeline_rx.try_recv().ok())
                .filter_map(|message| match message {
                    PersistenceMsg::TimelineDurablyAndAck { event, .. } => Some(event.kind),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(hook_events.iter().any(|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    event: chat_state::HookEventType::SubagentStart,
                    ..
                })
            )));
            assert!(hook_events.iter().any(|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed { .. })
            )));
            assert!(matches!(
                receipt.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert!(gateway_rx.try_recv().is_err());
            respond_to.send(Ok(())).unwrap();
            handling.await.unwrap().unwrap(); // The actor does not wait for gateway ACK.
            let acp_transport::AcpClientMessage::ExtNotification(live) =
                gateway_rx.recv().await.unwrap()
            else {
                panic!("expected original Grow delivery");
            };
            let wire: GrowSessionNotification =
                serde_json::from_str(live.request.params.get()).unwrap();
            assert_eq!(wire.meta, persisted.meta);
            assert!(wire.meta.as_ref().unwrap()["eventId"].as_str().is_some());
            assert!(matches!(
                receipt.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            live.response_tx.send(Ok(())).unwrap();
            assert_eq!(receipt.await.unwrap(), Ok(()));
            assert!(gateway_rx.try_recv().is_err());
            assert!(persistence_rx.try_recv().is_err());
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn publication_receipt_gateway_failures_retire_monitors_and_preview_credit() {
    tokio::task::LocalSet::new().run_until(async {
        for failure in ["budget", "gateway closed", "ACK dropped", "gateway error", "cancel", "deadline", "receiver closed"] {
            let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx.clone()).await;
            // Bypass the fixture's automatic durable-ACK bridge; these tests
            // control the production sender's real acknowledgement directly.
            actor.notifications.persistence_tx = persistence_tx;
            let actor = Arc::new(actor);
            *actor.sampling_preview.lock() = Some(("request".into(), 2, false));
            let budget = actor.notifications.preview_gateway_budget.clone();
            let held = if failure == "budget" { Some(budget.acquire(4096).await.unwrap()) } else { None };
            if failure == "gateway closed" { gateway_rx.close(); }
            let (handling, mut receipt) = publish(actor.clone());
            ack_persistence(&mut persistence_rx, true, &mut receipt).await;
            handling.await.unwrap().unwrap();
            let mut receipt = Some(receipt);
            let mut gateway_ack = None;
            if failure != "budget" && failure != "gateway closed" {
                let acp_transport::AcpClientMessage::ExtNotification(live) = gateway_rx.recv().await.unwrap() else {
                    panic!("expected Grow delivery");
                };
                gateway_ack = Some(live.response_tx);
            }
            match failure {
                "ACK dropped" => { drop(gateway_ack.take()); }
                "gateway error" => { gateway_ack.take().unwrap().send(Err(acp::Error::internal_error())).unwrap(); }
                "cancel" => actor.durable_ui_cancel.cancel(),
                "deadline" => tokio::time::advance(std::time::Duration::from_secs(30)).await,
                "receiver closed" => { drop(receipt.take()); }
                _ => {}
            }
            if let Some(receipt) = receipt {
                let error = receipt.await.unwrap().unwrap_err();
                assert!(error.starts_with("publication unconfirmed:"), "{failure}: {error}");
            }
            // A failure marker is also an explicit watchdog-exit handshake.
            assert!(matches!(persistence_rx.recv().await.unwrap(),
                PersistenceMsg::SamplingPreviewFailure { request_id, attempt: 2, .. } if request_id == "request"));
            if let Some(mut ack) = gateway_ack {
                ack.closed().await; // The completion monitor actually dropped its receiver.
                assert!(ack.send(Ok(())).is_err()); // Late success cannot resurrect the receipt.
            }
            drop(held);
            let all_credit = tokio::time::timeout(std::time::Duration::from_secs(1), budget.acquire(4096)).await
                .expect("bounded monitor must release all preview credit").unwrap();
            drop(all_credit);
        }
    }).await;
}

#[tokio::test]
async fn publication_receipt_persistence_failure_and_lost_ack_never_publish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for preview in [false, true] {
                for drop_ack in [false, true] {
                    let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel();
                    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
                    let mut actor =
                        create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx.clone()).await;
                    // Bypass the fixture's automatic durable-ACK bridge; these tests
                    // control the production sender's real acknowledgement directly.
                    actor.notifications.persistence_tx = persistence_tx;
                    let actor = Arc::new(actor);
                    if preview {
                        *actor.sampling_preview.lock() = Some(("request".into(), 2, false));
                    }
                    let (handling, receipt) = publish(actor);
                    let respond_to = match persistence_rx.recv().await.unwrap() {
                        PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. }
                            if !preview =>
                        {
                            respond_to
                        }
                        PersistenceMsg::PreviewIndependent { respond_to, .. } if preview => {
                            respond_to
                        }
                        other => panic!("unexpected persistence message: {other:?}"),
                    };
                    if drop_ack {
                        drop(respond_to);
                    } else {
                        respond_to
                            .send(Err(
                                crate::session::storage::AppendUpdateError::NotCommitted(
                                    std::io::Error::other("injected write failure"),
                                ),
                            ))
                            .unwrap();
                    }
                    assert!(handling.await.unwrap().is_err()); // Original Timeline fail-stop signal.
                    assert!(receipt.await.unwrap().is_err());
                    assert!(gateway_rx.try_recv().is_err());
                }
            }
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn publication_receipt_deadline_covers_inflight_persistence_without_claiming_rollback() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx.clone()).await;
            // Bypass the fixture's automatic durable-ACK bridge; these tests
            // control the production sender's real acknowledgement directly.
            actor.notifications.persistence_tx = persistence_tx;
            let actor = Arc::new(actor);
            let (handling, receipt) = publish(actor);
            let PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. } =
                persistence_rx.recv().await.unwrap()
            else {
                panic!("expected durable append");
            };
            tokio::time::advance(std::time::Duration::from_secs(30)).await;
            assert!(receipt.await.unwrap().unwrap_err().contains("deadline"));
            assert!(!handling.is_finished());
            respond_to.send(Ok(())).unwrap(); // Original owner still finishes its in-flight work.
            handling.await.unwrap().unwrap();
            let acp_transport::AcpClientMessage::ExtNotification(mut live) =
                gateway_rx.recv().await.unwrap()
            else {
                panic!("expected original Grow delivery");
            };
            live.response_tx.closed().await;
            assert!(live.response_tx.send(Ok(())).is_err());
        })
        .await;
}
