use super::cold_goal_pressure_tests::restored_goal;
use super::support::{create_test_actor_ex, seed_test_timeline};
use super::*;

async fn cold_goal_actor() -> (
    std::sync::Arc<SessionActor>,
    sampling_types::GoalDirectiveTag,
    chat_state::persistence::MockPersistenceReceiver,
) {
    let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
    let (persistence, _) = tokio::sync::mpsc::unbounded_channel();
    let (mut actor, _) = create_test_actor_ex(0, 100_000, 85, gateway, persistence).await;
    actor
        .behavior
        .lock()
        .select_behavior(tool_types::BehaviorId::Goal);
    let restored = restored_goal("active").unwrap();
    *actor.goal_tracker.lock() =
        crate::session::goal_tracker::GoalTracker::from_snapshot(restored).unwrap();
    let authority = actor.active_goal_directive_tag().unwrap();

    // Build a persisted Timeline fixture, then replay it through the same
    // validated ChatState restore entry used during session bring-up. This is
    // a reconstructed actor composition, not a full Session spawn test.
    seed_test_timeline(&actor, branch_history(), &["first prompt", "second prompt"]).await;
    let timeline =
        chat_state::Timeline::from_events(actor.chat_state_handle.timeline_events().await.unwrap())
            .unwrap();
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let (mock, persistence) = chat_state::persistence::MockTimelinePersistence::new();
    let (event_tx, _) = tokio::sync::mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_validated_timeline(
        timeline,
        config,
        Some(authority.clone()),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();

    (std::sync::Arc::new(actor), authority, persistence)
}

fn branch_history() -> Vec<ConversationItem> {
    let other_goal = sampling_types::GoalDirectiveTag {
        goal_id: "other-goal".into(),
        definition_revision: 1,
    };
    let restored_goal = sampling_types::GoalDirectiveTag {
        goal_id: "restored-goal".into(),
        definition_revision: 3,
    };
    let directive = |text, tag| {
        ConversationItem::goal_directive(text, sampling_types::SyntheticReason::SystemReminder, tag)
    };
    let mut first = ConversationItem::user("first prompt");
    first.set_prompt_index(0);
    let mut second = ConversationItem::user("second prompt");
    second.set_prompt_index(1);
    vec![
        ConversationItem::system("cold-restored session"),
        directive("historical other-goal instruction", other_goal),
        first,
        ConversationItem::assistant("first response"),
        second,
        ConversationItem::assistant("second response"),
        directive("restored active-goal instruction", restored_goal),
    ]
}

fn expected_portable_tokens(
    items: &[ConversationItem],
    active_goal: Option<&sampling_types::GoalDirectiveTag>,
) -> u64 {
    let projected =
        sampling_types::project_conversation_for_goal_scope(items.to_vec(), active_goal);
    let portable = sampling_types::project_portable_history_with_reasoning(&projected, None);
    chat_state::estimate_conversation_tokens(&portable)
}

async fn assert_request_scope(
    actor: &SessionActor,
    active_goal: Option<&sampling_types::GoalDirectiveTag>,
) {
    assert_eq!(actor.active_goal_directive_tag(), active_goal.cloned());
    let items = actor.chat_state_handle.get_conversation().await;
    let expected = expected_portable_tokens(&items, active_goal);
    assert_eq!(
        actor.chat_state_handle.get_projected_tokens().await,
        expected,
        "scope pressure is current before building the next request"
    );
    let request = actor
        .chat_state_handle
        .build_request(
            "cold-goal-route-rewind",
            vec![],
            None,
            active_goal.cloned(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        chat_state::estimate_request_input_tokens(&request),
        expected,
        "the actual portable request follows current restored Goal authority"
    );
    assert_eq!(
        actor.chat_state_handle.get_projected_tokens().await,
        expected,
        "pressure matches the request built from the current Goal authority"
    );
}

async fn assert_active_goal_request(
    actor: &SessionActor,
    active_goal: &sampling_types::GoalDirectiveTag,
) {
    assert_request_scope(actor, Some(active_goal)).await;
}

async fn select_route(actor: &std::sync::Arc<SessionActor>) {
    let model_id = crate::agent::models::ModelId::new("provider/cold-route");
    let mut config = actor.model_route.snapshot().sampling_config;
    config.model = "cold-restored-route-switch".into();
    let route = SessionActor::selection_route_for_test(model_id.clone(), config.clone(), 85);
    let catalog = SessionActor::published_catalog_for_test(
        model_id.clone(),
        config,
        None,
        std::time::Duration::from_secs(60),
        3,
        85,
    );
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .admit_session_model_selection(route, Some(catalog), None, respond_to)
        .await;
    actor.apply_pending_step_controls_if_idle().await;
    assert!(matches!(
        response.await.unwrap().unwrap(),
        crate::session::DesiredStateOutcome::Applied(id) if id == model_id
    ));
}

async fn assert_rewind_rejected_without_mutation(actor: &SessionActor) {
    let before_surface = actor.chat_state_handle.get_conversation().await;
    let before_events = actor
        .chat_state_handle
        .timeline_events()
        .await
        .unwrap()
        .len();
    let before_pressure = actor.chat_state_handle.get_projected_tokens().await;
    let result = actor
        .handle_rewind(crate::session::RewindRequest {
            target_prompt_index: 1,
            force: true,
            mode: crate::session::RewindMode::ConversationOnly,
        })
        .await
        .unwrap();
    assert!(!result.success, "an extant Goal blocks public rewind");
    assert_eq!(
        result.error.as_deref(),
        Some("Cannot rewind while Goal state exists. Run /goal clear first.")
    );
    assert_eq!(
        serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
        serde_json::to_value(before_surface).unwrap(),
        "rejected rewind preserves the current Surface"
    );
    assert_eq!(
        actor
            .chat_state_handle
            .timeline_events()
            .await
            .unwrap()
            .len(),
        before_events,
        "rejected rewind writes no Timeline fact"
    );
    assert_eq!(
        actor.chat_state_handle.get_projected_tokens().await,
        before_pressure,
        "rejected rewind preserves pressure"
    );
}

async fn rewind_chat_state_internally(actor: &SessionActor) {
    // This exercises ChatState's internal durable branch mechanism. The public
    // SessionActor rewind API rejects every extant Goal state, as covered above.
    actor.chat_state_handle.rewind_durably(1).await.unwrap();
    let retained = actor.chat_state_handle.get_conversation().await;
    assert!(
        retained.iter().any(|item| {
            matches!(item, ConversationItem::User(user)
            if user.goal_directive.as_ref().is_some_and(|tag| tag.goal_id == "other-goal"))
        }),
        "the internal rewind restores the earlier other-Goal tag"
    );
    assert!(!retained.iter().any(|item| {
        matches!(item, ConversationItem::User(user)
            if user.goal_directive.as_ref().is_some_and(|tag| tag.goal_id == "restored-goal"))
    }));
}

async fn run_cold_goal_route_rewind(route_first: bool, internal_rewind: bool) {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let (actor, authority, _persistence) = cold_goal_actor().await;
            actor.chat_state_handle.record_model_call_usage(
                Some("restored-route".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            assert_active_goal_request(&actor, &authority).await;
            let usage_before = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();
            assert_eq!(usage_before.totals.model_calls, 1);
            let history_before = actor.chat_state_handle.get_conversation().await;

            if route_first {
                select_route(&actor).await;
                assert_active_goal_request(&actor, &authority).await;
                if internal_rewind {
                    rewind_chat_state_internally(&actor).await;
                    assert_active_goal_request(&actor, &authority).await;
                } else {
                    assert_rewind_rejected_without_mutation(&actor).await;
                }
            } else {
                if internal_rewind {
                    rewind_chat_state_internally(&actor).await;
                    assert_active_goal_request(&actor, &authority).await;
                } else {
                    assert_rewind_rejected_without_mutation(&actor).await;
                    assert_active_goal_request(&actor, &authority).await;
                }
                select_route(&actor).await;
                assert_active_goal_request(&actor, &authority).await;
            }

            if internal_rewind {
                assert_ne!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(history_before).unwrap(),
                    "the internal durable rewind changes the selected Surface"
                );
            } else {
                assert_eq!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(history_before).unwrap(),
                    "route switch and rejected public rewind preserve history"
                );
            }
            let usage_after = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();
            assert_eq!(usage_after, usage_before, "route and rewind preserve usage");
            assert_eq!(actor.active_goal_directive_tag(), Some(authority));
            let snapshot = actor.goal_tracker.lock().snapshot().cloned().unwrap();
            assert_eq!(snapshot.goal_id, "restored-goal");
            assert_eq!(snapshot.definition_revision, 3);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cold_restored_goal_survives_route_then_rewind() {
    run_cold_goal_route_rewind(true, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cold_restored_goal_survives_rewind_then_route() {
    run_cold_goal_route_rewind(false, false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn chat_state_internal_rewind_keeps_cold_goal_authority_after_route() {
    run_cold_goal_route_rewind(true, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn chat_state_internal_rewind_keeps_cold_goal_authority_before_route() {
    run_cold_goal_route_rewind(false, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn completed_and_paused_cold_goals_also_block_rewind() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in ["complete", "paused"] {
                let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
                let (persistence, _) = tokio::sync::mpsc::unbounded_channel();
                let (actor, _) = create_test_actor_ex(0, 100_000, 85, gateway, persistence).await;
                let restored = restored_goal(case).unwrap();
                *actor.goal_tracker.lock() =
                    crate::session::goal_tracker::GoalTracker::from_snapshot(restored).unwrap();
                assert!(actor.active_goal_directive_tag().is_none(), "{case}");
                seed_test_timeline(&actor, branch_history(), &["first prompt", "second prompt"])
                    .await;
                assert_rewind_rejected_without_mutation(&actor).await;
                assert!(actor.goal_tracker.lock().snapshot().is_some(), "{case}");
            }
        })
        .await;
}

async fn clear_then_public_rewind(route_first: bool) {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let (actor, authority, _persistence) = cold_goal_actor().await;
            actor.chat_state_handle.record_model_call_usage(
                Some("restored-route".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            assert_active_goal_request(&actor, &authority).await;
            let usage_before = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();

            if route_first {
                select_route(&actor).await;
                assert_active_goal_request(&actor, &authority).await;
            }

            actor
                .execute_builtin_slash_command(crate::session::slash_commands::BuiltinAction::GoalClear)
                .await.unwrap();
            assert!(actor.goal_tracker.lock().snapshot().is_none());
            assert!(actor.active_goal_directive_tag().is_none());
            assert_request_scope(&actor, None).await;

            let response = actor
                .handle_rewind(crate::session::RewindRequest {
                    target_prompt_index: 1,
                    force: true,
                    mode: crate::session::RewindMode::ConversationOnly,
                })
                .await
                .unwrap();
            assert!(response.success, "public rewind succeeds after /goal clear: {response:?}");
            let retained = actor.chat_state_handle.get_conversation().await;
            assert!(retained.iter().any(|item| {
                matches!(item, ConversationItem::User(user)
                    if user.goal_directive.as_ref().is_some_and(|tag| tag.goal_id == "other-goal"))
            }), "rewind restores an earlier directive tagged with another Goal");
            assert!(!retained.iter().any(|item| {
                matches!(item, ConversationItem::User(user)
                    if user.goal_directive.as_ref().is_some_and(|tag| tag.goal_id == "restored-goal"))
            }));
            assert!(actor.active_goal_directive_tag().is_none());
            assert_request_scope(&actor, None).await;

            if !route_first {
                select_route(&actor).await;
                assert!(actor.active_goal_directive_tag().is_none());
                assert_request_scope(&actor, None).await;
            }
            assert_eq!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap(),
                usage_before,
                "explicit clear and public rewind preserve lifetime usage"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn goal_clear_then_route_and_public_rewind() {
    clear_then_public_rewind(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn route_then_goal_clear_and_public_rewind() {
    clear_then_public_rewind(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_goal_clear_keeps_authority_and_cold_recovery_scope() {
    tokio::task::LocalSet::new().run_until(async {
        let (actor, authority, persistence) = cold_goal_actor().await;
        assert_active_goal_request(&actor, &authority).await;
        let pressure = actor.chat_state_handle.get_projected_tokens().await;
        let before = actor.chat_state_handle.timeline_events().await.unwrap();
        let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
        drop(persistence);
        actor.execute_builtin_slash_command(crate::session::slash_commands::BuiltinAction::GoalClear).await.unwrap();
        assert_eq!(actor.active_goal_directive_tag(), Some(authority.clone()));
        assert!(actor.chat_state_handle.timeline_events().await.is_none(), "a permanent persistence failure closes the owner rather than permitting more mutations");
        // The failing mock did not accept the clear. Restart from the last
        // acknowledged history and the unchanged control authority.
        let (mock, _receiver) = chat_state::persistence::MockTimelinePersistence::new();
        let (event_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let restored = chat_state::ChatStateActor::spawn_from_validated_timeline(
            chat_state::Timeline::from_events(before).unwrap(), config,
            actor.active_goal_directive_tag(), Box::new(mock), event_tx,
            tokio_util::sync::CancellationToken::new(),
        ).await.unwrap();
        assert_eq!(restored.get_projected_tokens().await, pressure);
        let request = restored.build_request("failed-clear-restored", vec![], None, Some(authority), None).await.unwrap();
        assert_eq!(chat_state::estimate_request_input_tokens(&request), pressure);
    }).await;
}
