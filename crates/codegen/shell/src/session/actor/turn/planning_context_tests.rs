//! Provider-request projection through build_turn_request and real ChatState owners.
//! Parent route authorization is exercised by the coordinator consumer tests.
use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::{build_actor, test_agent_with_tools};
use tool_types::planning::{
    LocalPlan, PlanDelegation, PlanLink, PlanPriority, PlanStep, PlanStepStatus, PlanUpdateMode,
    PlanningReadSnapshot,
};
use tools::implementations::grow_build::task::{
    backend::ChannelBackend, parent_planning::ParentPlanningReadError, types::SubagentEvent,
};
use tools::implementations::grow_build::todo::TodoReadTool;

const PARENT: &str = "turn-parent-A";
const CHILD: &str = "turn-child-B";

fn plan() -> LocalPlan {
    let mut plan = LocalPlan::new(PARENT, "scope:turn-parent");
    plan.revision = 1;
    for index in 0..17 {
        plan.steps.push(PlanStep {
            id: format!("step-{index}"),
            content: if index == 16 {
                "initial delegated objective".into()
            } else {
                "sibling body is private".into()
            },
            priority: PlanPriority::High,
            status: PlanStepStatus::InProgress,
            requirements: Some(vec![
                "Keep the current owner's acceptance requirements".into(),
            ]),
            removed: false,
        });
    }
    plan.links.push(PlanLink {
        id: "turn-task".into(),
        parent_step_id: "step-16".into(),
        child_session_id: CHILD.into(),
        terminal: false,
        required: true,
        pending_source_revision: None,
        review_required_revision: 1,
        result_id: None,
        dispatch_failure: None,
    });
    plan
}

fn context_item(
    request: &sampling_types::ConversationRequest,
) -> &sampling_types::ConversationItem {
    let item = request.items.last().unwrap();
    assert!(matches!(item, sampling_types::ConversationItem::User(user)
        if user.synthetic_reason == Some(sampling_types::SyntheticReason::SystemReminder)));
    item
}

#[tokio::test(flavor = "current_thread")]
async fn first_and_later_turn_requests_refresh_parent_context_without_storing_it() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut child, mut child_gateway) = build_actor().await;
            let (mut parent, mut parent_gateway) = build_actor().await;
            Arc::get_mut(&mut child).unwrap().session_info.id = acp::SessionId::new(CHILD);
            Arc::get_mut(&mut parent).unwrap().session_info.id = acp::SessionId::new(PARENT);
            let initial = child.build_turn_request(vec![], None, None).await.unwrap();
            child.chat_state_handle.record_provider_context_anchor(
                chat_state::estimate_request_input_tokens(&initial),
            );
            let hints = &mut Arc::get_mut(&mut child).unwrap().startup_hints;
            hints.parent_session_id = Some("lifecycle-ROOT-decoy".into());
            hints.delegated_parent_plan = Some(ParentPlanBinding {
                parent_session_id: PARENT.into(),
                task_id: "turn-task".into(),
                parent_step_id: "step-16".into(),
            });
            let mut config = tools::registry::types::ToolConfig::for_tool::<TodoReadTool>();
            config.name_override = Some("read_alias_for_turn".into());
            *child.agent.borrow_mut() = test_agent_with_tools(vec![config]).await;
            let bridge = child.agent.borrow().tool_bridge().clone();
            let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            bridge
                .update_resource(ChannelBackend::for_session(events, CHILD).into_resource())
                .await;
            let query_parent = parent.clone();
            let reader = tokio::task::spawn_local(async move {
                while let Some(event) = receiver.recv().await {
                    let SubagentEvent::ReadParentPlanning(query) = event else {
                        panic!("unexpected write/event");
                    };
                    assert_eq!(query.source_session_id, CHILD);
                    let snapshot = query_parent
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap();
                    let delegation = PlanDelegation {
                        parent_session_id: PARENT.into(),
                        parent_scope_id: snapshot.plan.scope_id.clone(),
                        parent_step_id: "step-16".into(),
                        task_id: "turn-task".into(),
                        child_session_id: CHILD.into(),
                    };
                    let _ = query.respond_to.send(Ok(Some(PlanningReadSnapshot {
                        timeline_seq: snapshot.seq.get(),
                        plan: snapshot.plan,
                        delegation: Some(delegation),
                    })));
                }
            });
            child
                .chat_state_handle
                .push_user_message_durably(sampling_types::ConversationItem::user(
                    "Authored child objective",
                ))
                .await
                .unwrap();
            let mut plan = plan();
            let mut seq = parent
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(plan.clone()),
                    },
                ))
                .await
                .unwrap()
                .seq;
            let child_before = child.chat_state_handle.timeline_events().await.unwrap();
            let surface_before = child
                .chat_state_handle
                .materialize_timeline(CHILD.into())
                .await
                .unwrap();
            for pass in 0..2 {
                let parent_before = parent.chat_state_handle.timeline_events().await.unwrap();
                let parent_gate = parent.step_control_gate.lock().await;
                let request = tokio::time::timeout(
                    Duration::from_secs(2),
                    child.build_turn_request(vec![], None, None),
                )
                .await
                .unwrap()
                .unwrap();
                drop(parent_gate);
                assert_eq!(request.items.len(), surface_before.surface.len() + 1);
                let text = context_item(&request).text_content();
                assert!(text.contains(if pass == 0 {
                    "initial delegated objective"
                } else {
                    "latest delegated objective"
                }));
                assert!(!text.contains("lifecycle-ROOT-decoy"));
                assert!(!text.contains("sibling body is private"));
                if pass == 1 {
                    assert!(!text.contains("initial delegated objective"));
                }
                let json_start = text.find("Current planning context").unwrap();
                let payload = text[json_start..].split_once('\n').unwrap().1;
                // ChatState wraps request-only context in its synthetic reminder.
                let end = payload.rfind('}').unwrap() + 1;
                let context: serde_json::Value = serde_json::from_str(&payload[..end]).unwrap();
                assert_eq!(context["parent"]["source"]["timeline_seq"], seq.get());
                assert_eq!(context["parent"]["source"]["revision"], plan.revision);
                assert_eq!(context["parent"]["associated_step"]["position"], 17);
                assert_eq!(context["read_entry"]["tool"], "read_alias_for_turn");
                assert!(context["self"].is_null());
                assert_eq!(
                    child.chat_state_handle.get_projected_tokens().await,
                    chat_state::estimate_request_input_tokens(&request)
                );
                assert_eq!(
                    serde_json::to_value(child.chat_state_handle.timeline_events().await.unwrap())
                        .unwrap(),
                    serde_json::to_value(&child_before).unwrap()
                );
                assert_eq!(
                    serde_json::to_value(parent.chat_state_handle.timeline_events().await.unwrap())
                        .unwrap(),
                    serde_json::to_value(parent_before).unwrap()
                );
                let surface_after = child
                    .chat_state_handle
                    .materialize_timeline(CHILD.into())
                    .await
                    .unwrap();
                assert_eq!(surface_after.surface_ids, surface_before.surface_ids);
                assert_eq!(
                    serde_json::to_value(surface_after.surface).unwrap(),
                    serde_json::to_value(&surface_before.surface).unwrap()
                );
                assert_eq!(
                    request
                        .items
                        .iter()
                        .filter(|item| item.text_content() == "Authored child objective")
                        .count(),
                    1
                );
                if pass == 0 {
                    let mut updated = plan.steps[16].clone();
                    updated.content = "latest delegated objective".into();
                    plan.update_steps(PlanUpdateMode::Merge, vec![updated])
                        .unwrap();
                    seq = parent
                        .chat_state_handle
                        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                            chat_state::timeline::PlanningEvent {
                                previous: Some(seq),
                                plan: Box::new(plan.clone()),
                            },
                        ))
                        .await
                        .unwrap()
                        .seq;
                }
            }
            assert!(child_gateway.try_recv().is_err());
            assert!(parent_gateway.try_recv().is_err());
            reader.abort();
            assert!(reader.await.unwrap_err().is_cancelled());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn turn_parent_observation_happens_before_the_child_step_gate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut actor, _gateway) = build_actor().await;
            Arc::get_mut(&mut actor)
                .unwrap()
                .startup_hints
                .delegated_parent_plan = Some(ParentPlanBinding {
                parent_session_id: PARENT.into(),
                task_id: "turn-task".into(),
                parent_step_id: "step-16".into(),
            });
            let bridge = actor.agent.borrow().tool_bridge().clone();
            let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            bridge
                .update_resource(
                    ChannelBackend::for_session(events, actor.session_id_string()).into_resource(),
                )
                .await;
            let gate = actor.step_control_gate.lock().await;
            let request = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { actor.build_turn_request(vec![], None, None).await }
            });
            let event = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .unwrap();
            let SubagentEvent::ReadParentPlanning(query) = event else {
                panic!("expected readonly query");
            };
            query
                .respond_to
                .send(Err(ParentPlanningReadError::Unavailable))
                .unwrap();
            assert!(!request.is_finished());
            drop(gate);
            let request = tokio::time::timeout(Duration::from_secs(1), request)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let text = context_item(&request).text_content();
            assert!(text.contains("unavailable"));
            assert!(text.contains("turn-task"));
        })
        .await;
}
