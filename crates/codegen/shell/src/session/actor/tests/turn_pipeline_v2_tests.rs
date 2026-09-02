//! Contract tests for long-term Goal admission and idle continuation.

use super::support::*;
use super::turn::should_capture_implicit_goal_objective;
use super::*;

/// Match the dedicated production session thread's stack. The full prompt
/// admission future is intentionally large and does not fit the test harness'
/// smaller default thread stack in debug builds.
fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
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
async fn goal_failure_keeps_its_origin_across_terminalization() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut task = running_task_stub("goal-terminal");
            task.origin = crate::session::PromptOrigin::GoalContinuation {
                goal_id: "goal-1".into(),
                definition_revision: 7,
            };
            task.turn_kind = crate::session::TurnKind::Internal;
            let mut foreground = ForegroundState::RegularTurn(task);
            assert!(foreground.begin_terminalization("goal-terminal"));
            let origin = foreground
                .identity("goal-terminal")
                .map(|(origin, _)| origin)
                .expect("settling Goal keeps structured origin");
            let result: PromptTurnResult =
                Err(acp::Error::internal_error().data("provider failed"));
            let (_, suppress, goal_stop) =
                SessionActor::post_turn_goal_degradation_plan(&result, Some(&origin));
            assert!(!suppress);
            assert!(
                goal_stop.is_some(),
                "Goal-owned failure must stop continuation"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn active_control_runtimes_keep_an_idle_session_resident() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let state = actor.state.lock().await;
            assert!(!session_has_work(&state, None, false, false));
            assert!(session_has_work(
                &state,
                Some(crate::session::goal_tracker::GoalStatus::Active),
                false,
                false,
            ));
            assert!(
                session_has_work(&state, None, true, false),
                "an active Workflow run must keep its owning session resident"
            );
            for stopped in [
                crate::session::goal_tracker::GoalStatus::Paused,
                crate::session::goal_tracker::GoalStatus::Blocked,
                crate::session::goal_tracker::GoalStatus::BudgetLimited,
                crate::session::goal_tracker::GoalStatus::Complete,
            ] {
                assert!(!session_has_work(&state, Some(stopped), false, false));
            }
            drop(state);
            let mut state = actor.state.lock().await;
            state.behavior_control_worker_active = true;
            assert!(
                session_has_work(&state, None, false, false),
                "an idle Behavior worker must fence actor unload"
            );
            state.behavior_control_worker_active = false;
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
            install_test_foreground(&actor, "goal-objective").await;

            let result = actor
                .handle_prompt(
                    "goal-objective",
                    crate::session::actor::tests::support::admit_test_human_input(
                        &actor,
                        "goal-objective",
                    )
                    .await,
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

#[test]
fn autonomous_first_turn_commits_the_deferred_prefix_before_turn_started() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor.deferred_prefix.arm(tokio::task::spawn_local(async {
                "<user_info>deferred bootstrap prefix</user_info>".to_string()
            }));
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-1".into(),
                    "continue autonomously".into(),
                    None,
                    "2026-08-27T00:00:00Z".into(),
                )
                .unwrap();
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            let event_count_before = actor
                .chat_state_handle
                .timeline_events()
                .await
                .unwrap()
                .len();
            install_test_foreground(&actor, "first-goal-continuation").await;

            actor
                .handle_prompt(
                    "first-goal-continuation",
                    Vec::new(),
                    crate::session::PromptOrigin::GoalContinuation {
                        goal_id: "goal-1".into(),
                        definition_revision: 1,
                    },
                    Vec::new(),
                    crate::session::TurnKind::Internal,
                    vec![acp::ContentBlock::Text(acp::TextContent::new("/context"))],
                    tool_types::BehaviorId::Goal,
                    None,
                    None,
                    true,
                    None,
                    None,
                )
                .await
                .expect("autonomous first turn must cross the bootstrap barrier");

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let appended = &events[event_count_before..];
            let prefix = appended
                .iter()
                .position(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Messages(messages)
                            if messages.cause == chat_state::MessageCause::ContextRebuild
                    )
                })
                .expect("deferred prefix ContextRebuild");
            let turn = appended
                .iter()
                .position(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started {
                            identity,
                            ..
                        }) if identity.origin == "goal_continuation"
                    )
                })
                .expect("Goal continuation TurnStarted");
            assert!(
                prefix < turn,
                "ContextRebuild must precede the first prompt"
            );
            assert!(!appended[turn + 1..].iter().any(|event| {
                matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Messages(messages)
                        if messages.cause == chat_state::MessageCause::ContextRebuild
                )
            }));
        }));
    });
}

#[tokio::test(flavor = "current_thread")]
async fn paused_unbudgeted_goal_with_incomplete_usage_can_restart_explicitly() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use tools::implementations::grow_build::{
                CreateGoalTool, GetGoalTool, UpdateGoalTool, todo::TodoWriteTool,
            };
            use tools::registry::types::ToolConfig;

            let (mut actor, _gateway_rx) = build_actor().await;
            std::sync::Arc::get_mut(&mut actor)
                .expect("fixture has one actor owner")
                .goal_enabled = true;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                ToolConfig::for_tool::<CreateGoalTool>(),
                ToolConfig::for_tool::<GetGoalTool>(),
                ToolConfig::for_tool::<UpdateGoalTool>(),
                ToolConfig::for_tool::<TodoWriteTool>(),
            ])
            .await;
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-1".into(),
                    "continue after an interrupted provider request".into(),
                    None,
                    "2026-08-27T00:00:00Z".into(),
                )
                .unwrap();
            assert!(actor.goal_tracker.lock().mark_usage_incomplete("goal-1"));
            assert!(
                actor
                    .goal_tracker
                    .lock()
                    .pause(crate::session::goal_tracker::GoalPauseReason::User)
            );
            actor.sync_goal_usage_window();
            begin_test_causal_turn(&actor).await;

            let message = actor.restart_goal().await;

            assert!(message.starts_with("Goal restarted."), "{message}");
            assert!(message.contains("lower bound"), "{message}");
            let goal = actor.goal_tracker.lock().snapshot().cloned().unwrap();
            assert_eq!(
                goal.status,
                crate::session::goal_tracker::GoalStatus::Active
            );
            assert!(goal.usage_incomplete);
            assert!(goal.usage_incomplete_acknowledged);
            assert_eq!(
                actor.goal_usage_window.active_goal_id().as_deref(),
                Some("goal-1"),
                "the restart command's own active Step must not re-close admission"
            );
            assert_eq!(
                actor.behavior.lock().behavior(),
                tool_types::BehaviorId::Goal
            );
            assert!(
                !actor.enforce_goal_spending_limit().await,
                "an explicitly acknowledged lower bound must not immediately pause again"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unexpected_turn_owner_failure_closes_every_open_causal_child() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-1".into(),
                    "survive owner failure".into(),
                    None,
                    "2026-08-27T00:00:00Z".into(),
                )
                .unwrap();
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            actor.sync_goal_usage_window();
            let usage_epoch = actor
                .goal_usage_window
                .owner_epoch(&actor.session_id_string());
            actor
                .goal_usage_window
                .begin_model_attempt(&actor.session_id_string(), usage_epoch, Some("goal-1"))
                .await
                .unwrap()
                .expect("Goal-owned provider attempt");
            install_test_foreground(&actor, "panicked-turn").await;
            begin_test_causal_turn(&actor).await;
            actor
                .events
                .request_started("request-1".into(), "model".into(), 1, 1)
                .await
                .unwrap();
            actor
                .events
                .tool_started("read_file".into(), "tool-1".into(), None)
                .await
                .unwrap();

            actor
                .recover_panicked_turn("panicked-turn", "fixture panic")
                .await
                .unwrap();

            tokio::time::timeout(
                std::time::Duration::from_millis(100),
                actor
                    .goal_usage_window
                    .wait_for_owner_settlements_through(&actor.session_id_string(), usage_epoch),
            )
            .await
            .expect("panic recovery must not strand the provider attempt");
            assert!(
                actor
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .unwrap()
                    .usage_incomplete
            );

            let timeline = chat_state::Timeline::from_events(
                actor.chat_state_handle.timeline_events().await.unwrap(),
            )
            .unwrap();
            assert!(timeline.active_turn().is_none());
            assert!(timeline.active_step().is_none());
            assert!(timeline.open_request_ids().next().is_none());
            assert!(timeline.open_tool_call_ids().next().is_none());

            actor
                .handle_completion(
                    "panicked-turn".into(),
                    Err(acp::Error::internal_error().data("fixture panic")),
                )
                .await;
            assert!(actor.state.lock().await.foreground.is_idle());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_cancel_closes_request_tool_step_then_turn() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            install_test_foreground(&actor, "cancelled-turn").await;
            begin_test_causal_turn(&actor).await;
            actor
                .events
                .request_started("request-1".into(), "model".into(), 1, 1)
                .await
                .unwrap();
            actor
                .events
                .tool_started("read_file".into(), "tool-1".into(), None)
                .await
                .unwrap();

            actor
                .cancel_running_task(false, false, false, Some("ctrl_c".into()))
                .await
                .unwrap();

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let request_end = events
                .iter()
                .position(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Request(
                            chat_state::RequestEvent::Cancelled { id, .. }
                        ) if id == "request-1"
                    )
                })
                .expect("request terminal");
            let tool_end = events
                .iter()
                .position(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Tool(
                            chat_state::ToolEvent::Completed { call_id, .. }
                        ) if call_id == "tool-1"
                    )
                })
                .expect("tool terminal");
            let step_end = events
                .iter()
                .position(|event| {
                    matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Step(chat_state::StepEvent::Ended { .. })
                    )
                })
                .expect("step terminal");
            let turn_end = events
                .iter()
                .position(|event| {
                    matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. })
                    )
                })
                .expect("turn terminal");
            assert!(request_end < step_end);
            assert!(tool_end < step_end);
            assert!(step_end < turn_end);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn panic_after_durable_turn_terminal_never_appends_a_second_terminal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            install_test_foreground(&actor, "post-terminal-panic").await;
            begin_test_causal_turn(&actor).await;
            assert!(
                actor
                    .state
                    .lock()
                    .await
                    .foreground
                    .begin_terminalization("post-terminal-panic")
            );
            actor
                .emit_turn_ended(
                    crate::session::events::TurnOutcomeLabel::Completed,
                    chat_state::TurnTerminal {
                        stop_reason: "completed".into(),
                        completion_kind: "completed".into(),
                    },
                    None,
                    None,
                )
                .await
                .unwrap();
            let before = actor.chat_state_handle.timeline_events().await.unwrap();
            let terminal_count_before = before
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. })
                    )
                })
                .count();

            let error = actor
                .recover_panicked_turn("post-terminal-panic", "fixture panic")
                .await
                .expect_err("post-terminal panic must terminate without a second projection");
            assert!(
                format!("{error:?}").contains("post-terminal panic"),
                "{error:?}"
            );
            let after = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                after
                    .iter()
                    .filter(|event| {
                        matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Turn(
                                chat_state::TurnEvent::Ended { .. }
                            )
                        )
                    })
                    .count(),
                terminal_count_before
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
