//! Consumer failure and cancellation tests through the real session-bound backend.
use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::{build_actor, test_agent_with_tools};
use tools::implementations::grow_build::task::{
    backend::{ChannelBackend, SubagentBackendResource},
    parent_planning::{ParentPlanningReadError, ParentPlanningRequest},
    types::SubagentEvent,
};

async fn fixture() -> (
    Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    mpsc::UnboundedReceiver<SubagentEvent>,
) {
    let (mut actor, gateway) = build_actor().await;
    let child = Arc::get_mut(&mut actor).expect("the fixture has one actor owner");
    child.startup_hints.parent_session_id = Some("lifecycle-root-decoy".into());
    child.startup_hints.delegated_parent_plan = Some(ParentPlanBinding {
        parent_session_id: "direct-parent-A".into(),
        task_id: "current-task".into(),
        parent_step_id: "step-16".into(),
    });
    let (tx, events) = mpsc::unbounded_channel();
    actor
        .agent
        .borrow()
        .tool_bridge()
        .clone()
        .update_resource(ChannelBackend::for_session(tx, actor.session_id_string()).into_resource())
        .await;
    (actor, gateway, events)
}

async fn next_read(events: &mut mpsc::UnboundedReceiver<SubagentEvent>) -> ParentPlanningRequest {
    match tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap()
    {
        SubagentEvent::ReadParentPlanning(request) => request,
        _ => panic!("planning context must issue only a readonly parent observation"),
    }
}

fn json_context(text: &str) -> serde_json::Value {
    assert!(text.len() <= 8192);
    serde_json::from_str(text.split_once('\n').unwrap().1).unwrap()
}

async fn observable(actor: &SessionActor) -> serde_json::Value {
    let bridge = actor.agent.borrow().tool_bridge().clone();
    let resources = bridge.shared_resources().await;
    let resources = resources.lock().await;
    let projections = json!({
        "persisted": resources.serialize(),
        "plan": resources.get::<LocalPlanProjection>().map(|projection| &projection.0),
        "todo": resources.get::<tools::types::resources::State<tools::implementations::grow_build::todo::TodoState>>(),
    });
    drop(resources);
    json!({
        "timeline": actor.chat_state_handle.timeline_events().await.unwrap(),
        "surface": actor.chat_state_handle.get_conversation_with_revision().await.unwrap(),
        "projections": projections,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn parent_failure_keeps_binding_without_plan_gate_resource_lock_or_publication() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway, mut events) = fixture().await;
            let before = observable(&actor).await;
            let gate = actor.step_control_gate.lock().await;
            let read = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { actor.local_planning_context(&mut None).await }
            });
            let request = next_read(&mut events).await;
            assert_eq!(request.source_session_id, actor.session_id_string());
            let bridge = actor.agent.borrow().tool_bridge().clone();
            let resources = bridge.shared_resources().await;
            drop(
                resources
                    .try_lock()
                    .expect("parent query must release Resources before awaiting"),
            );
            request
                .respond_to
                .send(Err(ParentPlanningReadError::Unavailable))
                .unwrap();
            let text = tokio::time::timeout(Duration::from_secs(1), read)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .unwrap();
            drop(gate);
            let value = json_context(&text);
            assert_eq!(
                value["parent"]["binding"]["parent_session_id"],
                "direct-parent-A"
            );
            assert_eq!(value["parent"]["binding"]["task_id"], "current-task");
            assert_eq!(value["parent"]["binding"]["parent_step_id"], "step-16");
            assert_eq!(value["parent"]["error_code"], "unavailable");
            assert_eq!(value["read_entry"]["availability"], "unavailable");
            assert!(!text.contains("lifecycle-root-decoy"));
            assert_eq!(observable(&actor).await, before);
            assert!(matches!(
                gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn parent_observation_uses_the_backend_five_second_deadline() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway, mut events) = fixture().await;
            let start = tokio::time::Instant::now();
            let read = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { actor.local_planning_context(&mut None).await }
            });
            let request = next_read(&mut events).await;
            let text = tokio::time::timeout(Duration::from_secs(6), read)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(start.elapsed() >= Duration::from_secs(5));
            assert_eq!(json_context(&text)["parent"]["error_code"], "timed_out");
            assert!(request.cancellation.is_cancelled());
            assert!(request.respond_to.is_closed());
            assert!(!actor.background_service_shutdown.is_cancelled());
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .is_none()
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropped_context_cancels_only_its_read_and_the_next_request_reads_again() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway, mut events) = fixture().await;
            let read = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { actor.local_planning_context(&mut None).await }
            });
            let first = next_read(&mut events).await;
            read.abort();
            assert!(read.await.unwrap_err().is_cancelled());
            assert!(first.cancellation.is_cancelled());
            assert!(first.respond_to.is_closed());
            assert!(!actor.background_service_shutdown.is_cancelled());
            let read = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { actor.local_planning_context(&mut None).await }
            });
            let second = next_read(&mut events).await;
            assert!(!second.cancellation.is_cancelled());
            second
                .respond_to
                .send(Err(ParentPlanningReadError::NoParent))
                .unwrap();
            let text = read.await.unwrap().unwrap().unwrap();
            assert_eq!(json_context(&text)["parent"]["error_code"], "no_parent");
            assert_eq!(
                json_context(&text)["parent"]["binding"]["task_id"],
                "current-task"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn self_owner_error_propagates_before_any_parent_observation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut actor, _gateway, mut events) = fixture().await;
            Arc::get_mut(&mut actor).unwrap().chat_state_handle =
                chat_state::ChatStateHandle::noop();
            let expected = actor.chat_state_handle.planning_state().await.unwrap_err();
            assert_eq!(actor.local_planning_context(&mut None).await, Err(expected));
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn no_birth_binding_and_no_local_plan_does_not_read_the_lifecycle_root() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut actor, _gateway, mut events) = fixture().await;
            Arc::get_mut(&mut actor)
                .unwrap()
                .startup_hints
                .delegated_parent_plan = None;
            assert_eq!(actor.local_planning_context(&mut None).await.unwrap(), None);
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[test]
fn wire_startup_hints_cannot_supply_a_delegated_plan_route() {
    let hints: crate::session::StartupHints = serde_json::from_value(json!({
        "isSubagent": true,
        "parentSessionId": "lifecycle-root",
        "delegatedParentPlan": {
            "parent_session_id": "forged-parent", "task_id": "forged-task",
            "parent_step_id": "forged-step",
        },
    }))
    .unwrap();
    assert_eq!(hints.parent_session_id.as_deref(), Some("lifecycle-root"));
    assert_eq!(hints.delegated_parent_plan, None);
}

#[tokio::test(flavor = "current_thread")]
async fn held_resources_times_out_before_dispatch_and_keeps_the_parent_binding() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway, mut events) = fixture().await;
            let before = observable(&actor).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            let resources = bridge.shared_resources().await;
            let locked = resources.lock().await;
            let start = tokio::time::Instant::now();
            let text = tokio::time::timeout(
                Duration::from_secs(6),
                actor.local_planning_context(&mut None),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert!(start.elapsed() >= Duration::from_secs(5));
            let value = json_context(&text);
            assert_eq!(value["parent"]["error_code"], "timed_out");
            assert_eq!(
                value["parent"]["binding"]["parent_session_id"],
                "direct-parent-A"
            );
            assert_eq!(value["parent"]["binding"]["task_id"], "current-task");
            assert_eq!(value["parent"]["binding"]["parent_step_id"], "step-16");
            assert_eq!(value["read_entry"]["availability"], "unavailable");
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(!actor.background_service_shutdown.is_cancelled());
            drop(locked);
            assert_eq!(observable(&actor).await, before);
            assert!(matches!(
                gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn held_resources_also_bounds_a_self_only_context_without_dispatching_parent_reads() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut actor, mut gateway, mut events) = fixture().await;
            Arc::get_mut(&mut actor)
                .unwrap()
                .startup_hints
                .delegated_parent_plan = None;
            let mut plan = LocalPlan::new(actor.session_id_string(), "scope:self-only");
            plan.steps.push(PlanStep {
                id: "self-obligation".into(),
                content: "Preserve self responsibilities".into(),
                priority: PlanPriority::High,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            });
            let seq = actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(plan),
                    },
                ))
                .await
                .unwrap()
                .seq;
            let before = observable(&actor).await;
            let bridge = actor.agent.borrow().tool_bridge().clone();
            let resources = bridge.shared_resources().await;
            let locked = resources.lock().await;
            let start = tokio::time::Instant::now();
            let text = tokio::time::timeout(
                Duration::from_secs(6),
                actor.local_planning_context(&mut None),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert!(start.elapsed() >= Duration::from_secs(5));
            let value = json_context(&text);
            assert!(value["parent"].is_null());
            assert_eq!(value["self"]["source"]["timeline_seq"], seq.get());
            assert_eq!(value["self"]["steps_summary"][0]["id"], "self-obligation");
            assert_eq!(value["read_entry"]["availability"], "unavailable");
            assert!(matches!(
                events.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(!actor.background_service_shutdown.is_cancelled());
            drop(locked);
            assert_eq!(observable(&actor).await, before);
            assert!(matches!(
                gateway.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        })
        .await;
}

async fn assert_timeout_keeps_resolved_read_alias(reply: Option<ParentPlanningReadError>) {
    let (actor, mut gateway, mut events) = fixture().await;
    let bridge = actor.agent.borrow().tool_bridge().clone();
    let backend = bridge
        .read_resource::<SubagentBackendResource>()
        .await
        .unwrap();
    let mut tool = tools::registry::types::ToolConfig::for_tool::<
        tools::implementations::grow_build::todo::TodoReadTool,
    >();
    let alias = "resolved_alias_2_4a_read_plan";
    tool.name_override = Some(alias.into());
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![tool]).await;
    let bridge = actor.agent.borrow().tool_bridge().clone();
    bridge.update_resource(backend).await;
    assert_eq!(
        bridge
            .tool_for_kind(tools::types::tool::ToolKind::PlanRead)
            .await
            .as_deref(),
        Some(alias)
    );
    let before = observable(&actor).await;
    let started = tokio::time::Instant::now();
    let read = tokio::task::spawn_local({
        let actor = actor.clone();
        async move { actor.local_planning_context(&mut None).await }
    });
    let ParentPlanningRequest {
        source_session_id,
        cancellation,
        respond_to,
    } = next_read(&mut events).await;
    assert_eq!(source_session_id, actor.session_id_string());
    assert!(!cancellation.is_cancelled());
    let held_response = match reply {
        Some(error) => {
            respond_to.send(Err(error)).unwrap();
            None
        }
        None => Some(respond_to),
    };
    let text = tokio::time::timeout(Duration::from_secs(6), read)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    if let Some(response) = held_response {
        assert!(started.elapsed() >= Duration::from_secs(5));
        assert!(response.is_closed());
        assert!(
            response
                .send(Err(ParentPlanningReadError::TimedOut))
                .is_err()
        );
    }
    let value = json_context(&text);
    assert_eq!(value["parent"]["error_code"], "timed_out");
    assert_eq!(
        value["parent"]["binding"],
        json!({
            "parent_session_id": "direct-parent-A",
            "task_id": "current-task", "parent_step_id": "step-16",
        })
    );
    assert_eq!(value["read_entry"]["availability"], "available");
    assert_eq!(value["read_entry"]["tool"], alias);
    assert!(cancellation.is_cancelled());
    assert!(!actor.background_service_shutdown.is_cancelled());
    assert_eq!(observable(&actor).await, before);
    assert!(matches!(
        events.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        gateway.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn resolved_read_alias_survives_explicit_parent_timeout() {
    tokio::task::LocalSet::new()
        .run_until(async {
            assert_timeout_keeps_resolved_read_alias(Some(ParentPlanningReadError::TimedOut)).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn resolved_read_alias_survives_the_shared_observation_deadline() {
    tokio::task::LocalSet::new()
        .run_until(async {
            assert_timeout_keeps_resolved_read_alias(None).await;
        })
        .await;
}
