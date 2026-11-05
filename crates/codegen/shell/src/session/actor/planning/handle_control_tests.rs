use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::{begin_test_active_causal_turn, build_actor};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tool_types::planning::WaitDependency;
use tools::implementations::grow_build::task::{
    backend::{ChannelBackend, SubagentBackendResource},
    interaction::{AgentInteraction, AgentInteractionOutput},
    types::SubagentEvent,
};
use tools::implementations::grow_build::todo::TodoUpdate;

type DurableEvents = Arc<Mutex<Vec<chat_state::TimelineEvent>>>;

struct PendingPlanningWrite {
    event: chat_state::TimelineEvent,
    acknowledgement: oneshot::Sender<std::io::Result<()>>,
}

struct HandlePersistence {
    durable: DurableEvents,
    manual: Arc<AtomicBool>,
    writes: mpsc::UnboundedSender<PendingPlanningWrite>,
}

fn retain(durable: &DurableEvents, event: &chat_state::TimelineEvent) {
    let mut events = durable.lock().unwrap();
    if let Some(previous) = events.iter().find(|previous| previous.seq == event.seq) {
        assert_eq!(
            serde_json::to_value(previous).unwrap(),
            serde_json::to_value(event).unwrap()
        );
    } else {
        events.push(event.clone());
    }
}

impl chat_state::TimelinePersistence for HandlePersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> oneshot::Receiver<std::io::Result<()>> {
        let (acknowledgement, response) = oneshot::channel();
        if matches!(event.kind, chat_state::TimelineEventKind::Planning(_))
            && self.manual.load(Ordering::Acquire)
        {
            self.writes
                .send(PendingPlanningWrite {
                    event: event.clone(),
                    acknowledgement,
                })
                .unwrap();
        } else {
            retain(&self.durable, event);
            let _ = acknowledgement.send(Ok(()));
        }
        response
    }

    fn flush(&mut self) {}
}

struct Fixture {
    actor: Arc<SessionActor>,
    durable: DurableEvents,
    manual: Arc<AtomicBool>,
    writes: mpsc::UnboundedReceiver<PendingPlanningWrite>,
    write_tx: mpsc::UnboundedSender<PendingPlanningWrite>,
    sampling: sampling_types::SamplingConfig,
    shutdown: tokio_util::sync::CancellationToken,
}

impl Fixture {
    async fn new() -> Self {
        let (mut actor, _gateway) = build_actor().await;
        let target = Arc::get_mut(&mut actor).unwrap();
        target.startup_hints.is_subagent = true;
        target.startup_hints.parent_session_id = Some("lifecycle-root-decoy".into());
        target.startup_hints.delegated_parent_plan = Some(ParentPlanBinding {
            parent_session_id: "direct-parent".into(),
            task_id: "task-1".into(),
            parent_step_id: "step-1".into(),
        });
        let mut plan = LocalPlan::new(actor.session_id_string(), "work:handle-control");
        plan.steps.push(PlanStep {
            id: "local-step".into(),
            content: "Preserve completed investigation".into(),
            status: PlanStepStatus::InProgress,
            priority: PlanPriority::Normal,
            requirements: None,
            removed: false,
        });
        actor.commit_local_plan(None, plan).await.unwrap();
        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        let sampling = actor.chat_state_handle.get_sampling_config().await.unwrap();
        let durable = Arc::new(Mutex::new(events.clone()));
        let manual = Arc::new(AtomicBool::new(false));
        let (write_tx, writes) = mpsc::unbounded_channel();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let (event_tx, _events) = mpsc::unbounded_channel();
        let handle = chat_state::ChatStateActor::spawn_from_timeline(
            events,
            sampling.clone(),
            Box::new(HandlePersistence {
                durable: durable.clone(),
                manual: manual.clone(),
                writes: write_tx.clone(),
            }),
            event_tx,
            shutdown.clone(),
        )
        .await
        .unwrap();
        let target = Arc::get_mut(&mut actor).unwrap();
        target.chat_state_handle = handle;
        target.events = crate::session::events::EventTracker::new(target.chat_state_handle.clone());
        begin_test_active_causal_turn(&actor).await;
        Self {
            actor,
            durable,
            manual,
            writes,
            write_tx,
            sampling,
            shutdown,
        }
    }

    fn events(&self) -> Vec<chat_state::TimelineEvent> {
        self.durable.lock().unwrap().clone()
    }

    async fn plan(&self) -> LocalPlan {
        self.actor
            .chat_state_handle
            .planning_state()
            .await
            .unwrap()
            .unwrap()
            .plan
    }

    async fn consume(&self, ids: Vec<String>) {
        self.actor
            .consume_notifications_durably(ids, self.actor.events.current_turn().unwrap(), None)
            .await
            .unwrap();
    }

    async fn receive(&self, change: &tool_types::ReplanChange) -> String {
        self.actor
            .receive_plan_guidance("direct-parent".into(), change.clone())
            .await
            .unwrap()
    }

    async fn next_write(&mut self) -> PendingPlanningWrite {
        tokio::time::timeout(Duration::from_secs(2), self.writes.recv())
            .await
            .unwrap()
            .unwrap()
    }

    fn acknowledge(&self, write: PendingPlanningWrite) {
        retain(&self.durable, &write.event);
        write.acknowledgement.send(Ok(())).unwrap();
    }

    async fn cold_restore(&mut self) {
        self.actor.planning_delivery.abort_and_join().await.unwrap();
        self.shutdown.cancel();
        wait_for_closed_owner(&self.actor).await;
        self.manual.store(false, Ordering::Release);
        let shutdown = tokio_util::sync::CancellationToken::new();
        let (event_tx, _events) = mpsc::unbounded_channel();
        let handle = chat_state::ChatStateActor::spawn_from_timeline(
            self.events(),
            self.sampling.clone(),
            Box::new(HandlePersistence {
                durable: self.durable.clone(),
                manual: self.manual.clone(),
                writes: self.write_tx.clone(),
            }),
            event_tx,
            shutdown.clone(),
        )
        .await
        .expect("every durable handling prefix must cold-validate");
        let target = Arc::get_mut(&mut self.actor).unwrap();
        target.chat_state_handle = handle;
        target.events = crate::session::events::EventTracker::new(target.chat_state_handle.clone());
        self.shutdown = shutdown;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.actor.planning_delivery.cancel();
        self.actor.background_service_shutdown.cancel();
        self.shutdown.cancel();
    }
}

async fn wait_for_closed_owner(actor: &SessionActor) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while !actor.chat_state_handle.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn control(actor: &SessionActor, revision: u64, stop: bool) -> tool_types::ReplanChange {
    tool_types::ReplanChange {
        id: format!("task-1:step-1:{revision}"),
        link_id: "task-1".into(),
        child_session_id: actor.session_id_string(),
        step_id: "step-1".into(),
        source_revision: revision,
        content: "The complete parent requirement 界🙂".into(),
        requirements: Some(vec!["Preserve the exact typed control identity".into()]),
        priority: PlanPriority::High,
        status: if stop {
            PlanStepStatus::Cancelled
        } else {
            PlanStepStatus::InProgress
        },
        removed: false,
        progress: ReplanProgress::Dispatching,
    }
}

fn handle(ids: &[&str], reason: Option<&str>) -> PlanningAction {
    PlanningAction::HandleReplans {
        ids: ids.iter().map(|id| (*id).to_owned()).collect(),
        no_change_reason: reason.map(str::to_owned),
    }
}

fn input(actions: Vec<PlanningAction>, edit: bool) -> TodoWriteInput {
    TodoWriteInput {
        merge: true,
        todos: if edit {
            vec![TodoUpdate {
                id: "local-step".into(),
                content: Some("Adjust only remaining verification".into()),
                status: None,
                priority: None,
                requirements: None,
                removed: None,
            }]
        } else {
            Vec::new()
        },
        actions,
    }
}

fn launch_write(
    actor: &Arc<SessionActor>,
    input: TodoWriteInput,
) -> tokio::task::JoinHandle<Result<LocalPlan, String>> {
    let actor = actor.clone();
    tokio::task::spawn_local(async move {
        let (respond_to, response) = oneshot::channel();
        actor
            .handle_planning_command(PlanningCommand::Write {
                prompt_id: "test-active-turn".into(),
                input,
                respond_to,
            })
            .await;
        response.await.unwrap()
    })
}

async fn write(actor: &Arc<SessionActor>, input: TodoWriteInput) -> Result<LocalPlan, String> {
    tokio::time::timeout(Duration::from_secs(2), launch_write(actor, input))
        .await
        .unwrap()
        .unwrap()
}

fn planning(event: &chat_state::TimelineEvent) -> &LocalPlan {
    let chat_state::TimelineEventKind::Planning(event) = &event.kind else {
        panic!("expected a distinct durable Planning fact")
    };
    &event.plan
}

#[tokio::test(flavor = "current_thread")]
async fn ordinary_same_identity_cannot_handle_pending_typed_guidance_or_edit_todos() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for stop in [false, true] {
                let mut fixture = Fixture::new().await;
                let change = control(&fixture.actor, 7, stop);
                let ordinary = fixture
                    .actor
                    .receive_agent_message(
                        "direct-parent".into(),
                        replan_message_id(&change.id),
                        "An ordinary opinion with the same identity".into(),
                        false,
                        None,
                    )
                    .await
                    .unwrap();
                fixture.consume(vec![ordinary.clone()]).await;
                let typed = fixture.receive(&change).await;
                assert_ne!(ordinary, typed);
                let before = fixture.events();
                let original = fixture.plan().await;
                let command = input(vec![handle(&[&change.id], None)], true);
                assert!(
                    write(&fixture.actor, command.clone())
                        .await
                        .unwrap_err()
                        .contains("has not been consumed")
                );
                assert_eq!(fixture.plan().await, original);
                assert_eq!(
                    serde_json::to_value(fixture.events()).unwrap(),
                    serde_json::to_value(before).unwrap()
                );
                assert!(fixture.writes.try_recv().is_err());
                fixture.consume(vec![typed.clone()]).await;
                let prefix = fixture.events().len();
                let handled = write(&fixture.actor, command).await.unwrap();
                let facts = fixture.events();
                let updates = facts[prefix..]
                    .iter()
                    .filter(|event| {
                        matches!(event.kind, chat_state::TimelineEventKind::Planning(_))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(updates.len(), 2);
                let consumed = planning(updates[0]);
                assert_eq!(consumed.steps, original.steps);
                assert_eq!(consumed.revision, original.revision);
                assert_eq!(consumed.replans[0].progress, ReplanProgress::Consumed);
                assert_eq!(planning(updates[1]), &handled);
                let outcome = if stop {
                    ReplanOutcome::Stopping
                } else {
                    ReplanOutcome::Updated {
                        plan_revision: handled.revision,
                    }
                };
                assert_eq!(
                    handled.replans[0].progress,
                    ReplanProgress::Handled { outcome }
                );
                assert_eq!(
                    handled.steps[0].content,
                    "Adjust only remaining verification"
                );
                assert_eq!(handled.revision, original.revision + 1);
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
                let typed_messages = messages
                    .iter()
                    .filter(|message| message.receipt_id == typed)
                    .collect::<Vec<_>>();
                assert_eq!(typed_messages.len(), 1);
                assert_eq!(
                    typed_messages[0].message.as_ref(),
                    control::guidance_body("direct-parent", &change).unwrap()
                );
                chat_state::Timeline::from_events(facts).unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stop_no_change_rejects_all_actions_and_edits_before_any_consumed_commit() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for edit in [false, true] {
                for mixed in [0, 1, 2] {
                    let mut fixture = Fixture::new().await;
                    let ordinary = control(&fixture.actor, 7, false);
                    let stop = control(&fixture.actor, 8, true);
                    let first = fixture.receive(&ordinary).await;
                    let second = fixture.receive(&stop).await;
                    fixture.consume(vec![first, second]).await;
                    let actions = match mixed {
                        0 => vec![handle(&[&stop.id], Some("the current plan still applies"))],
                        1 => vec![handle(
                            &[&ordinary.id, &stop.id],
                            Some("the current plan still applies"),
                        )],
                        _ => vec![
                            handle(&[&ordinary.id], None),
                            handle(&[&stop.id], Some("the current plan still applies")),
                        ],
                    };
                    let before = fixture.events();
                    let original = fixture.plan().await;
                    assert!(
                        write(&fixture.actor, input(actions, edit))
                            .await
                            .unwrap_err()
                            .contains("stop guidance")
                    );
                    assert_eq!(fixture.plan().await, original);
                    assert_eq!(
                        serde_json::to_value(fixture.events()).unwrap(),
                        serde_json::to_value(before).unwrap()
                    );
                    assert!(fixture.writes.try_recv().is_err());
                    assert!(
                        fixture
                            .plan()
                            .await
                            .replans
                            .iter()
                            .all(|change| change.progress == ReplanProgress::Received)
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ordinary_no_change_and_mixed_stop_keep_their_distinct_outcomes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            let ordinary = control(&fixture.actor, 7, false);
            let receipt = fixture.receive(&ordinary).await;
            fixture.consume(vec![receipt]).await;
            let original = fixture.plan().await;
            let reason = "Existing verification already covers the changed requirement";
            let result = write(
                &fixture.actor,
                input(vec![handle(&[&ordinary.id], Some(reason))], false),
            )
            .await
            .unwrap();
            assert_eq!(result.steps, original.steps);
            assert_eq!(result.revision, original.revision);
            assert_eq!(
                result.replans[0].progress,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::NoChange {
                        reason: reason.into()
                    }
                }
            );

            let fixture = Fixture::new().await;
            let ordinary = control(&fixture.actor, 7, false);
            let mut stop = control(&fixture.actor, 8, false);
            stop.removed = true;
            let first = fixture.receive(&ordinary).await;
            let second = fixture.receive(&stop).await;
            fixture.consume(vec![first, second]).await;
            let result = write(
                &fixture.actor,
                input(vec![handle(&[&ordinary.id, &stop.id], None)], true),
            )
            .await
            .unwrap();
            assert_eq!(
                result.replans[0].progress,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Updated {
                        plan_revision: result.revision
                    }
                }
            );
            assert_eq!(
                result.replans[1].progress,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Stopping
                }
            );
            assert!(!fixture.actor.background_service_shutdown.is_cancelled());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn all_controls_require_current_host_parent_task_and_step_even_with_consumed_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for invalid in ["absent", "parent", "self", "task", "step"] {
                let mut fixture = Fixture::new().await;
                let change = control(&fixture.actor, 7, true);
                let receipt = fixture.receive(&change).await;
                fixture.consume(vec![receipt]).await;
                let target = Arc::get_mut(&mut fixture.actor).unwrap();
                if invalid == "absent" {
                    target.startup_hints.delegated_parent_plan = None;
                } else {
                    let binding = target.startup_hints.delegated_parent_plan.as_mut().unwrap();
                    match invalid {
                        "parent" => binding.parent_session_id = "lifecycle-root-decoy".into(),
                        "self" => binding.parent_session_id = target.session_info.id.0.to_string(),
                        "task" => binding.task_id = "old-resume-task".into(),
                        _ => binding.parent_step_id = "old-resume-step".into(),
                    }
                }
                let before = fixture.events();
                let command = input(vec![handle(&[&change.id], None)], true);
                assert!(write(&fixture.actor, command).await.is_err(), "{invalid}");
                assert_eq!(
                    serde_json::to_value(fixture.events()).unwrap(),
                    serde_json::to_value(before).unwrap()
                );
                assert!(fixture.writes.try_recv().is_err());
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn receiving_child_cannot_handle_a_locally_delegated_descendants_control() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            let snapshot = fixture
                .actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let mut plan = snapshot.plan;
            plan.revision = 7;
            let mut change = control(&fixture.actor, 7, true);
            change.child_session_id = "different-child".into();
            change.step_id = "local-step".into();
            change.progress = ReplanProgress::Received;
            plan.links.push(PlanLink {
                id: change.link_id.clone(),
                parent_step_id: change.step_id.clone(),
                child_session_id: change.child_session_id.clone(),
                terminal: false,
                required: true,
                pending_source_revision: Some(7),
                review_required_revision: 7,
                result_id: None,
                dispatch_failure: None,
            });
            plan.replans.push(change.clone());
            fixture
                .actor
                .commit_local_plan(Some(snapshot.seq), plan)
                .await
                .unwrap();
            let before = fixture.events();
            assert!(
                write(
                    &fixture.actor,
                    input(vec![handle(&[&change.id], None)], true)
                )
                .await
                .unwrap_err()
                .contains("only the receiving child")
            );
            assert_eq!(
                serde_json::to_value(fixture.events()).unwrap(),
                serde_json::to_value(before).unwrap()
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn non_handling_progress_stays_rejected_despite_exact_typed_consumption() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for progress in [
                ReplanProgress::Pending,
                ReplanProgress::Dispatching,
                ReplanProgress::Unapplied {
                    reason: "Execution already ended".into(),
                },
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Stopping,
                },
            ] {
                let fixture = Fixture::new().await;
                let mut change = control(&fixture.actor, 7, true);
                change.progress = progress.clone();
                let snapshot = fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                let mut plan = snapshot.plan;
                plan.replans.push(change.clone());
                fixture
                    .actor
                    .commit_local_plan(Some(snapshot.seq), plan)
                    .await
                    .unwrap();
                let receipt = fixture
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
                fixture.consume(vec![receipt]).await;
                let before = fixture.events();
                assert!(
                    write(
                        &fixture.actor,
                        input(vec![handle(&[&change.id], None)], true)
                    )
                    .await
                    .unwrap_err()
                    .contains("not awaiting handling"),
                    "{progress:?}"
                );
                assert_eq!(
                    serde_json::to_value(fixture.events()).unwrap(),
                    serde_json::to_value(before).unwrap()
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unavailable_receipt_owner_stays_an_error_and_cannot_create_an_empty_plan() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            let change = control(&fixture.actor, 7, true);
            let receipt = fixture.receive(&change).await;
            fixture.consume(vec![receipt]).await;
            let before = fixture.events();
            fixture.shutdown.cancel();
            wait_for_closed_owner(&fixture.actor).await;
            assert_eq!(
                fixture
                    .actor
                    .consumed_replan_guidance_reference(&change)
                    .await
                    .unwrap_err(),
                "planning guidance receipt owner is unavailable"
            );
            assert!(
                write(
                    &fixture.actor,
                    input(vec![handle(&[&change.id], None)], true)
                )
                .await
                .is_err()
            );
            assert_eq!(
                serde_json::to_value(fixture.events()).unwrap(),
                serde_json::to_value(before).unwrap()
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn consumed_then_handled_are_separate_durable_acks_under_the_same_step_gate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            let change = control(&fixture.actor, 7, true);
            let receipt = fixture.receive(&change).await;
            fixture.consume(vec![receipt]).await;
            let original = fixture.plan().await;
            fixture.manual.store(true, Ordering::Release);
            let command = launch_write(
                &fixture.actor,
                input(vec![handle(&[&change.id], None)], true),
            );
            let consumed = fixture.next_write().await;
            assert_eq!(
                planning(&consumed.event).replans[0].progress,
                ReplanProgress::Consumed
            );
            assert_eq!(planning(&consumed.event).steps, original.steps);
            assert_eq!(planning(&consumed.event).revision, original.revision);
            assert!(!command.is_finished());
            assert!(fixture.actor.step_control_gate.try_lock().is_err());
            fixture.acknowledge(consumed);
            let handled = fixture.next_write().await;
            assert_eq!(
                planning(&handled.event).replans[0].progress,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Stopping
                }
            );
            assert_eq!(planning(&handled.event).revision, original.revision + 1);
            let prefix = chat_state::Timeline::from_events(fixture.events()).unwrap();
            assert_eq!(
                prefix.planning_snapshot().unwrap().plan.replans[0].progress,
                ReplanProgress::Consumed
            );
            assert_eq!(
                prefix.planning_snapshot().unwrap().plan.steps,
                original.steps
            );
            assert!(!command.is_finished());
            assert!(fixture.actor.step_control_gate.try_lock().is_err());
            fixture.acknowledge(handled);
            let result = tokio::time::timeout(Duration::from_secs(2), command)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(
                result.replans[0].progress,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Stopping
                }
            );
            assert!(!fixture.actor.background_service_shutdown.is_cancelled());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn lost_or_failed_consumed_ack_cold_retry_preserves_prefix_and_edits_once() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for lost in [false, true] {
                let mut fixture = Fixture::new().await;
                let change = control(&fixture.actor, 7, true);
                let receipt = fixture.receive(&change).await;
                fixture.consume(vec![receipt.clone()]).await;
                let original = fixture.plan().await;
                fixture.manual.store(true, Ordering::Release);
                let request = input(vec![handle(&[&change.id], None)], true);
                let command = launch_write(&fixture.actor, request.clone());
                let consumed = fixture.next_write().await;
                let expected = consumed.event.clone();
                if lost {
                    retain(&fixture.durable, &consumed.event);
                    fixture.shutdown.cancel();
                    drop(consumed.acknowledgement);
                } else {
                    consumed
                        .acknowledgement
                        .send(Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "injected consumed failure",
                        )))
                        .unwrap();
                }
                assert!(
                    tokio::time::timeout(Duration::from_secs(2), command)
                        .await
                        .unwrap()
                        .unwrap()
                        .is_err()
                );
                let prefix = chat_state::Timeline::from_events(fixture.events()).unwrap();
                let durable = &prefix.planning_snapshot().unwrap().plan;
                assert_eq!(durable.steps, original.steps);
                assert_eq!(durable.revision, original.revision);
                assert_eq!(
                    durable.replans[0].progress,
                    if lost {
                        ReplanProgress::Consumed
                    } else {
                        ReplanProgress::Received
                    }
                );
                fixture.cold_restore().await;
                let result = write(&fixture.actor, request.clone()).await.unwrap();
                assert_eq!(result.revision, original.revision + 1);
                assert_eq!(
                    result.replans[0].progress,
                    ReplanProgress::Handled {
                        outcome: ReplanOutcome::Stopping
                    }
                );
                let events = fixture.events();
                let consumed_facts = events
                    .iter()
                    .filter(|event| match &event.kind {
                        chat_state::TimelineEventKind::Planning(event) => {
                            event.plan.replans.iter().any(|record| {
                                record.id == change.id
                                    && record.progress == ReplanProgress::Consumed
                            })
                        }
                        _ => false,
                    })
                    .count();
                assert_eq!(consumed_facts, 1);
                if lost {
                    let preserved = events
                        .iter()
                        .find(|event| event.seq == expected.seq)
                        .unwrap();
                    assert_eq!(
                        serde_json::to_value(preserved).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                }
                assert_eq!(
                    fixture
                        .actor
                        .chat_state_handle
                        .parent_message_receipts()
                        .await
                        .unwrap()
                        .len(),
                    1
                );
                let before = fixture.events();
                assert!(
                    write(&fixture.actor, request)
                        .await
                        .unwrap_err()
                        .contains("not awaiting handling")
                );
                assert_eq!(
                    serde_json::to_value(fixture.events()).unwrap(),
                    serde_json::to_value(before).unwrap()
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn handled_ack_loss_retries_exact_fact_without_duplicate_edits_or_early_response() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new().await;
            let change = control(&fixture.actor, 7, true);
            let receipt = fixture.receive(&change).await;
            fixture.consume(vec![receipt]).await;
            fixture.manual.store(true, Ordering::Release);
            let command = launch_write(
                &fixture.actor,
                input(vec![handle(&[&change.id], None)], true),
            );
            let consumed = fixture.next_write().await;
            fixture.acknowledge(consumed);
            let handled = fixture.next_write().await;
            let exact = serde_json::to_value(&handled.event).unwrap();
            retain(&fixture.durable, &handled.event);
            drop(handled.acknowledgement);
            let retry = fixture.next_write().await;
            assert_eq!(serde_json::to_value(&retry.event).unwrap(), exact);
            assert!(!command.is_finished());
            retry
                .acknowledgement
                .send(Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "injected transient acknowledgement failure",
                )))
                .unwrap();
            let final_retry = fixture.next_write().await;
            assert_eq!(serde_json::to_value(&final_retry.event).unwrap(), exact);
            assert!(!command.is_finished());
            fixture.acknowledge(final_retry);
            let result = tokio::time::timeout(Duration::from_secs(2), command)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(result.revision, 1);
            let facts = fixture.events();
            let handled_facts = facts
                .iter()
                .filter(|event| match &event.kind {
                    chat_state::TimelineEventKind::Planning(event) => {
                        event.plan.replans.iter().any(|record| {
                            record.id == change.id
                                && matches!(record.progress, ReplanProgress::Handled { .. })
                        })
                    }
                    _ => false,
                })
                .count();
            assert_eq!(handled_facts, 1);
            chat_state::Timeline::from_events(facts).unwrap();
        })
        .await;
}

async fn install_channel(actor: &SessionActor) -> mpsc::UnboundedReceiver<SubagentEvent> {
    let (sender, receiver) = mpsc::unbounded_channel();
    let bridge = actor.agent.borrow().tool_bridge().clone();
    bridge
        .update_resource(SubagentBackendResource(Arc::new(
            ChannelBackend::for_session(sender, actor.session_id_string()),
        )))
        .await;
    receiver
}

#[tokio::test(flavor = "current_thread")]
async fn handled_producer_ignores_early_ordinary_and_wrong_parent_receipts_until_exact_consumption()
{
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            let change = control(&fixture.actor, 7, true);
            let mut ordinary_ids = Vec::new();
            for sender in ["wrong-parent", "direct-parent"] {
                ordinary_ids.push(
                    fixture
                        .actor
                        .receive_agent_message(
                            sender.into(),
                            replan_message_id(&change.id),
                            "earlier ordinary receipt".into(),
                            false,
                            None,
                        )
                        .await
                        .unwrap(),
                );
            }
            ordinary_ids.push(
                fixture
                    .actor
                    .receive_notification_detailed(
                        chat_state::NotificationSource::PlanGuidance {
                            parent_session_id: "wrong-parent".into(),
                            message_id: replan_message_id(&change.id),
                        },
                        chat_state::NotificationSourceVersion::Ordinal {
                            value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
                        },
                        control::guidance_body("wrong-parent", &change).unwrap(),
                        None,
                    )
                    .await
                    .unwrap(),
            );
            fixture.consume(ordinary_ids).await;
            let typed = fixture.receive(&change).await;
            // A history written by the former message-ID-only handler can already
            // contain Handled while the actual typed control is still unconsumed.
            for progress in [
                ReplanProgress::Consumed,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Stopping,
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
                plan.advance_replan(&change.id, progress).unwrap();
                fixture
                    .actor
                    .commit_local_plan(Some(snapshot.seq), plan)
                    .await
                    .unwrap();
            }
            let mut events = install_channel(&fixture.actor).await;
            let mut acknowledged = std::collections::HashSet::new();
            assert!(
                fixture
                    .actor
                    .deliver_planning_changes(
                        tokio::time::Instant::now() + Duration::from_secs(1),
                        &mut acknowledged
                    )
                    .await
                    .unwrap_err()
                    .contains("has not been consumed")
            );
            assert!(events.try_recv().is_err());
            assert!(acknowledged.is_empty());
            fixture.consume(vec![typed]).await;
            let (delivered, ()) = tokio::join!(
                fixture.actor.deliver_planning_changes(
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    &mut acknowledged,
                ),
                async {
                    let SubagentEvent::Interact(request) =
                        tokio::time::timeout(Duration::from_secs(2), events.recv())
                            .await
                            .expect("the concurrently driven delivery must reach ChannelBackend")
                            .unwrap()
                    else {
                        panic!("expected host-only handling acknowledgement")
                    };
                    assert_eq!(request.source_session_id, fixture.actor.session_id_string());
                    assert!(request.target_child_id.is_none());
                    let AgentInteraction::ReplanHandled {
                        change_id,
                        outcome,
                        reply_to,
                    } = request.action
                    else {
                        panic!("expected ReplanHandled")
                    };
                    assert_eq!(change_id, change.id);
                    assert_eq!(outcome, ReplanOutcome::Stopping);
                    assert_eq!(
                        reply_to,
                        sampling_types::AgentMessageRef {
                            source_session_id: "direct-parent".into(),
                            message_id: replan_message_id(&change.id)
                        }
                    );
                    request
                        .respond_to
                        .send(Ok(AgentInteractionOutput::message_received(
                            request.id,
                            "exact-handled-ack",
                        )))
                        .unwrap();
                }
            );
            delivered.unwrap();
            assert_eq!(acknowledged, std::collections::HashSet::from([change.id]));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unresolved_history_does_not_starve_later_ack_or_direct_child_control_after_cold_load() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for typed_pending in [false, true] {
                let mut fixture = Fixture::new().await;
                let first = control(&fixture.actor, 7, false);
                let ordinary = fixture
                    .actor
                    .receive_agent_message(
                        "direct-parent".into(),
                        replan_message_id(&first.id),
                        "Ordinary consumption is not control consumption".into(),
                        false,
                        None,
                    )
                    .await
                    .unwrap();
                fixture.consume(vec![ordinary]).await;
                let first_receipt = if typed_pending {
                    Some(fixture.receive(&first).await)
                } else {
                    let snapshot = fixture
                        .actor
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap();
                    let mut plan = snapshot.plan;
                    let mut historical = first.clone();
                    historical.progress = ReplanProgress::Received;
                    plan.receive_replan(historical).unwrap();
                    fixture
                        .actor
                        .commit_local_plan(Some(snapshot.seq), plan)
                        .await
                        .unwrap();
                    None
                };
                let later = control(&fixture.actor, 8, false);
                let later_receipt = fixture.receive(&later).await;
                fixture.consume(vec![later_receipt]).await;
                // Retain history admitted before exact typed consumption was
                // required. Neither cold loading nor delivery may rewrite it.
                for progress in [
                    ReplanProgress::Consumed,
                    ReplanProgress::Handled {
                        outcome: ReplanOutcome::Updated { plan_revision: 0 },
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
                    for id in [&first.id, &later.id] {
                        plan.advance_replan(id, progress.clone()).unwrap();
                    }
                    fixture
                        .actor
                        .commit_local_plan(Some(snapshot.seq), plan)
                        .await
                        .unwrap();
                }
                let snapshot = fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                let mut plan = snapshot.plan;
                plan.links.push(PlanLink {
                    id: "grandchild-task".into(),
                    parent_step_id: "local-step".into(),
                    child_session_id: "actual-grandchild-session".into(),
                    terminal: false,
                    required: true,
                    pending_source_revision: None,
                    review_required_revision: 0,
                    result_id: None,
                    dispatch_failure: None,
                });
                plan.update_steps(
                    tool_types::PlanUpdateMode::Merge,
                    vec![PlanStep {
                        content: "Revised independent grandchild requirement".into(),
                        ..plan.steps[0].clone()
                    }],
                )
                .unwrap();
                let mut outgoing = plan.replans[2].clone();
                if typed_pending {
                    plan.advance_replan(&outgoing.id, ReplanProgress::Dispatching)
                        .unwrap();
                    outgoing.progress = ReplanProgress::Dispatching;
                }
                fixture
                    .actor
                    .commit_local_plan(Some(snapshot.seq), plan)
                    .await
                    .unwrap();
                let historical = fixture.plan().await.replans[0].clone();
                assert_eq!(
                    fixture
                        .plan()
                        .await
                        .replans
                        .iter()
                        .map(|record| record.id.as_str())
                        .collect::<Vec<_>>(),
                    [first.id.as_str(), later.id.as_str(), outgoing.id.as_str()]
                );
                fixture.cold_restore().await;
                let restored = chat_state::Timeline::from_events(
                    fixture.actor.chat_state_handle.timeline_events().await.unwrap(),
                )
                .unwrap();
                assert_eq!(restored.active_turn(), None);
                assert_eq!(fixture.plan().await.replans[0], historical);
                let mut events = install_channel(&fixture.actor).await;
                let mut acknowledged = std::collections::HashSet::new();
                for pass in 0..3 {
                    if pass == 2 {
                        begin_test_active_causal_turn(&fixture.actor).await;
                        let receipt = fixture.receive(&first).await;
                        if let Some(expected) = &first_receipt {
                            assert_eq!(&receipt, expected);
                        }
                        fixture.consume(vec![receipt]).await;
                    }
                    let expected_handled = match pass {
                        0 => Some(&later.id),
                        1 => None,
                        _ => Some(&first.id),
                    };
                    let mut expected_outgoing = outgoing.clone();
                    expected_outgoing.progress = if pass == 0 {
                        ReplanProgress::Dispatching
                    } else {
                        ReplanProgress::Received
                    };
                    let before = fixture.events();
                    let exchange = async {
                        if let Some(id) = expected_handled {
                            let SubagentEvent::Interact(request) = tokio::time::timeout(
                                Duration::from_secs(1),
                                events.recv(),
                            )
                            .await
                            .expect("an unresolved earlier record must not starve a later exact handled ACK")
                            .unwrap()
                            else {
                                panic!("expected handled acknowledgement through ChannelBackend")
                            };
                            assert_eq!(request.source_session_id, fixture.actor.session_id_string());
                            assert!(request.target_child_id.is_none());
                            let AgentInteraction::ReplanHandled {
                                change_id,
                                outcome,
                                reply_to,
                            } = request.action
                            else {
                                panic!("independent handled ACK must precede the outgoing child control")
                            };
                            assert_eq!(&change_id, id);
                            assert_eq!(outcome, ReplanOutcome::Updated { plan_revision: 0 });
                            assert_eq!(
                                reply_to,
                                sampling_types::AgentMessageRef {
                                    source_session_id: "direct-parent".into(),
                                    message_id: replan_message_id(id),
                                }
                            );
                            request
                                .respond_to
                                .send(Ok(AgentInteractionOutput::message_received(
                                    request.id,
                                    format!("handled-{pass}"),
                                )))
                                .unwrap();
                        }
                        let SubagentEvent::Interact(request) = tokio::time::timeout(
                            Duration::from_secs(1),
                            events.recv(),
                        )
                        .await
                        .expect("an unresolved earlier record must not starve a direct-child control")
                        .unwrap()
                        else {
                            panic!("expected outgoing planning control through ChannelBackend")
                        };
                        assert_eq!(request.source_session_id, fixture.actor.session_id_string());
                        assert_eq!(request.target_child_id.as_deref(), Some("grandchild-task"));
                        assert_eq!(request.id, replan_message_id(&outgoing.id));
                        let AgentInteraction::Replan { change } = request.action else {
                            panic!("already acknowledged later Handled must not be sent again")
                        };
                        assert_eq!(change, expected_outgoing);
                        assert_eq!(change.child_session_id, "actual-grandchild-session");
                        let source = fixture
                            .actor
                            .chat_state_handle
                            .planning_state()
                            .await
                            .unwrap()
                            .unwrap();
                        assert!(source.plan.replans.iter().any(|record| record == &change));
                        request
                            .respond_to
                            .send(Ok(AgentInteractionOutput::message_received(
                                request.id,
                                "grandchild-receipt",
                            )))
                            .unwrap();
                    };
                    let (delivered, ()) = tokio::join!(
                        fixture.actor.deliver_planning_changes(
                            tokio::time::Instant::now() + Duration::from_secs(2),
                            &mut acknowledged,
                        ),
                        exchange,
                    );
                    if pass < 2 {
                        assert!(delivered.unwrap_err().contains("has not been consumed"));
                        assert!(!acknowledged.contains(&first.id));
                        assert!(
                            fixture
                                .actor
                                .planning_delivery_needs_retry(&acknowledged)
                                .await
                                .unwrap()
                        );
                    } else {
                        delivered.unwrap();
                        assert!(acknowledged.contains(&first.id));
                        assert!(
                            !fixture
                                .actor
                                .planning_delivery_needs_retry(&acknowledged)
                                .await
                                .unwrap()
                        );
                    }
                    assert!(acknowledged.contains(&later.id));
                    assert!(
                        events.try_recv().is_err(),
                        "one pass must not duplicate a valid Handled ACK"
                    );
                    let current = fixture.plan().await;
                    assert_eq!(current.replans[0], historical);
                    assert_eq!(current.replans[1].id, later.id);
                    assert_eq!(current.replans[2].progress, ReplanProgress::Received);
                    assert!(!current.links[0].terminal);
                    assert!(current.links[0].result_id.is_none());
                    let after = fixture.events();
                    assert_eq!(
                        serde_json::to_value(&after[..before.len()]).unwrap(),
                        serde_json::to_value(before).unwrap()
                    );
                    assert_eq!(current.replans.len(), 3);
                }
                assert_eq!(acknowledged.len(), 2);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stop_ack_reaches_parent_without_settling_live_links_waits_reviews_or_questions() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new().await;
            let child = &fixture.actor;
            child.pending_interactions.lock().unwrap().insert(
                "existing-question".into(),
                crate::session::pending_interaction::PendingKind::Question,
            );
            let snapshot = child
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let mut local = snapshot.plan;
            local.links.push(PlanLink {
                id: "existing-descendant".into(),
                parent_step_id: "local-step".into(),
                child_session_id: "unsettled-descendant".into(),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
            local.waits.push(PlanWait {
                id: "existing-wait".into(),
                step_ids: vec!["local-step".into()],
                dependencies: vec![WaitDependency {
                    kind: DependencyKind::Question,
                    id: "existing-question".into(),
                }],
                condition: WaitCondition::All,
            });
            child
                .commit_local_plan(Some(snapshot.seq), local.clone())
                .await
                .unwrap();
            let change = control(child, 7, true);
            let receipt = fixture.receive(&change).await;
            fixture.consume(vec![receipt]).await;

            let (mut parent, _gateway) = build_actor().await;
            Arc::get_mut(&mut parent).unwrap().session_info.id =
                acp::SessionId::new("direct-parent");
            let mut source = LocalPlan::new(parent.session_id_string(), "parent-scope");
            source.revision = change.source_revision;
            source.steps.push(PlanStep {
                id: change.step_id.clone(),
                content: change.content.clone(),
                status: change.status,
                priority: change.priority,
                requirements: change.requirements.clone(),
                removed: change.removed,
            });
            source.links.push(PlanLink {
                id: change.link_id.clone(),
                parent_step_id: change.step_id.clone(),
                child_session_id: child.session_id_string(),
                terminal: false,
                required: true,
                pending_source_revision: Some(change.source_revision),
                review_required_revision: change.source_revision,
                result_id: None,
                dispatch_failure: None,
            });
            source.waits.push(PlanWait {
                id: "parent-wait-for-result".into(),
                step_ids: vec![change.step_id.clone()],
                dependencies: vec![WaitDependency {
                    kind: DependencyKind::Task,
                    id: change.link_id.clone(),
                }],
                condition: WaitCondition::All,
            });
            source.replans.push(change.clone());
            parent
                .commit_local_plan(None, source.clone())
                .await
                .unwrap();
            let prefix = parent
                .chat_state_handle
                .timeline_events()
                .await
                .unwrap()
                .len();
            let mut channel = install_channel(child).await;
            let handled = write(child, input(vec![handle(&[&change.id], None)], false))
                .await
                .unwrap();
            let SubagentEvent::Interact(request) =
                tokio::time::timeout(Duration::from_secs(2), channel.recv())
                    .await
                    .unwrap()
                    .unwrap()
            else {
                panic!("expected the real ChannelBackend acknowledgement")
            };
            let AgentInteraction::ReplanHandled {
                change_id,
                outcome,
                reply_to,
            } = request.action
            else {
                panic!("expected ReplanHandled")
            };
            assert_eq!(reply_to.source_session_id, parent.session_id_string());
            assert_eq!(reply_to.message_id, replan_message_id(&change.id));
            assert_eq!(outcome, ReplanOutcome::Stopping);
            parent
                .receive_replan_handled(&child.session_id_string(), &change_id, outcome.clone())
                .await
                .unwrap();
            let accepted_prefix = parent.chat_state_handle.timeline_events().await.unwrap();
            let stable_id = request.id.clone();
            request
                .respond_to
                .send(Ok(AgentInteractionOutput::message_unconfirmed(
                    request.id,
                    "lost_parent_ack",
                    "The parent committed but its reply was lost",
                )))
                .unwrap();
            let SubagentEvent::Interact(retry) =
                tokio::time::timeout(Duration::from_secs(2), channel.recv())
                    .await
                    .unwrap()
                    .unwrap()
            else {
                panic!("expected the same stopping acknowledgement retry")
            };
            assert_eq!(retry.id, stable_id);
            let AgentInteraction::ReplanHandled {
                change_id: retry_change,
                outcome: retry_outcome,
                reply_to: retry_reference,
            } = retry.action
            else {
                panic!("expected ReplanHandled retry")
            };
            assert_eq!(retry_change, change_id);
            assert_eq!(retry_outcome, outcome);
            assert_eq!(retry_reference, reply_to);
            parent
                .receive_replan_handled(&child.session_id_string(), &retry_change, retry_outcome)
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(parent.chat_state_handle.timeline_events().await.unwrap())
                    .unwrap(),
                serde_json::to_value(accepted_prefix).unwrap()
            );
            retry
                .respond_to
                .send(Ok(AgentInteractionOutput::message_received(
                    retry.id,
                    "parent-stopping-ack",
                )))
                .unwrap();
            if let Some(delivery) = child.planning_delivery.take() {
                tokio::time::timeout(Duration::from_secs(2), delivery)
                    .await
                    .unwrap()
                    .unwrap();
            }
            assert!(channel.try_recv().is_err());
            let result = parent
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap()
                .plan;
            assert_eq!(
                result.replans[0].progress,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Stopping
                }
            );
            assert!(!result.links[0].terminal);
            assert!(result.links[0].result_id.is_none());
            assert!(result.links[0].pending_source_revision.is_none());
            assert_eq!(result.waits, source.waits);
            assert_eq!(result.reviews, source.reviews);
            assert_eq!(result.unresolved_required_deliveries().len(), 1);
            let parent_events = parent.chat_state_handle.timeline_events().await.unwrap();
            let progress = parent_events[prefix..]
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Planning(event) => {
                        Some(event.plan.replans[0].progress.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                progress,
                [
                    ReplanProgress::Received,
                    ReplanProgress::Consumed,
                    ReplanProgress::Handled {
                        outcome: ReplanOutcome::Stopping
                    }
                ]
            );
            assert_eq!(handled.links, local.links);
            assert_eq!(handled.waits, local.waits);
            assert_eq!(handled.reviews, local.reviews);
            assert_eq!(handled.steps, local.steps);
            let blockers = child.planning_completion_blockers().await.unwrap();
            assert!(
                blockers
                    .iter()
                    .any(|blocker| blocker.contains("existing-descendant"))
            );
            assert!(
                blockers
                    .iter()
                    .any(|blocker| blocker.contains("existing-wait"))
            );
            assert!(
                child
                    .pending_interactions
                    .lock()
                    .unwrap()
                    .contains_key("existing-question")
            );
            assert!(
                child
                    .state
                    .lock()
                    .await
                    .foreground
                    .regular()
                    .is_some_and(|task| !task.is_finished())
            );
            assert!(!child.background_service_shutdown.is_cancelled());
        })
        .await;
}
