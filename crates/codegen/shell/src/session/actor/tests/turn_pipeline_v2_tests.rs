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
async fn completed_runner_keeps_foreground_fenced_until_terminal_settlement() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut foreground =
                ForegroundState::RegularTurn(running_task_stub("turn-before-terminal"));
            let task = foreground
                .begin_settling()
                .expect("regular turn enters settlement");
            assert_eq!(task.prompt_id, "turn-before-terminal");
            assert!(!foreground.is_idle());
            assert!(foreground.regular().is_none());
            assert!(!foreground.finish_settling("different-turn"));
            assert!(!foreground.is_idle());
            assert!(foreground.finish_settling("turn-before-terminal"));
            assert!(foreground.is_idle());
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
    assert!(
        !should_capture_implicit_goal_objective(
            &crate::session::PromptOrigin::User,
            true,
            Some(crate::session::goal_tracker::GoalStatus::Complete),
            "start the next goal",
        ),
        "a completed Goal must be explicitly edited or cleared before another objective is captured"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_goal_objective_commits_its_turn_terminal_before_continuation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = std::sync::Arc::new(
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
            );
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);

            let result = actor
                .handle_prompt(
                    "goal-objective",
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                        "finish the release audit",
                    ))],
                    tool_types::BehaviorId::Goal,
                    None,
                    None,
                    false,
                    None,
                    None,
                )
                .await
                .expect("implicit Goal objective should be admitted");

            assert!(matches!(
                result.completion_kind,
                crate::session::commands::PromptCompletionKind::Completed
            ));
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Active)
            );
            assert_eq!(
                actor.events.current_turn(),
                None,
                "the Goal continuation may only be armed after its user turn is durably closed"
            );
        })
        .await;
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
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-1".into(),
                    "replace the architecture and verify every feature".into(),
                    None,
                    "2026-08-24T00:00:00Z".into(),
                )
                .unwrap();

            let directive = actor
                .render_goal_continuation(0)
                .expect("active Goal continuation");
            assert!(directive.contains("BEGIN WITH A COMPLETION AUDIT"));
            assert!(directive.contains("complete objective"));
            assert!(directive.contains("todo_write"));
            assert!(directive.contains("task tool"));
            assert!(directive.contains("not a second Goal state"));
            assert!(directive.contains("narrow or replace the full objective"));
            assert!(directive.contains("authoritative current evidence"));
            assert!(directive.contains("three consecutive Goal turns"));
            assert!(directive.contains("token budget: unlimited"));
            assert!(!directive.contains("planner"));
            assert!(!directive.contains("verifier"));
            assert_eq!(
                actor
                    .active_goal_directive_tag()
                    .expect("active Goal projection")
                    .definition_revision,
                1
            );

            actor
                .goal_tracker
                .lock()
                .pause(crate::session::goal_tracker::GoalPauseReason::User);
            assert!(actor.render_goal_continuation(0).is_none());
            assert!(actor.active_goal_directive_tag().is_none());
        })
        .await;
}

#[test]
fn goal_runtime_requires_the_local_task_planner() {
    use tools::implementations::grow_build::{
        CREATE_GOAL_TOOL_NAME, GET_GOAL_TOOL_NAME, UPDATE_GOAL_TOOL_NAME,
    };

    let lifecycle_only = vec![
        CREATE_GOAL_TOOL_NAME.to_string(),
        GET_GOAL_TOOL_NAME.to_string(),
        UPDATE_GOAL_TOOL_NAME.to_string(),
    ];
    assert!(
        !super::goal_support::goal_runtime_available_from_tools(true, &lifecycle_only),
        "Goal must not advertise a task-first contract without todo_write"
    );

    let mut complete = lifecycle_only;
    complete.push("todo_write".into());
    assert!(super::goal_support::goal_runtime_available_from_tools(
        true, &complete
    ));
    assert!(!super::goal_support::goal_runtime_available_from_tools(
        false, &complete
    ));
}
