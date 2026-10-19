// Temporarily include inside session/actor/tool/mod.rs under #[cfg(test)].
#[tokio::test(flavor = "current_thread")]
async fn audit_v240_manual_behavior_keeps_deferred_candidate_durable() {
    tokio::task::LocalSet::new().run_until(async {
        use crate::session::behavior::{RetainedPlanAction, RetainedPlanDecisionOutcome, RetainedPlanStatus};
        let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
        let initial = {
            let mut behavior = actor.behavior.lock();
            behavior.select_behavior(tool_types::BehaviorId::Plan);
            behavior.record_plan_artifact("# Deferred plan");
            assert!(behavior.submit_initial_plan());
            let expected = behavior.snapshot();
            assert_eq!(behavior.decide_submitted_plan_if(&expected,
                expected.plan_submission_id.unwrap(), "defer1", RetainedPlanAction::Defer, None),
                RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred));
            behavior.snapshot()
        };
        actor.persist_control_snapshot_durably(initial, None).await.unwrap();
        let confirmation = actor.request_behavior_change(
            acp_transport::protocol::SessionModeId::new("normal")
        ).await.unwrap();
        assert!(matches!(confirmation,
            crate::session::behavior::BehaviorChangeOutcome::ConfirmationRequired { .. }),
            "leaving a drafted Plan must honor confirmation: {confirmation:?}");
        let applied = actor.request_behavior_change(
            acp_transport::protocol::SessionModeId::new("normal")
        ).await.unwrap();
        assert!(matches!(applied, crate::session::behavior::BehaviorChangeOutcome::Applied),
            "confirmed switch must apply: {applied:?}");
        let live = actor.behavior.lock().snapshot();
        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        let saved = crate::session::control::SessionControlSnapshot::latest_from_timeline(&events)
            .unwrap().unwrap().behavior;
        eprintln!("Deferred candidate after manual change: live={}, durable={}",
            live.retained_plan.is_some(), saved.retained_plan.is_some());
        assert!(live.retained_plan.is_some());
        assert_eq!(saved.retained_plan, live.retained_plan,
            "latest acknowledged Control must retain the deferred candidate before a crash/reload");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn audit_v240_manual_behavior_preserves_plan_identity_counter() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
            {
                let mut behavior = actor.behavior.lock();
                behavior.select_behavior(tool_types::BehaviorId::Plan);
                behavior.record_plan_artifact("# Approved plan");
                assert!(behavior.submit_initial_plan());
                assert!(behavior.approve_submitted_plan());
            }
            let expected = actor.behavior.lock().snapshot();
            assert!(
                actor
                    .finish_plan_to_default_if(&expected, Some("Completed report"))
                    .await
                    .unwrap()
            );
            assert!(matches!(
                actor
                    .request_behavior_change(acp_transport::protocol::SessionModeId::new("ask"))
                    .await
                    .unwrap(),
                crate::session::behavior::BehaviorChangeOutcome::Applied
            ));
            let live = actor.behavior.lock().snapshot();
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let saved =
                crate::session::control::SessionControlSnapshot::latest_from_timeline(&events)
                    .unwrap()
                    .unwrap()
                    .behavior;
            eprintln!(
                "Plan identity after manual change: live={}, durable={}",
                live.last_plan_submission_id, saved.last_plan_submission_id
            );
            assert_eq!(
                saved, live,
                "cold restore must preserve the acknowledged live control state"
            );
        })
        .await;
}
