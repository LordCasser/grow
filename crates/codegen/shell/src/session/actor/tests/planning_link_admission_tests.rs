use super::super::{SessionActor, TerminationState};
use super::support::{
    create_test_actor_with_planning_receiver, install_test_foreground, start_test_planning_driver,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tool_types::planning::{
    LocalPlan, PlanLink, PlanPriority, PlanStep, PlanStepStatus, ReplanProgress,
};
use tools::implementations::grow_build::todo::{
    TodoPriority, TodoState, TodoStatus, TodoUpdate, TodoWriteInput, project_todo_state,
    runtime::{LocalPlanProjection, PlanningAction, PlanningCommand, PlanningRuntimeHandle},
};
use tools::types::resources::State;

const PROMPT: &str = "planning-link-turn";
const STEP: &str = " step-α ";
type PlanningReply = oneshot::Receiver<Result<LocalPlan, String>>;

struct HeldPlanningAck {
    event: chat_state::TimelineEvent,
    respond_to: oneshot::Sender<std::io::Result<()>>,
}

#[derive(Clone, Default)]
struct PlanningPersistenceProbe {
    events: Arc<Mutex<Vec<chat_state::TimelineEvent>>>,
    hold_next: Arc<AtomicBool>,
}

struct PlanningPersistence {
    probe: PlanningPersistenceProbe,
    held: mpsc::UnboundedSender<HeldPlanningAck>,
}

impl chat_state::TimelinePersistence for PlanningPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> oneshot::Receiver<std::io::Result<()>> {
        let (respond_to, response) = oneshot::channel();
        let mut events = self.probe.events.lock().expect("durable test Timeline");
        if !events.iter().any(|stored| stored.seq == event.seq) {
            events.push(event.clone());
        }
        drop(events);
        if matches!(&event.kind, chat_state::TimelineEventKind::Planning(_))
            && self.probe.hold_next.swap(false, Ordering::AcqRel)
        {
            self.held
                .send(HeldPlanningAck {
                    event: event.clone(),
                    respond_to,
                })
                .unwrap_or_else(|_| panic!("controlled planning ACK receiver must stay alive"));
        } else {
            let _ = respond_to.send(Ok(()));
        }
        response
    }

    fn flush(&mut self) {}
}

struct Fixture {
    actor: Arc<SessionActor>,
    gateway: mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    probe: PlanningPersistenceProbe,
    held: mpsc::UnboundedReceiver<HeldPlanningAck>,
    chat_cancellation: tokio_util::sync::CancellationToken,
    _presentation: mpsc::UnboundedReceiver<crate::session::persistence::PersistenceMsg>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.actor.background_service_shutdown.cancel();
        self.chat_cancellation.cancel();
    }
}

async fn fixture_from_timeline(
    restored: Option<Vec<chat_state::TimelineEvent>>,
) -> (Fixture, mpsc::UnboundedReceiver<PlanningCommand>) {
    let (gateway_tx, gateway) = mpsc::unbounded_channel();
    let (persistence_tx, presentation) = mpsc::unbounded_channel();
    let (mut actor, commands) =
        create_test_actor_with_planning_receiver(0, 256_000, 85, gateway_tx, persistence_tx).await;
    let events = match restored {
        Some(events) => events,
        None => actor.chat_state_handle.timeline_events().await.unwrap(),
    };
    let sampling = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let probe = PlanningPersistenceProbe {
        events: Arc::new(Mutex::new(events.clone())),
        ..Default::default()
    };
    let (held_tx, held) = mpsc::unbounded_channel();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let chat_cancellation = tokio_util::sync::CancellationToken::new();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
        events,
        sampling,
        Box::new(PlanningPersistence {
            probe: probe.clone(),
            held: held_tx,
        }),
        event_tx,
        chat_cancellation.clone(),
    )
    .await
    .expect("reconstruct the real ChatState owner from its durable prefix");
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    let actor = Arc::new(actor);
    install_test_foreground(&actor, PROMPT).await;
    let bridge = actor.agent.borrow().tool_bridge().clone();
    actor.bind_local_planning_runtime(&bridge).await.unwrap();
    (
        Fixture {
            actor,
            gateway,
            probe,
            held,
            chat_cancellation,
            _presentation: presentation,
        },
        commands,
    )
}

async fn driven_fixture() -> Fixture {
    let (fixture, commands) = fixture_from_timeline(None).await;
    start_test_planning_driver(&fixture.actor, commands);
    fixture
}

async fn reply(response: PlanningReply) -> Result<LocalPlan, String> {
    tokio::time::timeout(Duration::from_secs(2), response)
        .await
        .expect("planning command must settle")
        .expect("planning owner must answer")
}

fn step_update(status: TodoStatus, removed: bool) -> TodoUpdate {
    TodoUpdate {
        id: STEP.into(),
        content: Some("Build the change".into()),
        status: Some(status),
        priority: Some(TodoPriority::High),
        requirements: Some(vec!["Verify the original acceptance requirement".into()]),
        removed: Some(removed),
    }
}

fn write_input(update: TodoUpdate) -> TodoWriteInput {
    TodoWriteInput {
        todos: vec![update],
        merge: true,
        actions: Vec::new(),
    }
}

impl Fixture {
    async fn send(&self, command: PlanningCommand) {
        let bridge = self.actor.agent.borrow().tool_bridge().clone();
        bridge
            .read_resource::<PlanningRuntimeHandle>()
            .await
            .expect("real session planning mailbox")
            .0
            .send(command)
            .expect("planning command receiver must stay alive");
    }

    async fn queue_link(
        &self,
        prompt_id: &str,
        step_id: &str,
        task_id: &str,
        required: bool,
    ) -> PlanningReply {
        let (respond_to, response) = oneshot::channel();
        self.send(PlanningCommand::Link {
            prompt_id: prompt_id.into(),
            step_id: step_id.into(),
            task_id: task_id.into(),
            required,
            respond_to,
        })
        .await;
        response
    }

    async fn queue_write(&self, input: TodoWriteInput) -> PlanningReply {
        let (respond_to, response) = oneshot::channel();
        self.send(PlanningCommand::Write {
            prompt_id: PROMPT.into(),
            input,
            respond_to,
        })
        .await;
        response
    }

    async fn write(&self, input: TodoWriteInput) -> LocalPlan {
        reply(self.queue_write(input).await)
            .await
            .expect("the admitted parent write must commit")
    }

    async fn projection(&self) -> serde_json::Value {
        let bridge = self.actor.agent.borrow().tool_bridge().clone();
        let resources = bridge.shared_resources().await;
        let resources = resources.lock().await;
        serde_json::json!({
            "persisted": resources.serialize(),
            "plan": resources.get::<LocalPlanProjection>().map(|plan| &plan.0),
            "todo": resources.get::<State<TodoState>>(),
        })
    }

    async fn observables(&self) -> serde_json::Value {
        serde_json::json!({
            "timeline": self.actor.chat_state_handle.timeline_events().await.unwrap(),
            "surface": self.actor.chat_state_handle.get_conversation_with_revision().await.unwrap(),
            "projection": self.projection().await,
            "durable_prefix": self.probe.events.lock().unwrap().clone(),
        })
    }

    fn drain_gateway(&mut self) {
        while self.gateway.try_recv().is_ok() {}
    }

    async fn reject_link(
        &mut self,
        prompt_id: &str,
        step_id: &str,
        task_id: &str,
        required: bool,
    ) -> String {
        self.drain_gateway();
        let before = self.observables().await;
        let error = reply(self.queue_link(prompt_id, step_id, task_id, required).await)
            .await
            .expect_err("this link must be rejected");
        assert_eq!(self.observables().await, before, "{error}");
        assert!(matches!(
            self.gateway.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        error
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pending_and_in_progress_bind_exact_step_with_either_delivery_obligation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for status in [TodoStatus::Pending, TodoStatus::InProgress] {
                for required in [false, true] {
                    let fixture = driven_fixture().await;
                    let before = fixture.write(write_input(step_update(status, false))).await;
                    let previous = fixture
                        .actor
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap();
                    let mut expected = before.clone();
                    expected.links.push(PlanLink {
                        id: "task-exact".into(),
                        parent_step_id: STEP.into(),
                        child_session_id: "task-exact".into(),
                        terminal: false,
                        required,
                        pending_source_revision: None,
                        review_required_revision: before.revision,
                        result_id: None,
                        dispatch_failure: None,
                    });
                    let linked = reply(
                        fixture
                            .queue_link(PROMPT, STEP, "task-exact", required)
                            .await,
                    )
                    .await
                    .expect("eligible current step must bind");
                    assert_eq!(linked, expected);
                    let committed = fixture
                        .actor
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(committed.seq > previous.seq);
                    assert_eq!(committed.plan, expected);
                    let bridge = fixture.actor.agent.borrow().tool_bridge().clone();
                    assert_eq!(
                        bridge
                            .read_resource::<LocalPlanProjection>()
                            .await
                            .unwrap()
                            .0,
                        Some(expected.clone())
                    );
                    assert_eq!(
                        serde_json::to_value(
                            bridge.read_resource::<State<TodoState>>().await.unwrap()
                        )
                        .unwrap(),
                        serde_json::to_value(State(project_todo_state(&expected))).unwrap()
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ineligible_removed_missing_and_nonexact_steps_leave_facts_and_projections_unchanged() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (status, removed) in [
                (TodoStatus::Completed, false),
                (TodoStatus::Cancelled, false),
                (TodoStatus::Pending, true),
                (TodoStatus::InProgress, true),
                (TodoStatus::Completed, true),
                (TodoStatus::Cancelled, true),
            ] {
                for required in [false, true] {
                    let mut fixture = driven_fixture().await;
                    fixture
                        .write(write_input(step_update(status, removed)))
                        .await;
                    fixture
                        .reject_link(PROMPT, STEP, "rejected-task", required)
                        .await;
                }
            }
            let mut fixture = driven_fixture().await;
            fixture
                .write(write_input(step_update(TodoStatus::Pending, false)))
                .await;
            for id in ["missing-step", "step-α", "", " "] {
                fixture
                    .reject_link(PROMPT, id, "rejected-task", false)
                    .await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn missing_or_foreign_canonical_plan_cannot_bind_or_publish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = driven_fixture().await;
            assert!(
                fixture
                    .reject_link(PROMPT, STEP, "task-missing", false)
                    .await
                    .contains("create the local parent step")
            );
            let mut foreign = LocalPlan::new("different-session", "scope:foreign");
            foreign.steps.push(PlanStep {
                id: STEP.into(),
                content: "Foreign work".into(),
                priority: PlanPriority::High,
                status: PlanStepStatus::Pending,
                requirements: None,
                removed: false,
            });
            fixture
                .actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(foreign),
                    },
                ))
                .await
                .unwrap();
            fixture.actor.hydrate_local_plan().await.unwrap();
            for required in [false, true] {
                assert!(
                    fixture
                        .reject_link(PROMPT, STEP, "task-foreign", required)
                        .await
                        .contains("different session")
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unavailable_canonical_owner_does_not_publish_or_append_a_link() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = driven_fixture().await;
            fixture
                .write(write_input(step_update(TodoStatus::Pending, false)))
                .await;
            fixture.drain_gateway();
            let projection = fixture.projection().await;
            let events = fixture.probe.events.lock().unwrap().clone();
            fixture.chat_cancellation.cancel();
            assert!(
                fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .is_err()
            );
            assert!(
                reply(
                    fixture
                        .queue_link(PROMPT, STEP, "task-unavailable", false)
                        .await
                )
                .await
                .is_err()
            );
            assert_eq!(fixture.projection().await, projection);
            assert_eq!(
                serde_json::to_value(&*fixture.probe.events.lock().unwrap()).unwrap(),
                serde_json::to_value(&events).unwrap()
            );
            assert!(matches!(
                fixture.gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn link_uses_current_owner_facts_instead_of_resource_projection_status() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = driven_fixture().await;
            let canonical = fixture
                .write(write_input(step_update(TodoStatus::Pending, false)))
                .await;
            let bridge = fixture.actor.agent.borrow().tool_bridge().clone();
            let mut stale = canonical.clone();
            stale.scope_id = "scope:stale-resource".into();
            stale.steps[0].status = PlanStepStatus::Completed;
            bridge
                .update_resource(LocalPlanProjection(Some(stale.clone())))
                .await;
            bridge
                .update_resource(State(project_todo_state(&stale)))
                .await;
            let linked = reply(
                fixture
                    .queue_link(PROMPT, STEP, "task-current", false)
                    .await,
            )
            .await
            .unwrap();
            assert_eq!(linked.scope_id, canonical.scope_id);
            assert_eq!(linked.steps, canonical.steps);

            let completed = fixture
                .write(write_input(step_update(TodoStatus::Completed, false)))
                .await;
            let mut stale = completed;
            stale.steps[0].status = PlanStepStatus::Pending;
            bridge
                .update_resource(LocalPlanProjection(Some(stale.clone())))
                .await;
            bridge
                .update_resource(State(project_todo_state(&stale)))
                .await;
            fixture
                .reject_link(PROMPT, STEP, "task-stale-projection", true)
                .await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn ended_preempted_or_wrong_prompt_cannot_bind_after_waiting_for_gate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for state in [
                "idle",
                "settling",
                "graceful",
                "fatal",
                "preempted",
                "suspended",
                "wrong-prompt",
            ] {
                let mut fixture = driven_fixture().await;
                fixture
                    .write(write_input(step_update(TodoStatus::Pending, false)))
                    .await;
                fixture.drain_gateway();
                let before = fixture.observables().await;
                let gate = fixture.actor.step_control_gate.lock().await;
                let response = fixture
                    .queue_link(
                        if state == "wrong-prompt" {
                            "old-turn"
                        } else {
                            PROMPT
                        },
                        STEP,
                        "task-after-turn",
                        false,
                    )
                    .await;
                {
                    let mut admission = fixture.actor.state.lock().await;
                    match state {
                        "idle" => admission.foreground.take_regular().unwrap().handle.abort(),
                        "settling" => admission
                            .foreground
                            .begin_settling()
                            .unwrap()
                            .handle
                            .abort(),
                        "graceful" => admission.termination.request(TerminationState::Graceful),
                        "fatal" => admission.termination.request(TerminationState::Fatal),
                        "preempted" => admission.terminal_preemption_pending = true,
                        "suspended" => admission.runtime_suspend.store(true, Ordering::SeqCst),
                        "wrong-prompt" => {}
                        _ => unreachable!(),
                    }
                }
                drop(gate);
                assert!(reply(response).await.unwrap_err().contains("admitted turn"));
                assert_eq!(fixture.observables().await, before, "{state}");
                assert!(matches!(
                    fixture.gateway.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_and_self_task_ids_are_rejected_but_distinct_tasks_can_share_a_step() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = driven_fixture().await;
            fixture
                .write(write_input(step_update(TodoStatus::Pending, false)))
                .await;
            let first = reply(fixture.queue_link(PROMPT, STEP, "task-first", true).await)
                .await
                .unwrap();
            for required in [false, true] {
                assert!(
                    fixture
                        .reject_link(PROMPT, STEP, "task-first", required)
                        .await
                        .contains("already bound")
                );
                let own_id = fixture.actor.session_id_string();
                assert!(
                    fixture
                        .reject_link(PROMPT, STEP, &own_id, required)
                        .await
                        .contains("already bound")
                );
            }
            let second = reply(fixture.queue_link(PROMPT, STEP, "task-second", false).await)
                .await
                .unwrap();
            assert_eq!(second.links.len(), 2);
            assert_eq!(second.links[0], first.links[0]);
            assert_eq!(second.links[1].parent_step_id, STEP);
            assert_eq!(second.steps, first.steps);
            assert_eq!(second.revision, first.revision);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn link_waits_for_durable_ack_before_reply_or_resource_publication() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = driven_fixture().await;
            let before = fixture
                .write(write_input(step_update(TodoStatus::InProgress, false)))
                .await;
            fixture.drain_gateway();
            let projection = fixture.projection().await;
            let event_count = fixture.probe.events.lock().unwrap().len();
            fixture.probe.hold_next.store(true, Ordering::Release);
            let mut response = fixture.queue_link(PROMPT, STEP, "task-held", true).await;
            let held = tokio::time::timeout(Duration::from_secs(2), fixture.held.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(
                response.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert_eq!(fixture.projection().await, projection);
            assert!(matches!(
                fixture.gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(fixture.actor.step_control_gate.try_lock().is_err());
            let chat_state::TimelineEventKind::Planning(prepared) = &held.event.kind else {
                panic!("only the actual Link planning commit may be held");
            };
            assert_eq!(prepared.plan.steps, before.steps);
            assert_eq!(prepared.plan.links.len(), 1);
            assert_eq!(prepared.plan.links[0].id, "task-held");
            held.respond_to.send(Ok(())).unwrap();
            let linked = reply(response).await.unwrap();
            assert_eq!(linked, *prepared.plan);
            assert_eq!(
                fixture
                    .actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan,
                linked
            );
            assert_eq!(
                fixture.probe.events.lock().unwrap().len(),
                event_count + 1,
                "Link uses the existing single durable planning transaction"
            );
            let bridge = fixture.actor.agent.borrow().tool_bridge().clone();
            assert_eq!(
                bridge
                    .read_resource::<LocalPlanProjection>()
                    .await
                    .unwrap()
                    .0,
                Some(linked)
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn lost_link_ack_is_unconfirmed_without_projection_and_retains_one_cold_binding() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = driven_fixture().await;
            fixture
                .write(write_input(step_update(TodoStatus::Pending, false)))
                .await;
            fixture.drain_gateway();
            let projection = fixture.projection().await;
            fixture.probe.hold_next.store(true, Ordering::Release);
            let response = fixture
                .queue_link(PROMPT, STEP, "task-unconfirmed", false)
                .await;
            let held = tokio::time::timeout(Duration::from_secs(2), fixture.held.recv())
                .await
                .unwrap()
                .unwrap();
            drop(held.respond_to);
            fixture.chat_cancellation.cancel();
            assert!(reply(response).await.unwrap_err().contains("unconfirmed"));
            assert_eq!(fixture.projection().await, projection);
            assert!(matches!(
                fixture.gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            let events = fixture.probe.events.lock().unwrap().clone();
            drop(fixture);
            let (mut restored, commands) = fixture_from_timeline(Some(events)).await;
            start_test_planning_driver(&restored.actor, commands);
            restored.actor.hydrate_local_plan().await.unwrap();
            let plan = restored
                .actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap()
                .plan;
            assert_eq!(plan.links.len(), 1);
            assert_eq!(plan.links[0].id, "task-unconfirmed");
            assert!(!plan.links[0].terminal);
            assert_eq!(plan.links[0].result_id, None);
            restored
                .reject_link(PROMPT, STEP, "task-unconfirmed", false)
                .await;
        })
        .await;
}

fn edit_input(kind: &str) -> TodoWriteInput {
    let mut update = TodoUpdate {
        id: STEP.into(),
        content: None,
        status: None,
        priority: None,
        requirements: None,
        removed: None,
    };
    match kind {
        "completed" => update.status = Some(TodoStatus::Completed),
        "cancelled" => update.status = Some(TodoStatus::Cancelled),
        "removed" => update.removed = Some(true),
        "intent" => {
            update.content = Some("Latest parent objective".into());
            update.requirements = Some(vec!["Verify the latest acceptance requirement".into()]);
            update.priority = Some(TodoPriority::Low);
        }
        _ => unreachable!(),
    }
    write_input(update)
}

#[tokio::test(flavor = "current_thread")]
async fn gate_orders_link_with_completion_cancellation_removal_and_intent_edits() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for kind in ["completed", "cancelled", "removed", "intent"] {
                for edit_first in [true, false] {
                    let (fixture, mut commands) = fixture_from_timeline(None).await;
                    let seed = fixture
                        .queue_write(write_input(step_update(TodoStatus::InProgress, false)))
                        .await;
                    fixture
                        .actor
                        .handle_planning_command(commands.recv().await.unwrap())
                        .await;
                    let original = reply(seed).await.unwrap();
                    let event_count = fixture.probe.events.lock().unwrap().len();
                    let write_response = fixture.queue_write(edit_input(kind)).await;
                    let write_command = commands.recv().await.unwrap();
                    let link_response = fixture.queue_link(PROMPT, STEP, "task-race", false).await;
                    let link_command = commands.recv().await.unwrap();
                    let gate = fixture.actor.step_control_gate.lock().await;
                    let write = fixture.actor.handle_planning_command(write_command);
                    let link = fixture.actor.handle_planning_command(link_command);
                    tokio::pin!(write, link);
                    // Poll the actual mailbox operations at the fair production gate.
                    // No sleep or hand-built planning maps determine commit order.
                    if edit_first {
                        assert!(futures::poll!(write.as_mut()).is_pending());
                        assert!(futures::poll!(link.as_mut()).is_pending());
                    } else {
                        assert!(futures::poll!(link.as_mut()).is_pending());
                        assert!(futures::poll!(write.as_mut()).is_pending());
                    }
                    drop(gate);
                    tokio::join!(write, link);
                    let written = reply(write_response).await.unwrap();
                    let linked = reply(link_response).await;
                    let committed = fixture
                        .actor
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap();
                    if edit_first && kind != "intent" {
                        assert!(linked.is_err(), "{kind} must be visible to Link admission");
                        assert!(written.links.is_empty());
                        assert!(written.replans.is_empty());
                        assert_eq!(committed.plan, written);
                        assert_eq!(fixture.probe.events.lock().unwrap().len(), event_count + 1);
                        continue;
                    }
                    let linked = linked.expect("this ordering still has an eligible step");
                    assert_eq!(linked.links.len(), 1);
                    assert_eq!(linked.links[0].parent_step_id, STEP);
                    assert!(!committed.plan.links[0].terminal);
                    assert_eq!(committed.plan.links[0].result_id, None);
                    assert_eq!(fixture.probe.events.lock().unwrap().len(), event_count + 2);
                    if edit_first {
                        assert_eq!(linked.steps, written.steps);
                        assert_eq!(linked.revision, written.revision);
                        assert_eq!(linked.links[0].review_required_revision, written.revision);
                        assert_eq!(linked.steps[0].content, "Latest parent objective");
                        assert_eq!(
                            linked.steps[0].requirements,
                            Some(vec!["Verify the latest acceptance requirement".into()])
                        );
                        assert_eq!(linked.steps[0].priority, PlanPriority::Low);
                        assert!(committed.plan.replans.is_empty());
                        assert_eq!(committed.plan, linked);
                    } else {
                        assert_eq!(linked.steps, original.steps);
                        assert_eq!(linked.revision, original.revision);
                        assert_eq!(committed.plan, written);
                        if kind == "completed" {
                            assert!(
                                written.replans.is_empty(),
                                "completion alone does not stop the linked execution"
                            );
                            assert_eq!(written.links[0], linked.links[0]);
                        } else {
                            assert_eq!(written.replans.len(), 1);
                            let change = &written.replans[0];
                            assert_eq!(change.link_id, "task-race");
                            assert_eq!(change.child_session_id, "task-race");
                            assert_eq!(change.step_id, STEP);
                            assert_eq!(change.source_revision, written.revision);
                            assert_eq!(change.progress, ReplanProgress::Pending);
                            assert_eq!(change.is_stop(), kind != "intent");
                            assert_eq!(
                                written.links[0].pending_source_revision,
                                Some(change.source_revision)
                            );
                            if kind == "intent" {
                                assert_eq!(change.content, written.steps[0].content);
                                assert_eq!(change.requirements, written.steps[0].requirements);
                                assert_eq!(change.priority, written.steps[0].priority);
                            }
                        }
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cold_current_custom_scope_rejects_historical_step_and_survives_new_regular_turn() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = driven_fixture().await;
            let old = fixture
                .write(write_input(step_update(TodoStatus::Pending, false)))
                .await;
            let selected = fixture
                .write(TodoWriteInput {
                    merge: true,
                    todos: Vec::new(),
                    actions: vec![PlanningAction::SelectScope {
                        scope_id: "scope:custom-β".into(),
                    }],
                })
                .await;
            assert_ne!(selected.scope_id, old.scope_id);
            assert!(selected.steps.is_empty());
            let mut current = step_update(TodoStatus::InProgress, false);
            current.id = "current-custom-step".into();
            let current = fixture.write(write_input(current)).await;
            let events = fixture.probe.events.lock().unwrap().clone();
            drop(fixture);
            let (mut restored, commands) = fixture_from_timeline(Some(events)).await;
            start_test_planning_driver(&restored.actor, commands);
            restored.actor.hydrate_local_plan().await.unwrap();
            {
                let gate = restored.actor.step_control_gate.lock().await;
                let mut state = restored.actor.state.lock().await;
                state.foreground.begin_settling().unwrap().handle.abort();
                assert!(state.foreground.finish_settling(PROMPT));
                drop(state);
                drop(gate);
            }
            install_test_foreground(&restored.actor, "followup-turn").await;
            restored
                .reject_link("followup-turn", STEP, "task-old-scope", false)
                .await;
            let linked = reply(
                restored
                    .queue_link(
                        "followup-turn",
                        "current-custom-step",
                        "task-current-scope",
                        true,
                    )
                    .await,
            )
            .await
            .unwrap();
            assert_eq!(linked.scope_id, "scope:custom-β");
            assert_eq!(linked.revision, current.revision);
            assert_eq!(linked.steps, current.steps);
            assert_eq!(linked.links[0].parent_step_id, "current-custom-step");
        })
        .await;
}
