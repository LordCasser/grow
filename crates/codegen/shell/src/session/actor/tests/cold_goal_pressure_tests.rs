use super::support::*;
use super::*;

pub(super) fn restored_goal(case: &str) -> Option<crate::session::goal_tracker::GoalState> {
    use crate::session::goal_tracker::{GoalPauseReason, GoalTracker};
    if case == "none" {
        return None;
    }
    let mut tracker = GoalTracker::default();
    tracker
        .create_goal(
            "restored-goal".into(),
            "finish the restored objective".into(),
            None,
            "2026-10-29T00:00:00Z".into(),
        )
        .unwrap();
    tracker.snapshot_mut().unwrap().definition_revision = 3;
    if case == "complete" {
        assert!(tracker.complete("verified completed work"));
    } else if case == "paused" {
        assert!(tracker.pause(GoalPauseReason::User));
    }
    // Use the same validated snapshot restoration as Session bring-up.
    let encoded = serde_json::to_value(tracker.snapshot().unwrap()).unwrap();
    let restored = GoalTracker::from_snapshot(serde_json::from_value(encoded).unwrap()).unwrap();
    restored.snapshot().cloned()
}

fn history() -> Vec<ConversationItem> {
    let directive = |revision, text| {
        ConversationItem::goal_directive(
            text,
            sampling_types::SyntheticReason::SystemReminder,
            sampling_types::GoalDirectiveTag {
                goal_id: "restored-goal".into(),
                definition_revision: revision,
            },
        )
    };
    vec![
        ConversationItem::system("cold Goal system"),
        directive(2, "old directive ".repeat(8_000)),
        ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
            "r".repeat(80_000),
        )),
        ConversationItem::assistant("previous result"),
        directive(3, "active current directive ".repeat(8_000)),
    ]
}

fn expected(items: &[ConversationItem], scope: Option<&sampling_types::GoalDirectiveTag>) -> u64 {
    let projected = sampling_types::project_conversation_for_goal_scope(items.to_vec(), scope);
    let portable = sampling_types::project_portable_history_with_reasoning(&projected, None);
    chat_state::estimate_conversation_tokens(&portable)
}

#[tokio::test(flavor = "current_thread")]
async fn cold_goal_pressure_is_initialized_before_first_advertisement() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in ["active", "complete", "paused", "none", "normal"] {
                let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
                let (persistence, _) = tokio::sync::mpsc::unbounded_channel();
                let (mut actor, mut notifications) =
                    create_test_actor_ex(0, 100_000, 85, gateway, persistence).await;
                let behavior = if case == "normal" {
                    tool_types::BehaviorId::Normal
                } else {
                    tool_types::BehaviorId::Goal
                };
                actor.behavior.lock().select_behavior(behavior);
                let goal = restored_goal(case);
                if let Some(goal) = &goal {
                    *actor.goal_tracker.lock() =
                        crate::session::goal_tracker::GoalTracker::from_snapshot(goal.clone())
                            .unwrap();
                }
                let scope = goal_support::goal_directive_scope(behavior, goal.as_ref());
                assert_eq!(scope.is_some(), case == "active");
                let items = history();
                let timeline = chat_state::Timeline::from_seed(items.clone()).unwrap();
                let event_count = timeline.events().len();
                let (mock, _) = chat_state::persistence::MockTimelinePersistence::new();
                let (event_tx, _events) = tokio::sync::mpsc::unbounded_channel();
                actor.chat_state_handle =
                    chat_state::ChatStateActor::spawn_from_validated_timeline(
                        timeline,
                        actor.chat_state_handle.get_sampling_config().await.unwrap(),
                        scope.clone(),
                        Box::new(mock),
                        event_tx,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                    .unwrap();

                // These reads and the first advertised UI context happen before
                // any request build. Historical tags cannot revive a stopped Goal.
                let expected = expected(&items, scope.as_ref());
                assert_eq!(
                    actor.chat_state_handle.get_projected_tokens().await,
                    expected,
                    "{case}"
                );
                assert_eq!(
                    actor.build_session_info().await.context.used,
                    expected,
                    "{case}"
                );
                actor.send_available_commands_update().await;
                let SessionEvent::Notification(
                    crate::session::replay_events::SessionNotification::Acp(notification),
                ) = notifications.try_recv().unwrap()
                else {
                    panic!("expected first command advertisement");
                };
                assert_eq!(
                    notification.meta.unwrap()["totalTokens"],
                    serde_json::json!(expected),
                    "{case}"
                );
                assert_eq!(
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    event_count,
                    "scope initialization is not a durable fact"
                );
                assert_eq!(
                    actor
                        .chat_state_handle
                        .try_get_session_usage()
                        .await
                        .unwrap()
                        .totals
                        .model_calls,
                    0
                );
                assert_eq!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(&items).unwrap()
                );

                let request = actor
                    .chat_state_handle
                    .build_request("cold-goal", vec![], None, scope, None)
                    .await
                    .unwrap();
                assert_eq!(
                    chat_state::estimate_request_input_tokens(&request),
                    expected,
                    "{case}"
                );
                assert_eq!(
                    actor.chat_state_handle.get_projected_tokens().await,
                    expected,
                    "first build preserves the initialized baseline: {case}"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cold_goal_scope_reconciles_runtime_pause_before_advertisement() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence, _) = tokio::sync::mpsc::unbounded_channel();
            let (mut actor, mut notifications) =
                create_test_actor_ex(0, 100_000, 85, gateway, persistence).await;
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            let goal = restored_goal("active").unwrap();
            *actor.goal_tracker.lock() =
                crate::session::goal_tracker::GoalTracker::from_snapshot(goal).unwrap();
            let initial_scope = actor.active_goal_directive_tag().unwrap();
            let items = history();
            let timeline = chat_state::Timeline::from_seed(items.clone()).unwrap();
            let (mock, _) = chat_state::persistence::MockTimelinePersistence::new();
            let (event_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
            actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_validated_timeline(
                timeline,
                actor.chat_state_handle.get_sampling_config().await.unwrap(),
                Some(initial_scope.clone()),
                Box::new(mock),
                event_tx,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(
                actor.chat_state_handle.get_projected_tokens().await,
                expected(&items, Some(&initial_scope))
            );
            assert!(
                actor
                    .goal_tracker
                    .lock()
                    .pause(crate::session::goal_tracker::GoalPauseReason::RuntimeUnavailable)
            );
            actor
                .chat_state_handle
                .set_request_goal_scope(actor.active_goal_directive_tag())
                .await
                .unwrap();
            let paused = expected(&items, None);
            assert_eq!(actor.chat_state_handle.get_projected_tokens().await, paused);
            let Some(chat_state::ChatStateEvent::ContextPressureUpdated { projected_tokens }) =
                events.recv().await
            else {
                panic!("scope reconciliation publishes the changed projection");
            };
            assert_eq!(projected_tokens, paused);
            actor.send_available_commands_update().await;
            let SessionEvent::Notification(
                crate::session::replay_events::SessionNotification::Acp(notification),
            ) = notifications.try_recv().unwrap()
            else {
                panic!("expected reconciled command advertisement");
            };
            assert_eq!(
                notification.meta.unwrap()["totalTokens"],
                serde_json::json!(paused)
            );
            assert_eq!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .totals
                    .model_calls,
                0
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn durable_goal_controls_refresh_scope_before_request_build() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence, _) = tokio::sync::mpsc::unbounded_channel();
            let (actor, _) = create_test_actor_ex(0, 100_000, 85, gateway, persistence).await;
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            *actor.goal_tracker.lock() = crate::session::goal_tracker::GoalTracker::from_snapshot(
                restored_goal("active").unwrap(),
            )
            .unwrap();
            super::support::replace_test_surface(&actor.chat_state_handle, history()).await;
            actor
                .chat_state_handle
                .set_request_goal_scope(actor.active_goal_directive_tag())
                .await
                .unwrap();
            let usage = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();
            for case in ["paused", "active", "complete", "active", "normal"] {
                let mut next =
                    restored_goal(if case == "normal" { "active" } else { case }).unwrap();
                if case == "active" {
                    next.definition_revision += 1;
                }
                let behavior =
                    crate::session::behavior::BehaviorSnapshot::selected(if case == "active" {
                        tool_types::BehaviorId::Goal
                    } else {
                        tool_types::BehaviorId::Normal
                    });
                let next = (case != "normal").then_some(next);
                let scope = goal_support::goal_directive_scope(behavior.behavior(), next.as_ref());
                actor
                    .persist_control_snapshot_durably(behavior, next)
                    .await
                    .unwrap();
                let items = actor.chat_state_handle.get_conversation().await;
                assert_eq!(
                    actor.chat_state_handle.get_projected_tokens().await,
                    expected(&items, scope.as_ref()),
                    "{case} scope must be current before build"
                );
                let request = actor
                    .chat_state_handle
                    .build_request("goal-control-scope", vec![], None, scope, None)
                    .await
                    .unwrap();
                assert_eq!(
                    actor.chat_state_handle.get_projected_tokens().await,
                    chat_state::estimate_request_input_tokens(&request)
                );
                assert_eq!(
                    actor
                        .chat_state_handle
                        .try_get_session_usage()
                        .await
                        .unwrap(),
                    usage
                );
            }
        })
        .await;
}
