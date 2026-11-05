use super::super::SessionActor;
use super::support::{
    begin_test_causal_turn, build_actor_with_planning_driver,
    create_test_actor_with_planning_receiver, start_test_planning_driver,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tool_types::planning::{
    DeliveryReview, DependencyKind, LocalPlan, PlanLink, PlanPriority, PlanStep, PlanStepStatus,
    PlanUpdateMode, PlanWait, PlanningReadSnapshot, ReviewDecision, WaitCondition, WaitDependency,
};
use tools::implementations::grow_build::task::types::CurrentPromptIdResource;
use tools::implementations::grow_build::todo::{
    TodoState, project_todo_state,
    runtime::{LocalPlanProjection, PlanningCommand, PlanningRuntimeHandle},
};
use tools::types::resources::State;

async fn read_plan(actor: &Arc<SessionActor>) -> Result<Option<PlanningReadSnapshot>, String> {
    let bridge = actor.agent.borrow().tool_bridge().clone();
    let runtime = bridge
        .read_resource::<PlanningRuntimeHandle>()
        .await
        .expect("fixture must expose the real session planning channel");
    let (respond_to, response) = oneshot::channel();
    runtime
        .0
        .send(PlanningCommand::Read { respond_to })
        .expect("planning driver must be alive");
    tokio::time::timeout(Duration::from_millis(500), response)
        .await
        .expect("read must return without waiting for the Step mutation gate")
        .expect("planning owner must answer")
}

async fn observables(actor: &SessionActor) -> serde_json::Value {
    let bridge = actor.agent.borrow().tool_bridge().clone();
    let resources = bridge.shared_resources().await;
    let resource_state = {
        let resources = resources.lock().await;
        serde_json::json!({
            "persisted": resources.serialize(),
            "plan_projection": resources.get::<LocalPlanProjection>().map(|plan| &plan.0),
            "todo_state": resources.get::<State<TodoState>>(),
            "prompt": resources.get::<CurrentPromptIdResource>().map(|prompt| &prompt.0),
        })
    };
    serde_json::json!({
        "timeline": actor.chat_state_handle.timeline_events().await.expect("live Timeline"),
        "surface": actor.chat_state_handle.get_conversation_with_revision().await.expect("live Surface"),
        "resources": resource_state,
    })
}

fn assert_no_planning_delivery(actor: &SessionActor) {
    assert!(!actor.planning_delivery_dirty.get());
    assert!(
        actor.planning_delivery.take().is_none(),
        "read must never schedule a planning delivery, even one that already finished"
    );
}

fn complete_plan(actor: &SessionActor) -> LocalPlan {
    let mut plan = LocalPlan::new(actor.session_id_string(), "scope:read");
    plan.revision = 7;
    for index in 0..17 {
        let step_id = format!("step-{index}");
        let child_id = format!("child-{index}");
        let result_id = (index != 0).then(|| {
            serde_json::to_string(&chat_state::TimelineRangeRef {
                timeline_id: child_id.clone(),
                first_seq: 101,
                last_seq: 101,
            })
            .unwrap()
        });
        plan.steps.push(PlanStep {
            id: step_id.clone(),
            content: format!(
                "Complete step {index}: {}",
                "full decision facts ".repeat(32)
            ),
            priority: PlanPriority::High,
            status: if index == 16 {
                PlanStepStatus::Cancelled
            } else {
                PlanStepStatus::InProgress
            },
            requirements: Some(vec!["retain the full acceptance requirements ".repeat(16)]),
            removed: index == 16,
        });
        plan.links.push(PlanLink {
            id: format!("task-{index}"),
            parent_step_id: step_id.clone(),
            child_session_id: child_id,
            terminal: index != 0,
            required: true,
            pending_source_revision: None,
            review_required_revision: plan.revision,
            result_id,
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
    let mut updated_step = plan.steps[0].clone();
    updated_step.content.push_str(" Latest owner requirement.");
    plan.update_steps(PlanUpdateMode::Merge, vec![updated_step])
        .expect("a real source edit creates a pending replan");
    for link in plan.links.iter().filter(|link| link.terminal) {
        plan.reviews.push(DeliveryReview {
            link_id: link.id.clone(),
            result_id: link.result_id.clone(),
            reviewed_source_revision: plan.revision,
            decision: ReviewDecision::Accepted,
            reason: Some("verified the complete delivered artifact".into()),
            verification_refs: vec!["artifact://full-validation-evidence".into()],
        });
    }
    plan.validate().expect("fixture is a canonical valid plan");
    plan
}

async fn commit_plan(actor: &SessionActor, plan: &LocalPlan) -> chat_state::EventSeq {
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: None,
                plan: Box::new(plan.clone()),
            },
        ))
        .await
        .expect("the session owner must commit the fixture plan")
        .seq
}

#[tokio::test(flavor = "current_thread")]
async fn read_without_plan_is_empty_without_creating_or_publishing_state() {
    tokio::task::LocalSet::new()
        .run_until(async {
            // This fixture is idle and does not run production startup hydration.
            let (actor, mut gateway) = build_actor_with_planning_driver().await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            actor.bind_local_planning_runtime(&bridge).await.unwrap();
            assert!(actor.state.lock().await.running_prompt_id().is_none());
            assert!(
                bridge
                    .read_resource::<CurrentPromptIdResource>()
                    .await
                    .is_none()
            );
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .is_none()
            );
            let before = observables(&actor).await;

            for _ in 0..3 {
                assert_eq!(read_plan(&actor).await, Ok(None));
                assert_no_planning_delivery(&actor);
            }

            assert_eq!(observables(&actor).await, before);
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(matches!(
                gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn read_returns_complete_owner_snapshot_without_projection_or_delivery_effects() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway) = build_actor_with_planning_driver().await;
            let plan = complete_plan(&actor);
            let seq = commit_plan(&actor, &plan).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            actor.bind_local_planning_runtime(&bridge).await.unwrap();
            let stale = LocalPlan::new(actor.session_id_string(), "stale-resource-scope");
            bridge
                .update_resource(LocalPlanProjection(Some(stale.clone())))
                .await;
            bridge
                .update_resource(State(project_todo_state(&stale)))
                .await;
            let before = observables(&actor).await;

            for _ in 0..3 {
                let snapshot = read_plan(&actor).await.unwrap().expect("committed plan");
                assert_eq!(snapshot.timeline_seq, seq.get());
                assert_eq!(snapshot.plan, plan);
                assert_eq!(snapshot.plan.session_id, actor.session_id_string());
                assert_eq!(snapshot.delegation, None);
                assert_no_planning_delivery(&actor);
            }

            assert_eq!(observables(&actor).await, before);
            assert!(matches!(
                gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn read_returns_while_step_control_gate_is_held() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway) = build_actor_with_planning_driver().await;
            let plan = complete_plan(&actor);
            let seq = commit_plan(&actor, &plan).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            actor.bind_local_planning_runtime(&bridge).await.unwrap();
            let gate = actor.step_control_gate.lock().await;

            let snapshot = read_plan(&actor).await.unwrap().expect("committed plan");
            assert_eq!(snapshot.timeline_seq, seq.get());
            assert_eq!(snapshot.plan, plan);
            assert_no_planning_delivery(&actor);
            drop(gate);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn read_keeps_planning_fact_seq_when_tool_and_step_events_advance_timeline() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway) = build_actor_with_planning_driver().await;
            let plan = complete_plan(&actor);
            let seq = commit_plan(&actor, &plan).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            actor.bind_local_planning_runtime(&bridge).await.unwrap();
            let before = read_plan(&actor).await.unwrap().expect("committed plan");

            begin_test_causal_turn(&actor).await;
            actor
                .events
                .tool_started("todo_read".into(), "read-call".into(), None)
                .await
                .expect("real tool start must commit");
            assert_eq!(read_plan(&actor).await, Ok(Some(before.clone())));
            actor
                .events
                .tool_completed_durably("read-call", "success".into(), None)
                .await
                .expect("real tool completion must commit");
            assert!(actor.events.end_step("complete"));
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.last().unwrap().seq > seq);
            assert!(matches!(
                events.last().unwrap().kind,
                chat_state::TimelineEventKind::Step(chat_state::StepEvent::Ended { .. })
            ));

            assert_eq!(read_plan(&actor).await, Ok(Some(before)));
            assert_no_planning_delivery(&actor);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn read_preserves_unavailable_chat_state_error_instead_of_empty_plan() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, _persistence_rx) = mpsc::unbounded_channel();
            let (mut actor, planning_rx) = create_test_actor_with_planning_receiver(
                0,
                256_000,
                85,
                gateway_tx,
                persistence_tx,
            )
            .await;
            // A real ChatStateHandle with a closed inbox has no available owner.
            actor.chat_state_handle = chat_state::ChatStateHandle::noop();
            let expected = actor.chat_state_handle.planning_state().await.unwrap_err();
            let bridge = actor.agent.borrow().tool_bridge().clone();
            bridge
                .update_resource(PlanningRuntimeHandle(actor.planning_command_tx.clone()))
                .await;
            let actor = Arc::new(actor);
            start_test_planning_driver(&actor, planning_rx);

            assert_eq!(read_plan(&actor).await, Err(expected));
            assert_no_planning_delivery(&actor);
        })
        .await;
}
