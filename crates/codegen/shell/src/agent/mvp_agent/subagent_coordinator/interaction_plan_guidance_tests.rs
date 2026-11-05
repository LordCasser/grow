//! Runner provenance checks through real source planning facts and the target mailbox.

use super::*;
use crate::agent::mvp_agent::tests::{make_test_handle_with_receiver, run_local_for_bridge_test};
use crate::session::actor::{SessionActor, tests::support::create_test_actor_ex};
use acp_transport::protocol as acp;
use chat_state::timeline::PlanningEvent;
use chat_state::{
    ChatStateActor, ChatStateEvent, MockPersistenceReceiver, MockTimelinePersistence,
    TimelineEventKind,
};
use std::future::Future as _;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tool_types::{
    LocalPlan, PlanLink, PlanPriority, PlanStep, PlanStepStatus, PlanUpdateMode, ReplanChange,
    ReplanOutcome, ReplanProgress,
};

type Response = oneshot::Receiver<Result<AgentInteractionOutput, String>>;

struct Source {
    handle: SessionHandle,
    commands: mpsc::UnboundedReceiver<SessionCommand>,
    persistence: MockPersistenceReceiver,
    cancellation: CancellationToken,
    _events: mpsc::UnboundedReceiver<ChatStateEvent>,
}

impl Source {
    async fn new(plan: Option<LocalPlan>) -> Self {
        let (persistence, records) = MockTimelinePersistence::new();
        let (events, event_rx) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        let chat = ChatStateActor::spawn(
            Vec::new(),
            sampling_config(),
            Box::new(persistence),
            events,
            cancellation.clone(),
        );
        let (mut handle, commands) = make_test_handle_with_receiver("test/default", None);
        handle.info.id = acp::SessionId::new("parent");
        handle.chat_state_handle = chat;
        let mut source = Self {
            handle,
            commands,
            persistence: records,
            cancellation,
            _events: event_rx,
        };
        if let Some(plan) = plan {
            source.commit(plan).await;
        }
        source.persistence.drain();
        source
    }

    async fn commit(&mut self, plan: LocalPlan) {
        let previous = self
            .handle
            .chat_state_handle
            .planning_state()
            .await
            .unwrap();
        self.handle
            .chat_state_handle
            .record_timeline_event_durably(TimelineEventKind::Planning(PlanningEvent {
                previous: previous.map(|snapshot| snapshot.seq),
                plan: Box::new(plan),
            }))
            .await
            .expect("the real source owner commits its canonical planning fact");
    }

    async fn assert_unchanged(&mut self, before: &serde_json::Value) {
        assert_eq!(&observables(&self.handle).await, before);
        assert!(
            self.persistence.drain().is_empty(),
            "source reads cannot append facts"
        );
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
}

struct Target {
    actor: SessionActor,
    handle: SessionHandle,
    commands: mpsc::UnboundedReceiver<SessionCommand>,
    gateway: mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    _persistence: mpsc::UnboundedReceiver<crate::session::persistence::PersistenceMsg>,
}

impl Target {
    async fn new() -> Self {
        let (gateway_tx, gateway) = mpsc::unbounded_channel();
        let (persistence_tx, persistence) = mpsc::unbounded_channel();
        let (mut actor, _events) =
            create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
        actor.session_info.id = acp::SessionId::new("child");
        let (mut handle, commands) = make_test_handle_with_receiver("test/default", None);
        handle.info = actor.session_info.clone();
        handle.chat_state_handle = actor.chat_state_handle.clone();
        Self {
            actor,
            handle,
            commands,
            gateway,
            _persistence: persistence,
        }
    }

    async fn observables(&self) -> serde_json::Value {
        serde_json::json!({
            "timeline": self.actor.chat_state_handle.timeline_events().await.unwrap(),
            "surface": self.actor.chat_state_handle.get_conversation_with_revision().await.unwrap(),
        })
    }

    async fn assert_no_effects(&mut self, before: &serde_json::Value) {
        assert_eq!(&self.observables().await, before);
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(matches!(
            self.gateway.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
}

fn sampling_config() -> sampling_types::SamplingConfig {
    serde_json::from_value(serde_json::json!({
        "base_url": "http://localhost", "model": "test", "context_window": 256_000,
    }))
    .unwrap()
}

async fn observables(handle: &SessionHandle) -> serde_json::Value {
    serde_json::json!({
        "timeline": handle.chat_state_handle.timeline_events().await.unwrap(),
        "surface": handle.chat_state_handle.get_conversation_with_revision().await.unwrap(),
    })
}

fn claimed_plan(stop: bool) -> LocalPlan {
    let mut plan = LocalPlan::new("parent", "work");
    let mut step = PlanStep {
        id: "step-1".into(),
        content: "original objective".into(),
        priority: PlanPriority::Normal,
        status: PlanStepStatus::InProgress,
        requirements: Some(vec!["original acceptance".into()]),
        removed: false,
    };
    plan.update_steps(PlanUpdateMode::Merge, vec![step.clone()])
        .unwrap();
    plan.links.push(PlanLink {
        id: "task-1".into(),
        parent_step_id: step.id.clone(),
        child_session_id: "child".into(),
        terminal: false,
        required: true,
        pending_source_revision: None,
        review_required_revision: plan.revision,
        result_id: None,
        dispatch_failure: None,
    });
    step.content = "claimed payload\n完整 source facts".into();
    step.requirements = Some(vec![
        "retain completed work".into(),
        "report unresolved results".into(),
    ]);
    step.priority = PlanPriority::High;
    if stop {
        step.status = PlanStepStatus::Cancelled;
    }
    plan.update_steps(PlanUpdateMode::Merge, vec![step])
        .unwrap();
    let id = plan.replans[0].id.clone();
    plan.advance_replan(&id, ReplanProgress::Dispatching)
        .unwrap();
    plan.validate().unwrap();
    plan
}

fn replan_request(change: ReplanChange) -> (AgentInteractionRequest, Response) {
    let (respond_to, response) = oneshot::channel();
    (
        AgentInteractionRequest {
            source_session_id: "parent".into(),
            target_child_id: Some("task-1".into()),
            id: "delivery-operation".into(),
            action: AgentInteraction::Replan { change },
            receipt_only: false,
            cancellation: CancellationToken::new(),
            respond_to,
        },
        response,
    )
}

fn start(
    request: AgentInteractionRequest,
    source: &Source,
    target: Option<SessionHandle>,
    target_id: &str,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_local(super::run(
        request,
        source.handle.clone(),
        target,
        target_id.into(),
        "delegated task".into(),
    ))
}

async fn answer(task: tokio::task::JoinHandle<()>, response: Response) -> AgentInteractionOutput {
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), response)
        .await
        .unwrap()
        .unwrap()
        .expect("the runner returns the existing structured message outcome")
}

fn assert_outcome(output: &AgentInteractionOutput, status: &str, code: &str) {
    assert_eq!(output.id, "delivery-operation");
    assert_eq!(output.status, status);
    assert_eq!(output.error.as_ref().unwrap().code, code);
    assert_eq!(output.receipt_id, None);
    assert_eq!(output.subagent_task_name.as_deref(), Some("delegated task"));
}

async fn accept_one(target: &mut Target, expected: &ReplanChange) {
    let command = tokio::time::timeout(Duration::from_secs(2), target.commands.recv())
        .await
        .unwrap()
        .unwrap();
    let SessionCommand::ReceivePlanGuidance {
        parent_session_id,
        change,
        respond_to,
    } = command
    else {
        panic!("only the exact planning control may enter the target mailbox")
    };
    assert_eq!(parent_session_id, "parent");
    assert_eq!(&change, expected);
    assert!(matches!(
        target.commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    respond_to
        .send(Ok("durable-target-receipt".into()))
        .unwrap();
}

#[test]
fn plan_guidance_claimed_source_sends_one_exact_control_and_waits_for_ack() {
    run_local_for_bridge_test(|| async {
        let plan = claimed_plan(false);
        let expected = plan.replans[0].clone();
        let mut source = Source::new(Some(plan)).await;
        let mut target = Target::new().await;
        let before_source = observables(&source.handle).await;
        let before_target = target.observables().await;
        let (request, response) = replan_request(expected.clone());
        let task = start(request, &source, Some(target.handle.clone()), "child");
        let command = tokio::time::timeout(Duration::from_secs(2), target.commands.recv())
            .await
            .unwrap()
            .unwrap();
        let SessionCommand::ReceivePlanGuidance {
            parent_session_id,
            change,
            respond_to,
        } = command
        else {
            panic!("expected the original guidance command")
        };
        assert_eq!(parent_session_id, "parent");
        assert_eq!(change, expected);
        assert!(
            !task.is_finished(),
            "sending alone must not claim durable receipt"
        );
        respond_to
            .send(Ok("durable-target-receipt".into()))
            .unwrap();
        let output = answer(task, response).await;
        assert_eq!(output.status, "received");
        assert_eq!(output.receipt_id.as_deref(), Some("durable-target-receipt"));
        assert_eq!(output.target_session_id.as_deref(), Some("child"));
        assert!(output.error.is_none());
        source.assert_unchanged(&before_source).await;
        target.assert_no_effects(&before_target).await;
    });
}

#[test]
fn plan_guidance_source_progress_and_reopening_keep_original_stop_retry_valid() {
    run_local_for_bridge_test(|| async {
        let mut plan = claimed_plan(true);
        let original = plan.replans[0].clone();
        let mut source = Source::new(Some(plan.clone())).await;
        let mut target = Target::new().await;
        let mut reopened = plan.steps[0].clone();
        reopened.content = "new objective for a later execution".into();
        reopened.status = PlanStepStatus::Pending;
        plan.update_steps(PlanUpdateMode::Merge, vec![reopened])
            .unwrap();
        source.commit(plan.clone()).await;
        assert_ne!(plan.steps[0].content, original.content);
        assert_eq!(plan.replans[0], original);
        for progress in [
            ReplanProgress::Dispatching,
            ReplanProgress::Received,
            ReplanProgress::Consumed,
            ReplanProgress::Handled {
                outcome: ReplanOutcome::Stopping,
            },
        ] {
            plan.advance_replan(&original.id, progress).unwrap();
            source.commit(plan.clone()).await;
            source.persistence.drain();
            let before_source = observables(&source.handle).await;
            let before_target = target.observables().await;
            let (request, response) = replan_request(original.clone());
            let task = start(request, &source, Some(target.handle.clone()), "child");
            accept_one(&mut target, &original).await;
            let output = answer(task, response).await;
            assert_eq!(output.status, "received");
            source.assert_unchanged(&before_source).await;
            target.assert_no_effects(&before_target).await;
        }
    });
}

#[test]
fn plan_guidance_missing_mismatched_or_unclaimed_source_is_rejected_without_delivery() {
    run_local_for_bridge_test(|| async {
        let mut target = Target::new().await;
        for case in [
            "no_plan",
            "plan_owner",
            "handle_owner",
            "request_owner",
            "missing_task",
            "wrong_task",
            "missing_link",
            "link_step",
            "link_child",
            "missing_change",
            "pending",
            "unapplied",
        ] {
            let mut plan = claimed_plan(false);
            let incoming = plan.replans[0].clone();
            match case {
                "plan_owner" => plan.session_id = "another-parent".into(),
                "missing_link" => {
                    plan.links.clear();
                    plan.replans.clear();
                }
                "link_step" => {
                    let mut step = plan.steps[0].clone();
                    step.id = "different-step".into();
                    plan.steps.push(step);
                    plan.links[0].parent_step_id = "different-step".into();
                    plan.replans[0].step_id = "different-step".into();
                }
                "link_child" => {
                    plan.links[0].child_session_id = "different-child".into();
                    plan.replans[0].child_session_id = "different-child".into();
                }
                "missing_change" => {
                    plan.replans.clear();
                    plan.links[0].pending_source_revision = None;
                }
                "pending" => plan.replans[0].progress = ReplanProgress::Pending,
                "unapplied" => plan
                    .mark_link_terminal("task-1", "child ended before delivery")
                    .unwrap(),
                _ => {}
            }
            let mut source = Source::new((case != "no_plan").then_some(plan)).await;
            let (mut request, response) = replan_request(incoming);
            match case {
                "handle_owner" => source.handle.info.id = acp::SessionId::new("different-handle"),
                "request_owner" => request.source_session_id = "different-caller".into(),
                "missing_task" => request.target_child_id = None,
                "wrong_task" => request.target_child_id = Some("different-task".into()),
                _ => {}
            }
            let before_source = observables(&source.handle).await;
            let before_target = target.observables().await;
            let task = start(request, &source, Some(target.handle.clone()), "child");
            let output = answer(task, response).await;
            assert_outcome(&output, "rejected", "invalid_planning_source");
            source.assert_unchanged(&before_source).await;
            target.assert_no_effects(&before_target).await;
        }
    });
}

#[test]
fn plan_guidance_every_immutable_payload_field_is_verified_before_dispatch() {
    run_local_for_bridge_test(|| async {
        let plan = claimed_plan(false);
        let original = plan.replans[0].clone();
        let mut source = Source::new(Some(plan)).await;
        let mut target = Target::new().await;
        let before_source = observables(&source.handle).await;
        let before_target = target.observables().await;
        for field in [
            "id",
            "link",
            "child",
            "step",
            "revision",
            "content",
            "requirements",
            "priority",
            "status",
            "removed",
        ] {
            let mut changed = original.clone();
            match field {
                "id" => changed.id = "forged-change".into(),
                "link" => changed.link_id = "different-task".into(),
                "child" => changed.child_session_id = "different-child".into(),
                "step" => changed.step_id = "different-step".into(),
                "revision" => changed.source_revision += 1,
                "content" => changed.content.push_str(" forged"),
                "requirements" => changed.requirements = Some(vec!["forged requirement".into()]),
                "priority" => changed.priority = PlanPriority::Low,
                "status" => changed.status = PlanStepStatus::Cancelled,
                "removed" => changed.removed = true,
                _ => unreachable!(),
            }
            let (mut request, response) = replan_request(changed.clone());
            request.target_child_id = Some(changed.link_id.clone());
            let task = start(request, &source, Some(target.handle.clone()), "child");
            let output = answer(task, response).await;
            assert_outcome(&output, "rejected", "invalid_planning_source");
            source.assert_unchanged(&before_source).await;
            target.assert_no_effects(&before_target).await;
        }
    });
}

#[test]
fn plan_guidance_cancellation_and_target_admission_boundaries_prevent_dispatch() {
    run_local_for_bridge_test(|| async {
        let plan = claimed_plan(false);
        let incoming = plan.replans[0].clone();
        let mut source = Source::new(Some(plan)).await;
        let before_source = observables(&source.handle).await;
        for case in [
            "cancelled",
            "receipt_only",
            "absent",
            "closed",
            "target_owner",
            "resolved_target",
        ] {
            let mut target = Target::new().await;
            let before_target = target.observables().await;
            let (mut request, response) = replan_request(incoming.clone());
            let mut target_handle = Some(target.handle.clone());
            let mut resolved = "child";
            let (status, code) = match case {
                "cancelled" => {
                    request.cancellation.cancel();
                    ("rejected", "cancelled_before_dispatch")
                }
                "receipt_only" => {
                    request.receipt_only = true;
                    ("rejected", "target_inactive")
                }
                "absent" => {
                    target_handle = None;
                    ("unconfirmed", "target_unavailable")
                }
                "closed" => {
                    target.commands.close();
                    ("unconfirmed", "target_unavailable")
                }
                "target_owner" => {
                    target_handle.as_mut().unwrap().info.id = acp::SessionId::new("another-child");
                    ("rejected", "invalid_planning_source")
                }
                "resolved_target" => {
                    resolved = "another-resolved-child";
                    ("rejected", "invalid_planning_source")
                }
                _ => unreachable!(),
            };
            let task = start(request, &source, target_handle, resolved);
            let output = answer(task, response).await;
            assert_outcome(&output, status, code);
            source.assert_unchanged(&before_source).await;
            target.assert_no_effects(&before_target).await;
        }
    });
}

#[test]
fn plan_guidance_source_observation_cannot_bypass_late_cancellation_or_endpoint_closure() {
    run_local_for_bridge_test(|| async {
        for case in ["cancelled", "target_closed", "source_closed"] {
            let plan = claimed_plan(false);
            let incoming = plan.replans[0].clone();
            let mut source = Source::new(Some(plan)).await;
            let mut target = Target::new().await;
            let before_target = target.observables().await;
            let committed = source
                .handle
                .chat_state_handle
                .timeline_events()
                .await
                .unwrap();
            source.cancellation.cancel();
            let (persistence, records) = MockTimelinePersistence::new_with_manual_timeline_ack();
            let (event_tx, event_rx) = mpsc::unbounded_channel();
            source.cancellation = CancellationToken::new();
            source.handle.chat_state_handle = ChatStateActor::spawn_from_timeline(
                committed,
                sampling_config(),
                Box::new(persistence),
                event_tx,
                source.cancellation.clone(),
            )
            .await
            .unwrap();
            source.persistence = records;
            source._events = event_rx;
            let chat = source.handle.chat_state_handle.clone();
            let pending_write = tokio::task::spawn_local(async move {
                chat.record_timeline_event_durably(TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "fixture".into(),
                        name: "hold_read_behind_ack".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await
            });
            let ack = source.persistence.next_timeline_ack().await.unwrap();
            let (request, response) = replan_request(incoming);
            let cancellation = request.cancellation.clone();
            let mut execution = Box::pin(super::run(
                request,
                source.handle.clone(),
                Some(target.handle.clone()),
                "child".into(),
                "delegated task".into(),
            ));
            // Poll the real runner until its source query is queued behind the durable write.
            std::future::poll_fn(|cx| {
                assert!(execution.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            match case {
                "cancelled" => cancellation.cancel(),
                "target_closed" => target.commands.close(),
                "source_closed" => source.cancellation.cancel(),
                _ => unreachable!(),
            }
            tokio::time::timeout(Duration::from_secs(2), execution)
                .await
                .unwrap();
            let output = tokio::time::timeout(Duration::from_secs(2), response)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let (status, code) = match case {
                "cancelled" => ("rejected", "cancelled_before_dispatch"),
                "target_closed" => ("unconfirmed", "target_unavailable"),
                "source_closed" => ("unconfirmed", "planning_source_unavailable"),
                _ => unreachable!(),
            };
            assert_outcome(&output, status, code);
            target.assert_no_effects(&before_target).await;
            if case == "source_closed" {
                drop(ack);
                assert!(
                    tokio::time::timeout(Duration::from_secs(2), pending_write)
                        .await
                        .unwrap()
                        .unwrap()
                        .is_err()
                );
                assert!(source.handle.chat_state_handle.is_closed());
                let (retry, response) = replan_request(claimed_plan(false).replans[0].clone());
                let task = start(retry, &source, Some(target.handle.clone()), "child");
                assert_outcome(
                    &answer(task, response).await,
                    "unconfirmed",
                    "planning_source_unavailable",
                );
                target.assert_no_effects(&before_target).await;
            } else {
                ack.send(Ok(())).unwrap();
                tokio::time::timeout(Duration::from_secs(2), pending_write)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let events = source
                    .handle
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(event.kind, TimelineEventKind::Observation(_)))
                        .count(),
                    1
                );
            }
        }
    });
}
