//! Contract tests for the unified foreground/FIFO/Goal idle pipeline.

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
async fn active_goal_keeps_an_idle_foreground_session_resident() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let state = actor.state.lock().await;
            assert!(!session_has_work(&state, None));
            assert!(session_has_work(
                &state,
                Some(crate::session::goal_tracker::GoalStatus::Active),
            ));
            assert!(!session_has_work(
                &state,
                Some(crate::session::goal_tracker::GoalStatus::Paused),
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unavailable_goal_harness_cannot_claim_a_stage_and_auto_pauses_on_refresh() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = std::sync::Arc::new(
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
            );
            actor.goal_tracker.lock().create_goal(
                "goal-no-tools".into(),
                "must fail closed".into(),
                None,
                0,
                chrono::Utc::now().to_rfc3339(),
                None,
            );
            let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

            actor.clone().drive_goal_on_idle(completion_tx).await;
            assert!(
                actor
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .unwrap()
                    .in_flight_stage
                    .is_none(),
                "an unproved harness must not claim planner work"
            );

            assert!(!actor.refresh_goal_harness_enabled().await);
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Paused)
            );
        })
        .await;
}

#[test]
fn goal_notification_filter_preserves_unrelated_background_results() {
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

#[test]
fn only_a_real_user_message_can_become_the_picker_goal_objective() {
    assert!(should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::User,
        true,
        false,
        "finish the refactor",
    ));
    assert!(!should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::NotificationDrain,
        true,
        false,
        "background command completed",
    ));
    assert!(!should_capture_implicit_goal_objective(
        &crate::session::PromptOrigin::User,
        true,
        true,
        "additional context",
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn delayed_goal_subagent_spawn_keeps_producer_stamped_ownership() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = std::sync::Arc::new(
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
            );
            actor.goal_tracker.lock().create_goal(
                "new-goal".into(),
                "do not inherit old planner usage".into(),
                None,
                0,
                chrono::Utc::now().to_rfc3339(),
                None,
            );

            actor
                .handle_grow_session_notification(GrowSessionNotification {
                    session_id: acp::SessionId::new(actor.session_id_string()),
                    update: GrowSessionUpdate::SubagentSpawned {
                        subagent_id: "old-planner".into(),
                        parent_session_id: actor.session_id_string(),
                        parent_prompt_id: None,
                        child_session_id: "old-planner".into(),
                        subagent_type: "general-purpose".into(),
                        description: "old Goal planner".into(),
                        effective_context_source: Some("new".into()),
                        context_normalized: false,
                        capability_mode: None,
                        persona: None,
                        role: None,
                        model: Some("test-model".into()),
                        resumed_from: None,
                        workflow_run_id: None,
                        goal_id: Some("old-goal".into()),
                    },
                    meta: None,
                })
                .await;

            assert_eq!(
                actor
                    .subagent_token_records
                    .lock()
                    .get("old-planner")
                    .and_then(|record| record.goal_id.as_deref()),
                Some("old-goal")
            );
            assert_eq!(
                actor
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .unwrap()
                    .subagent_tokens_spent,
                0
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn rewind_requires_goal_state_to_be_cleared_first() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.goal_tracker.lock().create_goal(
                "goal-rewind".into(),
                "keep history and blackboard coherent".into(),
                None,
                0,
                chrono::Utc::now().to_rfc3339(),
                None,
            );

            let response = actor
                .handle_rewind(crate::session::RewindRequest {
                    target_prompt_index: 0,
                    force: true,
                    mode: crate::session::RewindMode::All,
                })
                .await
                .expect("rewind request should be handled");

            assert!(!response.success);
            assert_eq!(
                response.error.as_deref(),
                Some("Cannot rewind while Goal state exists. Run /goal clear first.")
            );
            assert!(actor.goal_tracker.lock().snapshot().is_some());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn queued_row_steers_the_identified_turn_without_replacing_foreground() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            {
                let mut state = actor.state.lock().await;
                state.foreground = ForegroundState::RegularTurn(running_task_stub("turn-1"));
                state.pending_inputs.push_back(user_item("turn-1", "pager"));
                state
                    .pending_inputs
                    .push_back(user_item("queued-1", "pager"));
            }

            actor
                .handle_steer_queued_prompt("turn-1", "queued-1", 0, Some("pager"), None)
                .await;

            let state = actor.state.lock().await;
            assert_eq!(state.running_prompt_id(), Some("turn-1"));
            assert_eq!(state.pending_inputs.len(), 1);
            assert_eq!(state.pending_inputs[0].prompt_id, "turn-1");
            drop(state);
            let steered = actor.pending_interjections.snapshot();
            assert_eq!(steered.len(), 1);
            assert_eq!(steered[0].text, "text for queued-1");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stale_turn_identity_leaves_the_fifo_untouched() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            {
                let mut state = actor.state.lock().await;
                state.foreground = ForegroundState::RegularTurn(running_task_stub("turn-2"));
                state
                    .pending_inputs
                    .push_back(user_item("queued-1", "pager"));
            }

            actor
                .handle_steer_queued_prompt("turn-1", "queued-1", 0, Some("pager"), None)
                .await;

            let state = actor.state.lock().await;
            assert_eq!(state.running_prompt_id(), Some("turn-2"));
            assert_eq!(state.pending_inputs.len(), 1);
            assert_eq!(state.pending_inputs[0].prompt_id, "queued-1");
            assert!(actor.pending_interjections.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn user_fifo_wins_over_goal_idle_continuation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = std::sync::Arc::new(
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
            );
            {
                let mut goal = actor.goal_tracker.lock();
                goal.create_goal(
                    "goal-1".into(),
                    "finish the refactor".into(),
                    None,
                    0,
                    chrono::Utc::now().to_rfc3339(),
                    None,
                );
                assert!(goal.replace_plan(
                    "- [ ] implement".into(),
                    crate::session::goal_tracker::GoalPlanAuthor::Agent,
                    None,
                ));
            }
            actor
                .state
                .lock()
                .await
                .pending_inputs
                .push_back(user_item("user-1", "pager"));
            let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

            actor.clone().drive_goal_on_idle(completion_tx).await;

            let state = actor.state.lock().await;
            assert!(state.foreground.is_idle());
            assert_eq!(state.pending_inputs.len(), 1);
            assert_eq!(state.pending_inputs[0].prompt_id, "user-1");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn finished_goal_subagent_tokens_are_settled_once_and_persisted() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.goal_tracker.lock().create_goal(
                "goal-token".into(),
                "persist subagent usage".into(),
                None,
                100,
                chrono::Utc::now().to_rfc3339(),
                None,
            );
            actor.subagent_token_records.lock().insert(
                "planner-1".into(),
                SubagentTokenRecord {
                    goal_id: Some("goal-token".into()),
                    resume_anchor_cumulative: 0,
                    settled_cumulative: 0,
                    last_cumulative_reported: 300,
                    model: Some("test-model".into()),
                    finished: false,
                },
            );

            assert_eq!(actor.goal_tokens(150), (350, 0));
            assert_eq!(
                actor.settle_goal_subagent_tokens("planner-1", 400),
                Some(400),
            );
            assert_eq!(actor.goal_tokens(150), (450, 400));
            assert_eq!(
                actor.settle_goal_subagent_tokens("planner-1", 500),
                Some(100),
                "a ratcheted duplicate charges only the new terminal delta",
            );
            assert_eq!(
                actor
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .unwrap()
                    .subagent_tokens_spent,
                500,
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn complete_goal_receipt_ignores_late_subagent_accounting() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            {
                let mut tracker = actor.goal_tracker.lock();
                tracker.create_goal(
                    "goal-complete".into(),
                    "freeze accounting".into(),
                    None,
                    0,
                    chrono::Utc::now().to_rfc3339(),
                    None,
                );
                assert!(tracker.replace_plan(
                    "- [x] done".into(),
                    crate::session::goal_tracker::GoalPlanAuthor::Agent,
                    None,
                ));
                assert!(tracker.candidate_complete("done".into()));
                let lease = tracker
                    .claim_stage(crate::session::goal_tracker::GoalPhase::Verifying)
                    .unwrap();
                assert!(tracker.verification_achieved(&lease));
                assert!(tracker.complete_verified());
            }
            actor.subagent_token_records.lock().insert(
                "late-child".into(),
                SubagentTokenRecord {
                    goal_id: Some("goal-complete".into()),
                    resume_anchor_cumulative: 0,
                    settled_cumulative: 0,
                    last_cumulative_reported: 100,
                    model: None,
                    finished: false,
                },
            );

            assert_eq!(actor.settle_goal_subagent_tokens("late-child", 500), None);
            assert_eq!(
                actor
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .unwrap()
                    .subagent_tokens_spent,
                0
            );
            assert!(
                !actor
                    .subagent_token_records
                    .lock()
                    .contains_key("late-child")
            );
        })
        .await;
}
