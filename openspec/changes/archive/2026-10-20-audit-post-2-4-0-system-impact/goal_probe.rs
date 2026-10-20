// Audit probe: PASS means the documented defect was reproduced.
use super::*;
#[tokio::test(flavor = "current_thread")]
async fn audit_post_240_goal_activation_loses_deferred_plan_in_control() {
    tokio::task::LocalSet::new().run_until(async {
        use crate::session::behavior::{RetainedPlanAction, RetainedPlanDecisionOutcome, RetainedPlanStatus};
        use tools::implementations::grow_build::{CreateGoalTool, GetGoalTool, UpdateGoalTool, todo::TodoWriteTool};
        use tools::registry::types::ToolConfig;
        let (mut actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
        std::sync::Arc::get_mut(&mut actor).unwrap().goal_enabled = true;
        *actor.agent.borrow_mut() = crate::session::actor::tests::support::test_agent_with_tools(vec![
            ToolConfig::for_tool::<CreateGoalTool>(), ToolConfig::for_tool::<GetGoalTool>(),
            ToolConfig::for_tool::<UpdateGoalTool>(), ToolConfig::for_tool::<TodoWriteTool>(),
        ]).await;
        let before = {
            let mut behavior = actor.behavior.lock();
            behavior.select_behavior(tool_types::BehaviorId::Plan);
            behavior.record_plan_artifact("# Deferred audit plan");
            assert!(behavior.submit_initial_plan());
            let snapshot = behavior.snapshot();
            assert_eq!(behavior.decide_submitted_plan_if(&snapshot, snapshot.plan_submission_id.unwrap(), "audit-defer", RetainedPlanAction::Defer, None), RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred));
            behavior.select_behavior(tool_types::BehaviorId::Normal);
            behavior.snapshot()
        };
        assert!(before.retained_plan.is_some());
        actor.persist_control_snapshot_durably(before.clone(), None).await.unwrap();
        actor.initialize_goal_runtime("audit goal", None).await.unwrap();
        let live = actor.behavior.lock().snapshot();
        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        let persisted = crate::session::control::SessionControlSnapshot::latest_from_timeline(&events).unwrap().unwrap();
        assert_eq!(live.retained_plan, before.retained_plan);
        assert_eq!(live.last_plan_submission_id, before.last_plan_submission_id);
        assert!(persisted.behavior.retained_plan.is_none(), "probe expects the current defect");
        assert_eq!(persisted.behavior.last_plan_submission_id, 0);
        eprintln!("AUDIT Goal activation: live submission={}, durable submission={}, live retained={}, durable retained={}", live.last_plan_submission_id, persisted.behavior.last_plan_submission_id, live.retained_plan.is_some(), persisted.behavior.retained_plan.is_some());
    }).await;
}
