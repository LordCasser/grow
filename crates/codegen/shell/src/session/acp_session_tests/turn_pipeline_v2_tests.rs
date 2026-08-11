//! Contract tests for the unified foreground/FIFO/Goal idle pipeline.

use super::support::*;
use super::turn::should_capture_implicit_goal_objective;
use super::*;

fn canonical_goal_board(objective: &str, done: bool) -> String {
    let (checkbox, status) = if done {
        ("x", "done")
    } else {
        (" ", "in_progress")
    };
    format!(
        "# Goal\n\n> {objective}\n\n## Plan\n\n- [{checkbox}] **T1** `{status}` — Implement safely\n  - Scope: runtime\n  - Acceptance: tests pass\n\n## Goal acceptance\n\n- Tests pass\n\n## Verification evidence\n\n- Pending\n\n## Open gaps\n\n- None"
    )
}

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
                        permission_mode: None,
                        effective_permission_mode: None,
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
async fn delegated_goal_context_propagates_to_nested_subagent_ownership() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use tools::implementations::grow_build::task::types::{
                CurrentSubagentOwnerResource, GoalSubagentRole, SubagentOwner,
            };
            use tools::implementations::grow_build::update_goal::{
                GoalContextSnapshot, GoalContextSnapshotResource,
            };

            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.goal_tracker.lock().create_goal(
                "delegated-goal".into(),
                "preserve ownership".into(),
                None,
                0,
                chrono::Utc::now().to_rfc3339(),
                None,
            );
            let view = super::goal::goal_view_from_snapshot(
                actor.goal_tracker.lock().snapshot().unwrap(),
                0,
            );
            let bridge = actor.agent.borrow().tool_bridge().clone();
            bridge
                .update_resource(GoalContextSnapshotResource(Some(GoalContextSnapshot {
                    role: GoalSubagentRole::Worker,
                    view: view.clone(),
                })))
                .await;

            actor
                .publish_turn_scope_resources(
                    "nested-parent-turn".into(),
                    &crate::session::PromptOrigin::User,
                    tool_types::BehaviorId::Normal,
                )
                .await;

            let owner = bridge
                .read_resource::<CurrentSubagentOwnerResource>()
                .await
                .expect("turn ownership resource");
            assert_eq!(
                owner.0,
                SubagentOwner::goal(
                    view.goal_id,
                    view.objective_revision,
                    view.plan_revision,
                    view.board_revision,
                    GoalSubagentRole::Worker,
                )
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
                let lease = goal
                    .claim_stage(crate::session::goal_tracker::GoalPhase::Planning)
                    .unwrap();
                assert!(
                    goal.apply_planner_result(
                        &lease,
                        canonical_goal_board("finish the refactor", false)
                    )
                    .unwrap()
                );
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
                let planner = tracker
                    .claim_stage(crate::session::goal_tracker::GoalPhase::Planning)
                    .unwrap();
                assert!(
                    tracker
                        .apply_planner_result(
                            &planner,
                            canonical_goal_board("freeze accounting", true)
                        )
                        .unwrap()
                );
                assert!(tracker.candidate_complete(1, 1, "done".into()).unwrap());
                let lease = tracker
                    .claim_stage(crate::session::goal_tracker::GoalPhase::Verifying)
                    .unwrap();
                assert!(tracker.verification_achieved(&lease).unwrap());
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

#[test]
fn user_turn_infra_failure_does_not_degrade_an_active_goal() {
    let result: PromptTurnResult =
        Err(acp::Error::internal_error().data("provider rejected request"));
    let (_, _, pause) = SessionActor::post_turn_goal_degradation_plan(
        &result,
        Some(&crate::session::PromptOrigin::User),
    );
    assert!(
        pause.is_none(),
        "a user-owned turn failure must not be attributed to the Goal runtime"
    );

    let (_, _, pause) = SessionActor::post_turn_goal_degradation_plan(
        &result,
        Some(&crate::session::PromptOrigin::GoalContinuation {
            goal_id: "goal-1".into(),
            stage_id: 7,
        }),
    );
    assert!(
        pause.is_some(),
        "the same failure on a Goal-owned continuation must remain actionable"
    );
}

#[test]
fn user_turn_terminal_anomalies_do_not_pause_or_suppress_goal_runtime() {
    let result = |stop_reason, completion_kind| {
        Ok(crate::session::commands::PromptTurnOk {
            stop_reason,
            total_tokens: 0,
            turn_snapshot: None,
            completion_kind,
            structured_output: None,
            usage: None,
        })
    };
    for outcome in [
        result(
            acp::StopReason::Refusal,
            crate::session::commands::PromptCompletionKind::Completed,
        ),
        result(
            acp::StopReason::EndTurn,
            crate::session::commands::PromptCompletionKind::StationarityEnded,
        ),
        result(
            acp::StopReason::MaxTokens,
            crate::session::commands::PromptCompletionKind::MaxTurnsReached { limit: 3 },
        ),
    ] {
        let (_, suppress, pause) = SessionActor::post_turn_goal_degradation_plan(
            &outcome,
            Some(&crate::session::PromptOrigin::User),
        );
        assert!(!suppress);
        assert!(pause.is_none());
    }
}

#[test]
fn goal_internal_terminal_anomalies_pause_without_hot_looping() {
    let origin = crate::session::PromptOrigin::GoalContinuation {
        goal_id: "goal-1".into(),
        stage_id: 9,
    };
    let result = |stop_reason, completion_kind| {
        Ok(crate::session::commands::PromptTurnOk {
            stop_reason,
            total_tokens: 0,
            turn_snapshot: None,
            completion_kind,
            structured_output: None,
            usage: None,
        })
    };
    let (_, suppress, pause) = SessionActor::post_turn_goal_degradation_plan(
        &result(
            acp::StopReason::Refusal,
            crate::session::commands::PromptCompletionKind::Completed,
        ),
        Some(&origin),
    );
    assert!(!suppress);
    assert!(pause.is_some());

    let (_, suppress, pause) = SessionActor::post_turn_goal_degradation_plan(
        &result(
            acp::StopReason::EndTurn,
            crate::session::commands::PromptCompletionKind::StationarityEnded,
        ),
        Some(&origin),
    );
    assert!(suppress);
    assert!(pause.is_some());

    let (_, suppress, pause) = SessionActor::post_turn_goal_degradation_plan(
        &result(
            acp::StopReason::MaxTokens,
            crate::session::commands::PromptCompletionKind::MaxTurnsReached { limit: 3 },
        ),
        Some(&origin),
    );
    assert!(!suppress);
    assert!(pause.is_some());
}

#[test]
fn goal_finalization_requires_a_real_successful_report_terminal() {
    let result = |stop_reason, completion_kind| {
        Ok(crate::session::commands::PromptTurnOk {
            stop_reason,
            total_tokens: 0,
            turn_snapshot: None,
            completion_kind,
            structured_output: None,
            usage: None,
        })
    };
    assert!(SessionActor::goal_finalization_terminal_succeeded(&result(
        acp::StopReason::EndTurn,
        crate::session::commands::PromptCompletionKind::Completed,
    )));
    assert!(!SessionActor::goal_finalization_terminal_succeeded(
        &result(
            acp::StopReason::Refusal,
            crate::session::commands::PromptCompletionKind::Completed,
        )
    ));
    assert!(!SessionActor::goal_finalization_terminal_succeeded(
        &result(
            acp::StopReason::EndTurn,
            crate::session::commands::PromptCompletionKind::StationarityEnded,
        )
    ));
    assert!(!SessionActor::goal_finalization_terminal_succeeded(
        &result(
            acp::StopReason::Cancelled,
            crate::session::commands::PromptCompletionKind::Cancelled {
                category: None,
                context: None,
            },
        )
    ));
}

#[test]
fn goal_internal_stationarity_suppresses_the_next_goal_idle_continuation() {
    let result = Ok(crate::session::commands::PromptTurnOk {
        stop_reason: acp::StopReason::EndTurn,
        total_tokens: 0,
        turn_snapshot: None,
        completion_kind: crate::session::commands::PromptCompletionKind::StationarityEnded,
        structured_output: None,
        usage: None,
    });
    let (succeeded, suppress, pause) = SessionActor::post_turn_goal_degradation_plan(
        &result,
        Some(&crate::session::PromptOrigin::GoalContinuation {
            goal_id: "goal-1".into(),
            stage_id: 2,
        }),
    );
    assert!(succeeded);
    assert!(suppress);
    assert!(pause.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn goal_tool_schema_reaches_the_sampling_spec_as_an_object() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            *actor.agent.borrow_mut() =
                test_agent_with_tools(vec![tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::GetGoalTool,
                >()])
                .await;
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);

            let definitions = actor.prepare_tool_definitions_inner().await;
            let definition = definitions
                .iter()
                .find(|definition| {
                    definition.function.name
                        == tools::implementations::grow_build::GET_GOAL_TOOL_NAME
                })
                .expect("Goal Behavior must advertise get_goal");
            assert_eq!(definition.function.parameters["type"], "object");
            assert_eq!(
                definition.function.parameters["properties"],
                serde_json::json!({})
            );

            let specs = actor.turn_base_tool_specs(&definitions);
            let spec = specs
                .iter()
                .find(|spec| spec.name == tools::implementations::grow_build::GET_GOAL_TOOL_NAME)
                .expect("get_goal must survive conversion into the sampling request");
            assert_eq!(spec.parameters["type"], "object");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn delegated_goal_worker_keeps_read_only_snapshot_tool_outside_goal_behavior() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::GetGoalTool,
                >(),
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::UpdateGoalProgressTool,
                >(),
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::RequestGoalReplanTool,
                >(),
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::UpdateGoalTool,
                >(),
            ])
            .await;
            actor
                .agent
                .borrow()
                .tool_bridge()
                .update_resource(
                    tools::implementations::grow_build::update_goal::GoalContextSnapshotResource(
                        Some(
                            tools::implementations::grow_build::update_goal::GoalContextSnapshot {
                                role: tools::implementations::grow_build::task::types::GoalSubagentRole::Worker,
                                view: tools::implementations::grow_build::update_goal::GoalView {
                                    goal_id: "goal-1".into(),
                                    objective: "ship".into(),
                                    objective_revision: 0,
                                    status: "active".into(),
                                    phase: "executing".into(),
                                    token_budget: None,
                                    tokens_used: 0,
                                    plan_revision: 0,
                                    board_revision: 1,
                                    tasks: Vec::new(),
                                    plan_markdown: String::new(),
                                    verifier_feedback: None,
                                },
                            },
                        ),
                    ),
                )
                .await;

            let names: std::collections::HashSet<_> = actor
                .prepare_tool_definitions_inner()
                .await
                .into_iter()
                .map(|definition| definition.function.name)
                .collect();
            assert!(names.contains(tools::implementations::grow_build::GET_GOAL_TOOL_NAME));
            for mutation in [
                tools::implementations::grow_build::UPDATE_GOAL_PROGRESS_TOOL_NAME,
                tools::implementations::grow_build::REQUEST_GOAL_REPLAN_TOOL_NAME,
                tools::implementations::grow_build::UPDATE_GOAL_TOOL_NAME,
            ] {
                assert!(!names.contains(mutation), "delegated worker exposed {mutation}");
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn active_turn_tool_surface_is_pinned_to_its_captured_behavior() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            *actor.agent.borrow_mut() =
                test_agent_with_tools(vec![tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::GetGoalTool,
                >()])
                .await;

            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            *actor.turn_behavior.lock() = tool_types::BehaviorId::Normal;
            actor
                .session_turn_active
                .store(true, std::sync::atomic::Ordering::Relaxed);

            let active_defs = actor.prepare_tool_definitions_inner().await;
            assert!(
                active_defs.iter().all(|definition| {
                    definition.function.name
                        != tools::implementations::grow_build::GET_GOAL_TOOL_NAME
                }),
                "a Normal turn must not inherit Goal tools from a mid-turn Behavior switch"
            );

            actor
                .session_turn_active
                .store(false, std::sync::atomic::Ordering::Relaxed);
            let idle_defs = actor.prepare_tool_definitions_inner().await;
            assert!(idle_defs.iter().any(|definition| {
                definition.function.name == tools::implementations::grow_build::GET_GOAL_TOOL_NAME
            }));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn behavior_tool_surfaces_are_filtered_by_taxonomy_even_when_tools_are_renamed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let mut goal = tools::registry::types::ToolConfig::for_tool::<
                tools::implementations::grow_build::GetGoalTool,
            >();
            goal.name_override = Some("renamed_goal_reader".into());
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                goal,
                tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::plan_control::PlanControlTool,
                >(),
            ])
            .await;

            let normal: std::collections::HashSet<_> = actor
                .prepare_tool_definitions_inner()
                .await
                .into_iter()
                .map(|definition| definition.function.name)
                .collect();
            assert!(!normal.contains("renamed_goal_reader"));
            assert!(!normal.contains("plan_control"));

            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            let goal_behavior: std::collections::HashSet<_> = actor
                .prepare_tool_definitions_inner()
                .await
                .into_iter()
                .map(|definition| definition.function.name)
                .collect();
            assert!(goal_behavior.contains("renamed_goal_reader"));
            assert!(!goal_behavior.contains("plan_control"));

            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Plan);
            let plan: std::collections::HashSet<_> = actor
                .prepare_tool_definitions_inner()
                .await
                .into_iter()
                .map(|definition| definition.function.name)
                .collect();
            assert!(plan.contains("plan_control"));
            assert!(!plan.contains("renamed_goal_reader"));
        })
        .await;
}
