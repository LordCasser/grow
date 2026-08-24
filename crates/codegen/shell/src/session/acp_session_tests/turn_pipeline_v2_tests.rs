//! Contract tests for long-term Goal admission and idle continuation.

use super::support::*;
use super::turn::should_capture_implicit_goal_objective;
use super::*;

#[tokio::test(flavor = "current_thread")]
async fn foreground_snapshot_carries_origin_and_kind_without_parsing_its_id() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let foreground = ForegroundState::RegularTurn(running_task_stub("opaque-turn-id"));
            let snapshot = foreground.snapshot().expect("regular foreground");
            assert_eq!(snapshot.prompt_id, "opaque-turn-id");
            assert_eq!(snapshot.origin, "user");
            assert_eq!(snapshot.turn_kind, "user");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn only_active_goal_keeps_an_idle_session_resident() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let state = actor.state.lock().await;
            assert!(!session_has_work(&state, None, false));
            assert!(session_has_work(
                &state,
                Some(crate::session::goal_tracker::GoalStatus::Active),
                false,
            ));
            for stopped in [
                crate::session::goal_tracker::GoalStatus::Paused,
                crate::session::goal_tracker::GoalStatus::Blocked,
                crate::session::goal_tracker::GoalStatus::UsageLimited,
                crate::session::goal_tracker::GoalStatus::BudgetLimited,
                crate::session::goal_tracker::GoalStatus::Complete,
            ] {
                assert!(!session_has_work(&state, Some(stopped), false));
            }
        })
        .await;
}

#[test]
fn only_real_user_input_can_supply_an_implicit_goal_objective() {
    assert!(should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::User,
        true,
        None,
        "finish the refactor",
    ));
    assert!(!should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::NotificationDrain,
        true,
        None,
        "background command completed",
    ));
    assert!(!should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::User,
        true,
        Some(crate::session::goal_tracker::GoalStatus::Active),
        "additional context",
    ));
    assert!(should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::User,
        true,
        Some(crate::session::goal_tracker::GoalStatus::Complete),
        "start the next goal",
    ));
}

#[test]
fn goal_task_completion_filter_preserves_unrelated_background_results() {
    fn pending(task_id: &str) -> PendingNotification {
        PendingNotification {
            prompt_id: format!("notification-{task_id}"),
            prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(task_id))],
            priority: NotificationPriority::Next,
            source: NotificationSource::BashTaskCompleted {
                task_id: task_id.to_string(),
            },
        }
    }

    let goal_owned = std::collections::HashSet::from(["goal-task".to_string()]);
    let (surfaced, dropped) = SessionActor::split_goal_suppressed(
        &goal_owned,
        vec![pending("goal-task"), pending("user-task")],
    );
    assert_eq!(dropped, 1);
    assert_eq!(surfaced.len(), 1);
    assert_eq!(surfaced[0].source.task_id(), "user-task");
}

#[tokio::test(flavor = "current_thread")]
async fn every_continuation_audits_the_full_goal_before_planning_a_local_slice() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-1".into(),
                    "replace the architecture and verify every feature".into(),
                    None,
                    0,
                    "2026-08-24T00:00:00Z".into(),
                )
                .unwrap();

            let directive = actor
                .render_goal_continuation(0)
                .expect("active Goal continuation");
            assert!(directive.contains("BEGIN WITH A COMPLETION AUDIT"));
            assert!(directive.contains("entire objective"));
            assert!(directive.contains("todo_write"));
            assert!(directive.contains("task tool"));
            assert!(directive.contains("not a second Goal state"));
            assert!(directive.contains("must not replace or narrow the full objective"));
            assert!(directive.contains("full objective is achieved"));
            assert!(!directive.contains("planner"));
            assert!(!directive.contains("verifier"));

            actor
                .goal_tracker
                .lock()
                .pause(crate::session::goal_tracker::GoalPauseReason::User);
            assert!(actor.render_goal_continuation(0).is_none());
        })
        .await;
}
