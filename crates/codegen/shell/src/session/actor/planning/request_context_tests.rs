//! Local plan projection and delegation authority boundaries.
use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::build_actor;
use tools::implementations::grow_build::task::{backend::ChannelBackend, types::SubagentEvent};

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
async fn primary_plan_owner_error_propagates() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut actor, _gateway, mut events) = fixture().await;
            Arc::get_mut(&mut actor).unwrap().chat_state_handle =
                chat_state::ChatStateHandle::noop();
            let expected = actor.chat_state_handle.planning_state().await.unwrap_err();
            assert_eq!(actor.local_planning_context().await, Err(expected));
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
            assert_eq!(actor.local_planning_context().await.unwrap(), None);
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
async fn held_resources_bounds_a_self_only_context_without_dispatching_parent_reads() {
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
            let text = tokio::time::timeout(Duration::from_secs(6), actor.local_planning_context())
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
