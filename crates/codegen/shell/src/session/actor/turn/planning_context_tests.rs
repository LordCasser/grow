use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::build_actor;
use tool_types::planning::{LocalPlan, PlanPriority, PlanStep, PlanStepStatus};
use tools::implementations::grow_build::task::backend::ChannelBackend;

#[tokio::test(flavor = "current_thread")]
async fn delegated_requests_never_read_or_project_plans() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut child, _gateway) = build_actor().await;
            let hints = &mut Arc::get_mut(&mut child).unwrap().startup_hints;
            hints.is_subagent = true;
            hints.delegated_parent_plan = Some(ParentPlanBinding {
                parent_session_id: "parent".into(),
                task_id: "task".into(),
                parent_step_id: "step".into(),
            });
            let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            child
                .agent
                .borrow()
                .tool_bridge()
                .clone()
                .update_resource(
                    ChannelBackend::for_session(events, child.session_id_string()).into_resource(),
                )
                .await;
            let mut plan = LocalPlan::new(child.session_id_string(), "scope:child");
            plan.steps.push(PlanStep {
                id: "local".into(),
                content: "child local plan marker".into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            });
            child
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(plan),
                    },
                ))
                .await
                .unwrap();

            for _ in 0..2 {
                let request = child.build_turn_request(vec![], None, None).await.unwrap();
                assert!(request.source_projection.unwrap()["runtime_context"].is_null());
                assert!(request.items.iter().all(|item| {
                    let text = item.text_content();
                    !text.contains("Current planning context")
                        && !text.contains("child local plan marker")
                }));
                assert!(receiver.try_recv().is_err());
            }
        })
        .await;
}
