use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::{
    begin_test_active_causal_turn, build_actor, install_test_foreground,
};
use crate::session::notification_inbox::AgentMessageDeliveryError;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

type DurableEvents = Arc<Mutex<Vec<chat_state::TimelineEvent>>>;

struct PendingWrite {
    event: chat_state::TimelineEvent,
    acknowledgement: tokio::sync::oneshot::Sender<std::io::Result<()>>,
}

struct ControlPersistence {
    durable: DurableEvents,
    writes: mpsc::UnboundedSender<PendingWrite>,
    manual: Arc<AtomicBool>,
}

fn retain(durable: &DurableEvents, event: &chat_state::TimelineEvent) {
    let mut events = durable.lock().unwrap();
    if let Some(existing) = events.iter().find(|existing| existing.seq == event.seq) {
        assert_eq!(
            serde_json::to_value(existing).unwrap(),
            serde_json::to_value(event).unwrap()
        );
    } else {
        events.push(event.clone());
    }
}

impl chat_state::TimelinePersistence for ControlPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (acknowledgement, receiver) = tokio::sync::oneshot::channel();
        let controlled = matches!(event.kind, chat_state::TimelineEventKind::Planning(_))
            || matches!(
                &event.kind,
                chat_state::TimelineEventKind::Notification(
                    chat_state::NotificationEvent::Received {
                        source: chat_state::NotificationSource::PlanGuidance { .. },
                        ..
                    }
                )
            );
        if controlled && self.manual.load(Ordering::Acquire) {
            let _ = self.writes.send(PendingWrite {
                event: event.clone(),
                acknowledgement,
            });
        } else {
            retain(&self.durable, event);
            let _ = acknowledgement.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {}
}

struct Fixture {
    actor: Arc<SessionActor>,
    durable: DurableEvents,
    writes: mpsc::UnboundedReceiver<PendingWrite>,
    write_tx: mpsc::UnboundedSender<PendingWrite>,
    manual: Arc<AtomicBool>,
    sampling: sampling_types::SamplingConfig,
    shutdown: tokio_util::sync::CancellationToken,
}

impl Fixture {
    async fn new() -> Self {
        let (mut actor, _gateway) = build_actor().await;
        let target = Arc::get_mut(&mut actor).unwrap();
        target.startup_hints.parent_session_id = Some("lifecycle-root-decoy".into());
        target.startup_hints.is_subagent = true;
        target.startup_hints.delegated_parent_plan = Some(ParentPlanBinding {
            parent_session_id: "direct-parent".into(),
            task_id: "task-1".into(),
            parent_step_id: "step-1".into(),
        });
        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        let sampling = actor.chat_state_handle.get_sampling_config().await.unwrap();
        let durable = Arc::new(Mutex::new(events.clone()));
        let (write_tx, writes) = mpsc::unbounded_channel();
        let manual = Arc::new(AtomicBool::new(false));
        let shutdown = tokio_util::sync::CancellationToken::new();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let handle = chat_state::ChatStateActor::spawn_from_timeline(
            events,
            sampling.clone(),
            Box::new(ControlPersistence {
                durable: durable.clone(),
                writes: write_tx.clone(),
                manual: manual.clone(),
            }),
            event_tx,
            shutdown.clone(),
        )
        .await
        .unwrap();
        let target = Arc::get_mut(&mut actor).unwrap();
        target.chat_state_handle = handle;
        target.events = crate::session::events::EventTracker::new(target.chat_state_handle.clone());
        install_test_foreground(&actor, "control-turn").await;
        manual.store(true, Ordering::Release);
        Self {
            actor,
            durable,
            writes,
            write_tx,
            manual,
            sampling,
            shutdown,
        }
    }

    async fn next_write(&mut self) -> PendingWrite {
        tokio::time::timeout(Duration::from_secs(2), self.writes.recv())
            .await
            .unwrap()
            .unwrap()
    }

    fn acknowledge(&self, write: PendingWrite) {
        retain(&self.durable, &write.event);
        write.acknowledgement.send(Ok(())).unwrap();
    }

    fn events(&self) -> Vec<chat_state::TimelineEvent> {
        self.durable.lock().unwrap().clone()
    }

    fn auto_ack(&self) {
        self.manual.store(false, Ordering::Release);
    }

    async fn cold_restore(&mut self) {
        self.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !self.actor.chat_state_handle.is_closed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        self.auto_ack();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let handle = chat_state::ChatStateActor::spawn_from_timeline(
            self.events(),
            self.sampling.clone(),
            Box::new(ControlPersistence {
                durable: self.durable.clone(),
                writes: self.write_tx.clone(),
                manual: self.manual.clone(),
            }),
            event_tx,
            shutdown.clone(),
        )
        .await
        .expect("the durable prefix must cold-validate");
        let actor = Arc::get_mut(&mut self.actor).expect("old delivery task released its owner");
        actor.chat_state_handle = handle;
        actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
        self.shutdown = shutdown;
        self.manual.store(true, Ordering::Release);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

fn control(actor: &SessionActor, stop: bool) -> tool_types::ReplanChange {
    tool_types::ReplanChange {
        id: "task-1:step-1:7".into(),
        link_id: "task-1".into(),
        child_session_id: actor.session_id_string(),
        step_id: "step-1".into(),
        source_revision: 7,
        content: "Full revised requirement 界🙂\"\\".repeat(1000),
        requirements: Some(vec![
            "Complete the original exact requirement 界🙂\"\\".repeat(1000),
        ]),
        priority: PlanPriority::High,
        status: if stop {
            PlanStepStatus::Cancelled
        } else {
            PlanStepStatus::InProgress
        },
        removed: stop,
        progress: ReplanProgress::Dispatching,
    }
}

fn launch(
    actor: &Arc<SessionActor>,
    change: tool_types::ReplanChange,
) -> tokio::task::JoinHandle<Result<String, AgentMessageDeliveryError>> {
    let actor = actor.clone();
    tokio::task::spawn_local(async move {
        actor
            .receive_plan_guidance("direct-parent".into(), change)
            .await
    })
}

async fn delivery_result(
    task: tokio::task::JoinHandle<Result<String, AgentMessageDeliveryError>>,
) -> Result<String, AgentMessageDeliveryError> {
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
}

fn assert_responsibility(event: &chat_state::TimelineEvent, expected: &tool_types::ReplanChange) {
    let chat_state::TimelineEventKind::Planning(planning) = &event.kind else {
        panic!("the first durable fact must be Planning, before notification admission")
    };
    let mut expected = expected.clone();
    expected.progress = ReplanProgress::Received;
    assert_eq!(planning.plan.replans, [expected]);
    planning.plan.validate().unwrap();
}

fn assert_no_release(
    actor: &SessionActor,
    task: &tokio::task::JoinHandle<Result<String, AgentMessageDeliveryError>>,
) {
    assert!(!task.is_finished(), "delivery must await both durable ACKs");
    assert!(
        !*actor.parent_message_interrupt.borrow(),
        "interruption must follow both durable ACKs"
    );
    assert!(
        actor.step_control_gate.try_lock().is_err(),
        "control admission owns the existing step gate"
    );
}

fn assert_typed_receipt(
    actor: &SessionActor,
    event: &chat_state::TimelineEvent,
    change: &tool_types::ReplanChange,
) -> String {
    let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
        id,
        source,
        source_version,
        owner_session_id,
        payload_ref,
    }) = &event.kind
    else {
        panic!("the second durable fact must be a notification receipt")
    };
    assert_eq!(owner_session_id, &actor.session_id_string());
    assert_eq!(
        source,
        &chat_state::NotificationSource::PlanGuidance {
            parent_session_id: "direct-parent".into(),
            message_id: replan_message_id(&change.id),
        }
    );
    assert_eq!(
        source_version,
        &chat_state::NotificationSourceVersion::Ordinal {
            value: chat_state::PARENT_MESSAGE_SOURCE_VERSION
        }
    );
    let body =
        crate::session::notification_inbox::read_payload(&actor.session_directory, payload_ref)
            .unwrap();
    assert!(body.len() <= 16 * 1024);
    assert_eq!(
        body,
        control::guidance_body("direct-parent", change).unwrap()
    );
    id.clone()
}

#[tokio::test(flavor = "current_thread")]
async fn replan_and_stop_wait_for_plan_then_typed_receipt_ack_before_signal_or_response() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for stop in [false, true] {
                let mut fixture = Fixture::new().await;
                let change = control(&fixture.actor, stop);
                let delivery = launch(&fixture.actor, change.clone());
                let planning = fixture.next_write().await;
                assert_responsibility(&planning.event, &change);
                assert_no_release(&fixture.actor, &delivery);
                assert!(
                    chat_state::Timeline::from_events(fixture.events())
                        .unwrap()
                        .planning_snapshot()
                        .is_none()
                );
                fixture.acknowledge(planning);
                let receipt = fixture.next_write().await;
                let id = assert_typed_receipt(&fixture.actor, &receipt.event, &change);
                assert_no_release(&fixture.actor, &delivery);
                let prefix = chat_state::Timeline::from_events(fixture.events()).unwrap();
                assert_eq!(
                    prefix.planning_snapshot().unwrap().plan.replans[0].content,
                    change.content
                );
                assert!(prefix.parent_message_receipts().is_empty());
                let bridge = fixture.actor.agent.borrow().tool_bridge().clone();
                let projection = bridge.read_resource::<LocalPlanProjection>().await.unwrap();
                assert_eq!(
                    projection.0.as_ref().unwrap().replans[0].requirements,
                    change.requirements
                );
                fixture.acknowledge(receipt);
                assert_eq!(delivery_result(delivery).await.unwrap(), id);
                assert!(*fixture.actor.parent_message_interrupt.borrow());
                assert!(!fixture.actor.background_service_shutdown.is_cancelled());
                let events = fixture
                    .actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap();
                let last = &events[events.len() - 2..];
                assert_responsibility(&last[0], &change);
                assert_typed_receipt(&fixture.actor, &last[1], &change);
                assert!(last[0].seq < last[1].seq);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_plan_commit_cannot_create_a_receipt_blob_or_interrupt() {
    tokio::task::LocalSet::new().run_until(async {
        let mut fixture = Fixture::new().await;
        let before = fixture.events();
        let change = control(&fixture.actor, false);
        let delivery = launch(&fixture.actor, change.clone());
        let planning = fixture.next_write().await;
        assert_responsibility(&planning.event, &change);
        planning.acknowledgement.send(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "injected planning failure"))).unwrap();
        assert!(matches!(delivery_result(delivery).await, Err(AgentMessageDeliveryError::Unconfirmed { code, .. }) if code == "planning_unconfirmed"));
        assert_eq!(serde_json::to_value(fixture.events()).unwrap(), serde_json::to_value(before).unwrap());
        assert!(fixture.writes.try_recv().is_err());
        assert!(!fixture.actor.session_dir.join("artifacts/notifications").exists());
        assert!(!*fixture.actor.parent_message_interrupt.borrow());
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_between_commits_cold_retry_fills_only_the_missing_receipt() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            let change = control(&fixture.actor, true);
            let actor = fixture.actor.clone();
            let artifact_gate = actor.notification_artifact_gate.lock().await;
            let delivery = launch(&fixture.actor, change.clone());
            let planning = fixture.next_write().await;
            assert_responsibility(&planning.event, &change);
            fixture.acknowledge(planning);
            let snapshot = tokio::time::timeout(
                Duration::from_secs(1),
                fixture.actor.chat_state_handle.planning_state(),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(snapshot.plan.replans[0].requirements, change.requirements);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), fixture.writes.recv())
                    .await
                    .is_err()
            );
            assert_no_release(&fixture.actor, &delivery);
            delivery.abort();
            assert!(delivery.await.unwrap_err().is_cancelled());
            drop(artifact_gate);
            drop(actor);
            let prefix = fixture.events();
            let timeline = chat_state::Timeline::from_events(prefix.clone()).unwrap();
            assert_eq!(
                timeline.planning_snapshot().unwrap().plan.replans[0].progress,
                ReplanProgress::Received
            );
            assert!(timeline.parent_message_receipts().is_empty());
            fixture.cold_restore().await;
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
            let retry = launch(&fixture.actor, change.clone());
            let receipt = fixture.next_write().await;
            let id = assert_typed_receipt(&fixture.actor, &receipt.event, &change);
            assert_no_release(&fixture.actor, &retry);
            fixture.acknowledge(receipt);
            assert_eq!(delivery_result(retry).await.unwrap(), id);
            let events = fixture.events();
            assert_eq!(events.len(), prefix.len() + 1);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Planning(_)
                    ))
                    .count(),
                1
            );
            assert_eq!(
                chat_state::Timeline::from_events(events)
                    .unwrap()
                    .parent_message_receipts()
                    .len(),
                1
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_receipt_commit_preserves_received_prefix_for_a_live_cold_owner_retry() {
    tokio::task::LocalSet::new().run_until(async {
        let mut fixture = Fixture::new().await;
        let change = control(&fixture.actor, false);
        let delivery = launch(&fixture.actor, change.clone());
        let planning = fixture.next_write().await;
        assert_responsibility(&planning.event, &change);
        fixture.acknowledge(planning);
        let receipt = fixture.next_write().await;
        let expected_id = assert_typed_receipt(&fixture.actor, &receipt.event, &change);
        receipt.acknowledgement.send(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "injected receipt failure"))).unwrap();
        assert!(matches!(delivery_result(delivery).await, Err(AgentMessageDeliveryError::Unconfirmed { code, .. }) if code == "receipt_commit_unknown"));
        assert!(!*fixture.actor.parent_message_interrupt.borrow());
        let prefix = fixture.events();
        let timeline = chat_state::Timeline::from_events(prefix.clone()).unwrap();
        assert_eq!(timeline.planning_snapshot().unwrap().plan.replans[0].content, change.content);
        assert!(timeline.parent_message_receipts().is_empty());
        fixture.cold_restore().await;
        let retry = launch(&fixture.actor, change.clone());
        let receipt = fixture.next_write().await;
        assert_eq!(assert_typed_receipt(&fixture.actor, &receipt.event, &change), expected_id);
        fixture.acknowledge(receipt);
        assert_eq!(delivery_result(retry).await.unwrap(), expected_id);
        assert_eq!(fixture.events().len(), prefix.len() + 1);
        assert_eq!(fixture.actor.chat_state_handle.planning_state().await.unwrap().unwrap().plan.replans.len(), 1);
        assert_eq!(fixture.actor.chat_state_handle.parent_message_receipts().await.unwrap().len(), 1);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn lost_or_failed_receipt_ack_retries_the_exact_fact_before_signalling() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            let change = control(&fixture.actor, false);
            let delivery = launch(&fixture.actor, change.clone());
            let planning = fixture.next_write().await;
            fixture.acknowledge(planning);
            let first = fixture.next_write().await;
            let id = assert_typed_receipt(&fixture.actor, &first.event, &change);
            let exact = serde_json::to_value(&first.event).unwrap();
            retain(&fixture.durable, &first.event);
            drop(first.acknowledgement);
            let retry = fixture.next_write().await;
            assert_eq!(serde_json::to_value(&retry.event).unwrap(), exact);
            assert_no_release(&fixture.actor, &delivery);
            retry
                .acknowledgement
                .send(Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "transient ACK failure",
                )))
                .unwrap();
            let final_retry = fixture.next_write().await;
            assert_eq!(serde_json::to_value(&final_retry.event).unwrap(), exact);
            assert_no_release(&fixture.actor, &delivery);
            fixture.acknowledge(final_retry);
            assert_eq!(delivery_result(delivery).await.unwrap(), id);
            assert!(*fixture.actor.parent_message_interrupt.borrow());
            assert_eq!(
                chat_state::Timeline::from_events(fixture.events())
                    .unwrap()
                    .parent_message_receipts()
                    .len(),
                1
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn persisted_receipt_with_lost_ack_confirms_after_cold_stop_without_new_interrupt() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            let change = control(&fixture.actor, true);
            let delivery = launch(&fixture.actor, change.clone());
            let planning = fixture.next_write().await;
            fixture.acknowledge(planning);
            let receipt = fixture.next_write().await;
            let id = assert_typed_receipt(&fixture.actor, &receipt.event, &change);
            retain(&fixture.durable, &receipt.event);
            fixture.shutdown.cancel();
            drop(receipt.acknowledgement);
            assert!(matches!(
                delivery_result(delivery).await,
                Err(AgentMessageDeliveryError::Unconfirmed { .. })
            ));
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
            fixture.cold_restore().await;
            fixture.actor.state.lock().await.termination = TerminationState::Graceful;
            let before = serde_json::to_value(fixture.events()).unwrap();
            assert_eq!(
                fixture
                    .actor
                    .receive_plan_guidance("direct-parent".into(), change)
                    .await
                    .unwrap(),
                id
            );
            assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
            assert!(fixture.writes.try_recv().is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn previous_guidance_receipt_confirms_without_republication_after_cold_restore() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            fixture.auto_ack();
            let mut change = control(&fixture.actor, false);
            change.progress = ReplanProgress::Received;
            let mut plan = LocalPlan::new(fixture.actor.session_id_string(), "work:control-turn");
            plan.receive_replan(change.clone()).unwrap();
            fixture.actor.commit_local_plan(None, plan).await.unwrap();
            let id = fixture
                .actor
                .receive_notification_detailed(
                    chat_state::NotificationSource::PlanGuidance {
                        parent_session_id: "direct-parent".into(),
                        message_id: replan_message_id(&change.id),
                    },
                    chat_state::NotificationSourceVersion::Ordinal {
                        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
                    },
                    control::previous_replan_body("direct-parent", &change).unwrap(),
                    None,
                )
                .await
                .unwrap();
            fixture.cold_restore().await;
            fixture.actor.state.lock().await.termination = TerminationState::Graceful;
            fixture.actor.parent_message_interrupt.send_replace(false);
            let before = serde_json::to_value(fixture.events()).unwrap();
            assert_eq!(
                fixture
                    .actor
                    .receive_plan_guidance("direct-parent".into(), change)
                    .await
                    .unwrap(),
                id
            );
            assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn exact_duplicates_keep_received_consumed_and_handled_progress_without_reinterrupting() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            fixture.auto_ack();
            begin_test_active_causal_turn(&fixture.actor).await;
            let change = control(&fixture.actor, false);
            let id = fixture
                .actor
                .receive_plan_guidance("direct-parent".into(), change.clone())
                .await
                .unwrap();
            fixture.actor.parent_message_interrupt.send_replace(false);
            let received = fixture
                .actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                fixture
                    .actor
                    .receive_plan_guidance("direct-parent".into(), change.clone())
                    .await
                    .unwrap(),
                id
            );
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .seq,
                received.seq
            );
            let turn = fixture.actor.events.current_turn().unwrap();
            fixture
                .actor
                .consume_notifications_durably(vec![id.clone()], turn, None)
                .await
                .unwrap();
            for progress in [
                ReplanProgress::Consumed,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Updated {
                        plan_revision: received.plan.revision,
                    },
                },
            ] {
                let snapshot = fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                let mut plan = snapshot.plan;
                plan.advance_replan(&change.id, progress.clone()).unwrap();
                fixture
                    .actor
                    .commit_local_plan(Some(snapshot.seq), plan)
                    .await
                    .unwrap();
                let before = serde_json::to_value(
                    fixture
                        .actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(
                    fixture
                        .actor
                        .receive_plan_guidance("direct-parent".into(), change.clone())
                        .await
                        .unwrap(),
                    id
                );
                let snapshot = fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(snapshot.plan.replans[0].progress, progress);
                assert_eq!(
                    serde_json::to_value(
                        fixture
                            .actor
                            .chat_state_handle
                            .timeline_events()
                            .await
                            .unwrap()
                    )
                    .unwrap(),
                    before
                );
                assert!(!*fixture.actor.parent_message_interrupt.borrow());
            }
            fixture.actor.state.lock().await.termination = TerminationState::Graceful;
            assert_eq!(
                fixture
                    .actor
                    .receive_plan_guidance("direct-parent".into(), change)
                    .await
                    .unwrap(),
                id
            );
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .get_conversation()
                    .await
                    .iter()
                    .filter(|item| matches!(item, ConversationItem::AgentMessage(_)))
                    .count(),
                1
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_binding_target_payload_and_immutable_retry_have_no_admission_side_effects() {
    tokio::task::LocalSet::new().run_until(async {
        let mut fixture = Fixture::new().await;
        let original = control(&fixture.actor, false);
        let mut invalid = Vec::new();
        invalid.push(("lifecycle-root-decoy".to_owned(), original.clone()));
        invalid.push((fixture.actor.session_id_string(), original.clone()));
        let mut changed = original.clone(); changed.child_session_id = "other-child".into();
        invalid.push(("direct-parent".into(), changed));
        let mut changed = original.clone(); changed.link_id = "other-task".into();
        invalid.push(("direct-parent".into(), changed));
        let mut changed = original.clone(); changed.step_id = "other-step".into();
        invalid.push(("direct-parent".into(), changed));
        let mut changed = original.clone(); changed.source_revision = 0;
        invalid.push(("direct-parent".into(), changed));
        let mut changed = original.clone(); changed.id = "invalid\nidentity".into();
        invalid.push(("direct-parent".into(), changed));
        let mut changed = original.clone(); changed.content = "x".repeat(64 * 1024 + 1);
        invalid.push(("direct-parent".into(), changed));
        let mut changed = original.clone(); changed.requirements = Some(vec!["entry".into(); 257]);
        invalid.push(("direct-parent".into(), changed));
        let before = serde_json::to_value(fixture.events()).unwrap();
        for (parent, change) in invalid {
            let result = tokio::time::timeout(Duration::from_secs(1), fixture.actor.receive_plan_guidance(parent, change)).await.unwrap();
            assert!(matches!(result, Err(AgentMessageDeliveryError::Rejected { .. })));
            assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
            assert!(fixture.writes.try_recv().is_err());
        }
        Arc::get_mut(&mut fixture.actor).unwrap().startup_hints.delegated_parent_plan = None;
        assert!(matches!(fixture.actor.receive_plan_guidance("direct-parent".into(), original.clone()).await,
            Err(AgentMessageDeliveryError::Rejected { code, .. }) if code == "invalid_replan_binding"));
        assert!(!fixture.actor.session_dir.join("artifacts/notifications").exists());
        Arc::get_mut(&mut fixture.actor).unwrap().startup_hints.delegated_parent_plan = Some(ParentPlanBinding {
            parent_session_id: "direct-parent".into(), task_id: "task-1".into(), parent_step_id: "step-1".into(),
        });
        fixture.auto_ack();
        fixture.actor.receive_plan_guidance("direct-parent".into(), original.clone()).await.unwrap();
        fixture.actor.parent_message_interrupt.send_replace(false);
        let before = serde_json::to_value(fixture.events()).unwrap();
        let mut changed = original;
        changed.requirements.as_mut().unwrap()[0].push_str("changed immutable payload");
        assert!(matches!(fixture.actor.receive_plan_guidance("direct-parent".into(), changed).await,
            Err(AgentMessageDeliveryError::Rejected { code, .. }) if code == "replan_conflict"));
        assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
        assert!(!*fixture.actor.parent_message_interrupt.borrow());
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn new_controls_require_open_execution_regular_foreground_and_steering() {
    tokio::task::LocalSet::new().run_until(async {
        for boundary in ["execution", "foreground", "steering"] {
            let mut fixture = Fixture::new().await;
            let change = control(&fixture.actor, true);
            {
                let mut state = fixture.actor.state.lock().await;
                match boundary {
                    "execution" => state.termination = TerminationState::Graceful,
                    "foreground" => state.foreground = ForegroundState::Idle,
                    _ => state.foreground.regular_mut().unwrap().steering_open = false,
                }
            }
            let before = serde_json::to_value(fixture.events()).unwrap();
            let result = tokio::time::timeout(Duration::from_secs(1), fixture.actor.receive_plan_guidance("direct-parent".into(), change)).await.unwrap();
            assert!(matches!(result, Err(AgentMessageDeliveryError::Rejected { code, .. }) if code == "target_inactive"));
            assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
            assert!(fixture.writes.try_recv().is_err());
            assert!(!fixture.actor.session_dir.join("artifacts/notifications").exists());
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
        }
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn stopped_received_prefix_cannot_fill_a_missing_receipt_or_claim_success() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            fixture.auto_ack();
            let change = control(&fixture.actor, true);
            let mut received = change.clone();
            received.progress = ReplanProgress::Received;
            let mut plan = LocalPlan::new(fixture.actor.session_id_string(), "original-scope");
            plan.receive_replan(received.clone()).unwrap();
            fixture.actor.commit_local_plan(None, plan).await.unwrap();
            fixture.cold_restore().await;
            fixture.actor.state.lock().await.termination = TerminationState::Graceful;
            let before = serde_json::to_value(fixture.events()).unwrap();
            assert!(
                matches!(fixture.actor.receive_plan_guidance("direct-parent".into(), change).await,
            Err(AgentMessageDeliveryError::Rejected { code, .. }) if code == "target_inactive")
            );
            assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .replans,
                [received]
            );
            assert!(
                fixture
                    .actor
                    .chat_state_handle
                    .parent_message_receipts()
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(fixture.writes.try_recv().is_err());
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn exact_receipt_cannot_bypass_a_missing_planning_responsibility_on_a_closed_target() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            fixture.auto_ack();
            let change = control(&fixture.actor, false);
            fixture
                .actor
                .receive_notification_detailed(
                    chat_state::NotificationSource::PlanGuidance {
                        parent_session_id: "direct-parent".into(),
                        message_id: replan_message_id(&change.id),
                    },
                    chat_state::NotificationSourceVersion::Ordinal {
                        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
                    },
                    control::guidance_body("direct-parent", &change).unwrap(),
                    None,
                )
                .await
                .unwrap();
            fixture.actor.state.lock().await.termination = TerminationState::Graceful;
            let before = serde_json::to_value(fixture.events()).unwrap();
            assert!(
                matches!(fixture.actor.receive_plan_guidance("direct-parent".into(), change).await,
            Err(AgentMessageDeliveryError::Rejected { code, .. }) if code == "target_inactive")
            );
            assert!(
                fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(serde_json::to_value(fixture.events()).unwrap(), before);
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn consumed_ordinary_same_identity_does_not_collide_with_typed_model_delivery() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            fixture.auto_ack();
            begin_test_active_causal_turn(&fixture.actor).await;
            let change = control(&fixture.actor, false);
            let message_id = replan_message_id(&change.id);
            let ordinary_source = chat_state::NotificationSource::ParentMessage {
                parent_session_id: "direct-parent".into(),
                message_id: message_id.clone(),
                interrupt: false,
            };
            let typed_source = chat_state::NotificationSource::PlanGuidance {
                parent_session_id: "direct-parent".into(),
                message_id: message_id.clone(),
            };
            let version = chat_state::NotificationSourceVersion::Ordinal {
                value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
            };
            let ordinary = fixture
                .actor
                .receive_agent_message(
                    "direct-parent".into(),
                    message_id,
                    "ordinary opinion".into(),
                    false,
                    None,
                )
                .await
                .unwrap();
            let turn = fixture.actor.events.current_turn().unwrap();
            fixture
                .actor
                .consume_notifications_durably(vec![ordinary.clone()], turn, None)
                .await
                .unwrap();
            let typed = fixture
                .actor
                .receive_plan_guidance("direct-parent".into(), change.clone())
                .await
                .unwrap();
            assert_ne!(ordinary, typed);
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .parent_message_receipts()
                    .await
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .consumed_notification_turn(ordinary_source, version.clone())
                    .await,
                Some(Some(turn))
            );
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .consumed_notification_turn(typed_source.clone(), version.clone())
                    .await,
                Some(None)
            );
            fixture
                .actor
                .consume_notifications_durably(vec![typed.clone()], turn, None)
                .await
                .unwrap();
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .consumed_notification_turn(typed_source, version)
                    .await,
                Some(Some(turn))
            );
            fixture.actor.parent_message_interrupt.send_replace(false);
            let before = fixture.events().len();
            assert_eq!(
                fixture
                    .actor
                    .receive_plan_guidance("direct-parent".into(), change.clone())
                    .await
                    .unwrap(),
                typed
            );
            assert_eq!(fixture.events().len(), before);
            fixture.cold_restore().await;
            let request = fixture
                .actor
                .chat_state_handle
                .build_request(&fixture.actor.session_id_string(), vec![], None, None, None)
                .await
                .unwrap();
            let messages = request
                .items
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::AgentMessage(batch) => Some(&batch.messages),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>();
            assert_eq!(
                messages
                    .iter()
                    .filter(|message| message.receipt_id == ordinary)
                    .count(),
                1
            );
            let controls = messages
                .iter()
                .filter(|message| message.receipt_id == typed)
                .collect::<Vec<_>>();
            assert_eq!(controls.len(), 1);
            assert_eq!(
                controls[0].message.as_ref(),
                control::guidance_body("direct-parent", &change).unwrap()
            );
            assert!(
                fixture
                    .actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(!*fixture.actor.parent_message_interrupt.borrow());
        })
        .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn stop_softly_wakes_a_real_command_wait_and_keeps_execution_question_and_result_owners() {
    use tool_types::planning::{DependencyKind, WaitCondition, WaitDependency};
    use tools::computer::types::{TaskKind, TerminalRunRequest};
    use tools::types::resources::Terminal;

    tokio::task::LocalSet::new().run_until(async {
        let fixture = Fixture::new().await;
        fixture.auto_ack();
        begin_test_active_causal_turn(&fixture.actor).await;
        let bridge = fixture.actor.agent.borrow().tool_bridge().clone();
        let resources = bridge.shared_resources().await;
        let terminal = resources.lock().await.get::<Terminal>().unwrap().0.clone();
        let release = fixture.actor.session_dir.join("release-controlled-command");
        let quoted_release = release.to_string_lossy().replace('\'', "'\\''");
        let handle = terminal.run_background(TerminalRunRequest {
            command: format!("while [ ! -f '{quoted_release}' ]; do sleep 0.01; done; printf CONTROL_COMMAND_RESULT"),
            working_directory: std::env::temp_dir(),
            env: std::collections::HashMap::new(),
            timeout: Duration::from_secs(30),
            output_byte_limit: 4096,
            output_file: fixture.actor.session_dir.join("controlled-command.log"),
            notification_handle: tools::notification::ToolNotificationHandle::noop(),
            tool_call_id: "control-command".into(),
            display_command: None,
            auto_background_on_timeout: false,
            foreground_block_budget: None,
            kind: TaskKind::Bash,
            owner_session_id: Some(fixture.actor.session_id_string()),
            goal_id: None,
            goal_definition_revision: None,
            description: Some("planning control lifecycle test".into()),
        }).await.unwrap();
        fixture.actor.pending_interactions.lock().unwrap().insert("existing-question".into(), crate::session::pending_interaction::PendingKind::Question);
        let mut plan = LocalPlan::new(fixture.actor.session_id_string(), "original-live-work");
        plan.steps.push(PlanStep {
            id: "local-step".into(), content: "Preserve this existing work".into(),
            status: PlanStepStatus::InProgress, priority: PlanPriority::Normal,
            requirements: None, removed: false,
        });
        plan.links.push(PlanLink {
            id: "existing-descendant".into(), parent_step_id: "local-step".into(), child_session_id: "real-result-still-outstanding".into(),
            terminal: false, required: true, pending_source_revision: None, review_required_revision: 0, result_id: None, dispatch_failure: None,
        });
        plan.waits.push(PlanWait {
            id: "wait-for-command-and-answer".into(), step_ids: vec!["local-step".into()],
            dependencies: vec![
                WaitDependency { kind: DependencyKind::Command, id: handle.task_id.clone() },
                WaitDependency { kind: DependencyKind::Question, id: "existing-question".into() },
            ],
            condition: WaitCondition::All,
        });
        fixture.actor.commit_local_plan(None, plan.clone()).await.unwrap();
        assert!(fixture.actor.planning_waits_for_event().await);
        let waiter = tokio::task::spawn_local({
            let actor = fixture.actor.clone();
            async move { actor.await_planned_dependencies(&crate::session::PromptOrigin::User).await }
        });
        tokio::task::yield_now().await;
        let change = control(&fixture.actor, true);
        let received = fixture.actor.receive_plan_guidance("direct-parent".into(), change).await;
        let wakeup = tokio::time::timeout(Duration::from_secs(2), waiter).await;
        let during_stop = terminal.get_task(&handle.task_id).await;
        // Release the real process before assertions so a failed assertion
        // cannot leave this test's command running in the shared test runtime.
        std::fs::write(&release, b"settle the original command").unwrap();
        let result = terminal.wait_for_completion(&handle.task_id, Some(Duration::from_secs(3))).await.unwrap();
        received.unwrap();
        assert!(matches!(wakeup.unwrap().unwrap().unwrap(), PlannedDependenciesOutcome::ContinueTurn));
        let during_stop = during_stop.unwrap();
        assert!(!during_stop.completed, "stop guidance cannot invent command termination");
        assert_eq!(during_stop.owner_session_id.as_deref(), Some(fixture.actor.session_id_string().as_str()));
        assert!(result.completed);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.output.matches("CONTROL_COMMAND_RESULT").count(), 1);
        let current = fixture.actor.chat_state_handle.planning_state().await.unwrap().unwrap().plan;
        assert_eq!(current.scope_id, plan.scope_id);
        assert_eq!(current.steps, plan.steps);
        assert_eq!(current.links, plan.links);
        assert_eq!(current.waits, plan.waits);
        assert_eq!(current.reviews, plan.reviews);
        assert_eq!(current.replans[0].progress, ReplanProgress::Received);
        assert!(!current.links[0].terminal);
        assert!(current.links[0].result_id.is_none());
        assert!(!fixture.actor.planning_completion_blockers().await.unwrap().is_empty());
        assert_eq!(fixture.actor.pending_interactions.lock().unwrap().get("existing-question"), Some(&crate::session::pending_interaction::PendingKind::Question));
        assert!(fixture.actor.state.lock().await.foreground.regular().is_some_and(|task| !task.is_finished()));
        assert!(!fixture.actor.background_service_shutdown.is_cancelled());
    }).await;
}
