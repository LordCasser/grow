//! Exercise the Shell trait handler with resident handles and real ChatState owners.
use super::super::ShellChildRunner;
use crate::agent::mvp_agent::tests::{
    build_minimal_agent_for_tests, make_test_handle_with_receiver, run_local_for_bridge_test,
};
use crate::agent::mvp_agent::{LocalRef, MvpAgent};
use crate::session::actor::{SessionActor, tests::support::create_test_actor_ex};
use crate::session::{SessionCommand, SessionHandle};
use acp_transport::protocol as acp;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tool_types::planning::{
    DeliveryReview, DependencyKind, LocalPlan, PlanDelegation, PlanLink, PlanPriority, PlanStep,
    PlanStepStatus, PlanUpdateMode, PlanWait, PlanningReadSnapshot, ReviewDecision, WaitCondition,
    WaitDependency,
};
use tools::implementations::grow_build::task::coordinator::ChildRunner;
use tools::implementations::grow_build::task::parent_planning::{
    ParentPlanningReadError, ParentPlanningRequest,
};
use tools::implementations::grow_build::task::types::CurrentPromptIdResource;
use tools::implementations::grow_build::todo::{
    TodoState, project_todo_state, runtime::LocalPlanProjection,
};
use tools::types::resources::State;

type ReadResult = Result<Option<PlanningReadSnapshot>, ParentPlanningReadError>;

struct Fixture {
    actor: SessionActor,
    handle: SessionHandle,
    commands: mpsc::UnboundedReceiver<SessionCommand>,
    gateway: mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    _persistence: mpsc::UnboundedReceiver<crate::session::persistence::PersistenceMsg>,
}

impl Fixture {
    async fn new(session_id: &str) -> Self {
        let (gateway_tx, gateway) = mpsc::unbounded_channel();
        let (persistence_tx, persistence) = mpsc::unbounded_channel();
        let (mut actor, _events) =
            create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
        actor.session_info.id = acp::SessionId::new(session_id);
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

    async fn resources(&self) -> serde_json::Value {
        let bridge = self.actor.agent.borrow().tool_bridge().clone();
        let resources = bridge.shared_resources().await;
        let resources = resources.lock().await;
        serde_json::json!({
            "persisted": resources.serialize(),
            "plan_projection": resources.get::<LocalPlanProjection>().map(|plan| &plan.0),
            "todo_state": resources.get::<State<TodoState>>(),
            "prompt": resources.get::<CurrentPromptIdResource>().map(|prompt| &prompt.0),
        })
    }

    async fn observables(&self) -> serde_json::Value {
        serde_json::json!({
            "timeline": self.actor.chat_state_handle.timeline_events().await.unwrap(),
            "surface": self.actor.chat_state_handle.get_conversation_with_revision().await.unwrap(),
            "resources": self.resources().await,
        })
    }

    fn assert_no_publication(&mut self) {
        assert!(matches!(
            self.gateway.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
}

fn start_read(
    agent: &MvpAgent,
    source: &str,
    parent: &str,
    task: &str,
    step: Option<&str>,
    cancellation: CancellationToken,
) -> oneshot::Receiver<ReadResult> {
    let (respond_to, response) = oneshot::channel();
    let runner = ShellChildRunner {
        agent_ref: LocalRef::new(agent),
    };
    // Invoke the production trait entry, including its real MvpAgent lookup.
    runner.read_parent_plan(
        ParentPlanningRequest {
            source_session_id: source.into(),
            cancellation,
            respond_to,
        },
        parent.into(),
        task.into(),
        step.map(str::to_owned),
    );
    response
}

async fn answer(response: oneshot::Receiver<ReadResult>) -> ReadResult {
    tokio::time::timeout(Duration::from_secs(6), response)
        .await
        .expect("the Shell handler must honor its five-second deadline")
        .expect("the Shell handler must return its typed observation")
}

fn complete_plan(session_id: &str, child_session_id: &str) -> LocalPlan {
    let mut plan = LocalPlan::new(session_id, "scope:direct-parent");
    plan.revision = 7;
    for index in 0..17 {
        let step_id = format!("step-{index}");
        let child_id = if index == 16 {
            child_session_id.to_owned()
        } else {
            format!("other-child-{index}")
        };
        plan.steps.push(PlanStep {
            id: step_id.clone(),
            content: format!("Full step {index}: {}", "完整 decision facts ".repeat(32)),
            priority: PlanPriority::High,
            status: if index == 16 {
                PlanStepStatus::Cancelled
            } else {
                PlanStepStatus::InProgress
            },
            requirements: Some(vec![
                "Retain the complete acceptance requirements. ".repeat(16),
            ]),
            removed: index == 16,
        });
        plan.links.push(PlanLink {
            id: format!("task-{index}"),
            parent_step_id: step_id.clone(),
            child_session_id: child_id.clone(),
            terminal: index != 0,
            required: true,
            pending_source_revision: None,
            review_required_revision: plan.revision,
            result_id: (index != 0).then(|| {
                serde_json::to_string(&chat_state::TimelineRangeRef {
                    timeline_id: child_id,
                    first_seq: 101,
                    last_seq: 101,
                })
                .unwrap()
            }),
            dispatch_failure: None,
        });
        plan.waits.push(PlanWait {
            id: format!("wait-{index}"),
            step_ids: vec![step_id],
            condition: WaitCondition::All,
            dependencies: vec![WaitDependency {
                kind: DependencyKind::Task,
                id: format!("task-{index}"),
            }],
        });
    }
    plan.links.push(PlanLink {
        id: "same-step-retry".into(),
        parent_step_id: "step-16".into(),
        child_session_id: "other-same-step".into(),
        terminal: false,
        required: false,
        pending_source_revision: None,
        review_required_revision: plan.revision,
        result_id: None,
        dispatch_failure: None,
    });
    let mut changed = plan.steps[0].clone();
    changed.content.push_str(" Latest owner requirement.");
    plan.update_steps(PlanUpdateMode::Merge, vec![changed])
        .unwrap();
    for link in plan.links.iter().filter(|link| link.terminal) {
        plan.reviews.push(DeliveryReview {
            link_id: link.id.clone(),
            result_id: link.result_id.clone(),
            reviewed_source_revision: plan.revision,
            decision: ReviewDecision::Accepted,
            reason: Some("Verified the exact delivered artifact".into()),
            verification_refs: vec!["artifact://complete-validation-evidence".into()],
        });
    }
    plan.validate().expect("fixture must be a canonical plan");
    plan
}

async fn commit_plan(fixture: &Fixture, plan: &LocalPlan) -> chat_state::EventSeq {
    fixture
        .actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: None,
                plan: Box::new(plan.clone()),
            },
        ))
        .await
        .expect("the real ChatState owner must commit the planning fact")
        .seq
}

#[test]
fn nested_handler_returns_complete_direct_parent_fact_without_side_effects() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut root = Fixture::new("root-session").await;
        let mut parent = Fixture::new("direct-parent-A").await;
        let mut child = Fixture::new("actual-child-B").await;
        let root_plan = LocalPlan::new("root-session", "root-decoy-scope");
        commit_plan(&root, &root_plan).await;
        let plan = complete_plan("direct-parent-A", "actual-child-B");
        let seq = commit_plan(&parent, &plan).await;
        parent
            .actor
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "fixture".into(),
                    name: "later_non_planning_fact".into(),
                    turn: None,
                    step: None,
                    data: None,
                },
            ))
            .await
            .unwrap();
        let bridge = parent.actor.agent.borrow().tool_bridge().clone();
        bridge
            .update_resource(LocalPlanProjection(Some(root_plan.clone())))
            .await;
        bridge
            .update_resource(State(project_todo_state(&root_plan)))
            .await;
        agent
            .sessions
            .borrow_mut()
            .insert(root.handle.info.id.clone(), root.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let before = (
            root.observables().await,
            parent.observables().await,
            child.observables().await,
        );
        for _ in 0..3 {
            let mut response = start_read(
                &agent,
                "actual-child-B",
                "direct-parent-A",
                "task-16",
                Some("step-16"),
                CancellationToken::new(),
            );
            assert!(
                matches!(
                    response.try_recv(),
                    Err(oneshot::error::TryRecvError::Empty)
                ),
                "synchronous handler only dispatches the observation"
            );
            let snapshot = answer(response).await.unwrap().unwrap();
            assert_eq!(snapshot.timeline_seq, seq.get());
            assert_eq!(snapshot.plan, plan);
            assert_eq!(
                snapshot.delegation,
                Some(PlanDelegation {
                    parent_session_id: "direct-parent-A".into(),
                    parent_scope_id: plan.scope_id.clone(),
                    parent_step_id: "step-16".into(),
                    task_id: "task-16".into(),
                    child_session_id: "actual-child-B".into(),
                })
            );
            assert!(snapshot.plan.steps[16].removed);
            assert!(snapshot.plan.links[16].terminal);
            assert!(!snapshot.plan.reviews.is_empty() && !snapshot.plan.replans.is_empty());
        }
        assert_eq!(
            (
                root.observables().await,
                parent.observables().await,
                child.observables().await
            ),
            before
        );
        root.assert_no_publication();
        parent.assert_no_publication();
        child.assert_no_publication();
    });
}

#[test]
fn unbound_internal_handler_reads_full_parent_plan_without_delegation() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("internal-child").await;
        let plan = complete_plan("parent-session", "internal-child");
        let seq = commit_plan(&parent, &plan).await;
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let before = (parent.observables().await, child.observables().await);
        assert_eq!(
            answer(start_read(
                &agent,
                "internal-child",
                "parent-session",
                "internal-task",
                None,
                CancellationToken::new()
            ))
            .await,
            Ok(Some(PlanningReadSnapshot {
                timeline_seq: seq.get(),
                plan,
                delegation: None
            }))
        );
        assert_eq!(
            (parent.observables().await, child.observables().await),
            before
        );
        parent.assert_no_publication();
        child.assert_no_publication();
    });
}

#[test]
fn no_parent_plan_is_empty_only_for_an_unbound_execution() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("actual-child").await;
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let before = (parent.observables().await, child.observables().await);
        assert_eq!(
            answer(start_read(
                &agent,
                "actual-child",
                "parent-session",
                "task-16",
                None,
                CancellationToken::new()
            ))
            .await,
            Ok(None)
        );
        assert_eq!(
            answer(start_read(
                &agent,
                "actual-child",
                "parent-session",
                "task-16",
                Some("step-16"),
                CancellationToken::new()
            ))
            .await,
            Err(ParentPlanningReadError::InvalidEvidence)
        );
        assert_eq!(
            (parent.observables().await, child.observables().await),
            before
        );
        parent.assert_no_publication();
        child.assert_no_publication();
    });
}

#[test]
fn delegation_requires_exact_task_step_and_child_even_with_a_same_step_retry() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("actual-child").await;
        let mut sibling = Fixture::new("other-same-step").await;
        let plan = complete_plan("parent-session", "actual-child");
        commit_plan(&parent, &plan).await;
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        for fixture in [&child, &sibling] {
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(fixture.handle.info.id.clone(), fixture.handle.clone());
        }
        let before = (
            parent.observables().await,
            child.observables().await,
            sibling.observables().await,
        );
        for (source, task, step) in [
            ("actual-child", "unknown-task", "step-16"),
            ("actual-child", "same-step-retry", "step-16"),
            ("actual-child", "task-16", "step-15"),
            ("actual-child", "task-16", "unknown-step"),
            ("other-same-step", "task-16", "step-16"),
        ] {
            assert_eq!(
                answer(start_read(
                    &agent,
                    source,
                    "parent-session",
                    task,
                    Some(step),
                    CancellationToken::new()
                ))
                .await,
                Err(ParentPlanningReadError::InvalidEvidence),
                "source={source}, task={task}, step={step}"
            );
        }
        assert_eq!(
            (
                parent.observables().await,
                child.observables().await,
                sibling.observables().await
            ),
            before
        );
        parent.assert_no_publication();
        child.assert_no_publication();
        sibling.assert_no_publication();
    });
}

#[test]
fn snapshot_identity_must_match_the_resolved_parent_for_bound_and_unbound_reads() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("actual-child").await;
        let plan = complete_plan("different-owner", "actual-child");
        commit_plan(&parent, &plan).await;
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let before = (parent.observables().await, child.observables().await);
        for step in [None, Some("step-16")] {
            assert_eq!(
                answer(start_read(
                    &agent,
                    "actual-child",
                    "parent-session",
                    "task-16",
                    step,
                    CancellationToken::new()
                ))
                .await,
                Err(ParentPlanningReadError::InvalidEvidence)
            );
        }
        assert_eq!(
            (parent.observables().await, child.observables().await),
            before
        );
        parent.assert_no_publication();
        child.assert_no_publication();
    });
}

#[test]
fn missing_or_closed_handles_are_unavailable_rather_than_no_parent() {
    run_local_for_bridge_test(|| async {
        for unavailable in [
            "source_missing",
            "parent_missing",
            "source_closed",
            "parent_closed",
            "source_chat_closed",
            "parent_chat_closed",
        ] {
            let agent = build_minimal_agent_for_tests();
            let mut parent = Fixture::new("parent-session").await;
            let mut child = Fixture::new("actual-child").await;
            let before = (parent.observables().await, child.observables().await);
            if unavailable == "source_closed" {
                child.commands.close();
            }
            if unavailable == "parent_closed" {
                parent.commands.close();
            }
            if unavailable == "source_chat_closed" {
                child.handle.chat_state_handle = chat_state::ChatStateHandle::noop();
            }
            if unavailable == "parent_chat_closed" {
                parent.handle.chat_state_handle = chat_state::ChatStateHandle::noop();
            }
            if unavailable != "parent_missing" {
                agent
                    .sessions
                    .borrow_mut()
                    .insert(parent.handle.info.id.clone(), parent.handle.clone());
            }
            if unavailable != "source_missing" {
                agent
                    .active_child_sessions
                    .borrow_mut()
                    .insert(child.handle.info.id.clone(), child.handle.clone());
            }
            assert_eq!(
                answer(start_read(
                    &agent,
                    "actual-child",
                    "parent-session",
                    "task-16",
                    Some("step-16"),
                    CancellationToken::new()
                ))
                .await,
                Err(ParentPlanningReadError::Unavailable),
                "{unavailable}"
            );
            assert_eq!(
                (parent.observables().await, child.observables().await),
                before
            );
            assert!(matches!(
                parent.gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                child.gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        }
    });
}

struct HeldWrite {
    persistence: chat_state::MockPersistenceReceiver,
    acknowledgement: oneshot::Sender<std::io::Result<()>>,
    write:
        tokio::task::JoinHandle<Result<chat_state::TimelineEvent, chat_state::TimelineWriteError>>,
    cancellation: CancellationToken,
}

async fn hold_parent_write(parent: &mut Fixture) -> HeldWrite {
    let events = parent
        .actor
        .chat_state_handle
        .timeline_events()
        .await
        .unwrap();
    let (persistence, mut receiver) =
        chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let cancellation = CancellationToken::new();
    let chat = chat_state::ChatStateActor::spawn_from_timeline(
        events,
        serde_json::from_value(serde_json::json!({"base_url":"http://localhost", "model":"test", "context_window":256000})).unwrap(),
        Box::new(persistence),
        event_tx,
        cancellation.clone(),
    ).await.unwrap();
    parent.actor.chat_state_handle = chat.clone();
    parent.handle.chat_state_handle = chat.clone();
    let write = tokio::task::spawn_local(async move {
        chat.record_timeline_event_durably(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: "fixture".into(),
                name: "held_write".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .await
    });
    let acknowledgement = receiver
        .next_timeline_ack()
        .await
        .expect("real parent writer is now blocked before commit");
    HeldWrite {
        persistence: receiver,
        acknowledgement,
        write,
        cancellation,
    }
}

#[test]
fn parent_query_deadline_is_bounded_without_cancelling_the_blocked_owner() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("actual-child").await;
        let plan = complete_plan("parent-session", "actual-child");
        let seq = commit_plan(&parent, &plan).await;
        let before = (parent.observables().await, child.observables().await);
        let mut held = hold_parent_write(&mut parent).await;
        let pending = serde_json::to_value(
            &parent
                .handle
                .chat_state_handle
                .pending_timeline_commit()
                .unwrap()
                .event,
        )
        .unwrap();
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let started = tokio::time::Instant::now();
        assert_eq!(
            answer(start_read(
                &agent,
                "actual-child",
                "parent-session",
                "task-16",
                Some("step-16"),
                CancellationToken::new()
            ))
            .await,
            Err(ParentPlanningReadError::TimedOut)
        );
        assert!(started.elapsed() >= Duration::from_secs(5));
        assert!(!held.cancellation.is_cancelled() && !held.write.is_finished());
        assert_eq!(
            serde_json::to_value(
                &parent
                    .handle
                    .chat_state_handle
                    .pending_timeline_commit()
                    .unwrap()
                    .event
            )
            .unwrap(),
            pending
        );
        held.acknowledgement.send(Ok(())).unwrap();
        let committed = held.write.await.unwrap().unwrap();
        let after = parent.observables().await;
        assert_eq!(after["surface"], before.0["surface"]);
        assert_eq!(after["resources"], before.0["resources"]);
        assert_eq!(
            after["timeline"].as_array().unwrap().len(),
            before.0["timeline"].as_array().unwrap().len() + 1
        );
        assert_eq!(
            after["timeline"].as_array().unwrap().last().unwrap(),
            &serde_json::to_value(committed).unwrap()
        );
        assert_eq!(child.observables().await, before.1);
        let records = held.persistence.drain();
        assert!(
            matches!(records.as_slice(), [chat_state::PersistenceRecord::Timeline(event)] if matches!(&event.kind, chat_state::TimelineEventKind::Observation(observation) if observation.name == "held_write"))
        );
        let snapshot = answer(start_read(
            &agent,
            "actual-child",
            "parent-session",
            "task-16",
            Some("step-16"),
            CancellationToken::new(),
        ))
        .await
        .unwrap()
        .unwrap();
        assert_eq!(snapshot.timeline_seq, seq.get());
        assert_eq!(snapshot.plan, plan);
        parent.assert_no_publication();
        child.assert_no_publication();
    });
}

#[test]
fn cancelling_an_observation_preserves_parent_commit_child_state_and_execution() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("actual-child").await;
        let plan = complete_plan("parent-session", "actual-child");
        commit_plan(&parent, &plan).await;
        let child_before = child.observables().await;
        let parent_resources = parent.resources().await;
        let mut held = hold_parent_write(&mut parent).await;
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let cancellation = CancellationToken::new();
        let response = start_read(
            &agent,
            "actual-child",
            "parent-session",
            "task-16",
            Some("step-16"),
            cancellation.clone(),
        );
        tokio::task::yield_now().await;
        cancellation.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), answer(response))
                .await
                .unwrap(),
            Err(ParentPlanningReadError::Unavailable)
        );
        assert!(!held.cancellation.is_cancelled() && !held.write.is_finished());
        assert_eq!(child.observables().await, child_before);
        assert_eq!(parent.resources().await, parent_resources);
        parent.assert_no_publication();
        child.assert_no_publication();
        held.acknowledgement.send(Ok(())).unwrap();
        held.write.await.unwrap().unwrap();
        assert_eq!(
            held.persistence.drain().len(),
            1,
            "cancelled read appended no fact"
        );
        let already_cancelled = CancellationToken::new();
        already_cancelled.cancel();
        let before = (parent.observables().await, child.observables().await);
        assert_eq!(
            answer(start_read(
                &agent,
                "actual-child",
                "parent-session",
                "task-16",
                Some("step-16"),
                already_cancelled
            ))
            .await,
            Err(ParentPlanningReadError::Unavailable)
        );
        assert_eq!(
            (parent.observables().await, child.observables().await),
            before
        );
        assert!(
            answer(start_read(
                &agent,
                "actual-child",
                "parent-session",
                "task-16",
                Some("step-16"),
                CancellationToken::new()
            ))
            .await
            .is_ok()
        );
    });
}

#[test]
fn losing_the_parent_chat_state_during_a_query_returns_unavailable() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut parent = Fixture::new("parent-session").await;
        let mut child = Fixture::new("actual-child").await;
        let mut held = hold_parent_write(&mut parent).await;
        agent
            .sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let child_before = child.observables().await;
        let response = start_read(
            &agent,
            "actual-child",
            "parent-session",
            "internal-task",
            None,
            CancellationToken::new(),
        );
        tokio::task::yield_now().await;
        held.cancellation.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), answer(response))
                .await
                .unwrap(),
            Err(ParentPlanningReadError::Unavailable)
        );
        assert!(held.write.await.unwrap().is_err());
        assert_eq!(held.persistence.drain().len(), 1);
        assert_eq!(child.observables().await, child_before);
        assert!(!parent.handle.cmd_tx.is_closed());
        parent.assert_no_publication();
        child.assert_no_publication();
    });
}
