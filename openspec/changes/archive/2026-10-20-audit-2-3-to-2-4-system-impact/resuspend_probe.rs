//! Cross-stage regression coverage for suspended human waits.
//!
//! The first suspension attempt can outlive its caller while draining an
//! auxiliary owner. Its foreground worker then drops the in-memory interaction
//! guard, so a retry must recover the still-open interaction from Timeline.
use super::*;
use crate::session::actor::tests::support::{begin_test_causal_turn, build_actor};
use crate::session::pending_interaction::{PendingInteractionGuard, PendingKind};
use chat_state::session_runtime::{SuspendedOperationState, TurnContinuation, TurnResumePoint};
use chat_state::{InteractionEvent, InteractionKind, InteractionResolution, TimelineEventKind};

fn run_local_test(
    test: impl FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> + Send + 'static,
) {
    std::thread::Builder::new()
        .stack_size(48 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(tokio::task::LocalSet::new().run_until(test()));
        })
        .unwrap()
        .join()
        .unwrap();
}

fn expected_operation_state(kind: PendingKind) -> SuspendedOperationState {
    match kind {
        PendingKind::Permission => SuspendedOperationState::AwaitPermission,
        PendingKind::Question | PendingKind::PlanApproval => SuspendedOperationState::AwaitUser,
    }
}

fn expected_interaction_kind(kind: PendingKind) -> InteractionKind {
    match kind {
        PendingKind::Permission => InteractionKind::Permission,
        PendingKind::Question => InteractionKind::Question,
        PendingKind::PlanApproval => InteractionKind::PlanApproval,
    }
}

async fn request_suspension(
    actor: &std::sync::Arc<SessionActor>,
    operation_id: &str,
    deadline: tokio::time::Instant,
) -> Result<RuntimeNodeRef, String> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .request_node_suspension(
            operation_id.to_owned(),
            actor.session_id_string(),
            deadline,
            respond_to,
        )
        .await;
    response.await.expect("suspension worker replies")
}

async fn failed_first_suspend_then_restored_wait(
    kind: PendingKind,
    worker_resolution: bool,
    before_intent: bool,
) {
    let (actor, _gateway) = build_actor().await;
    begin_test_causal_turn(&actor).await;

    const CALL_ID: &str = "suspended-human-wait";
    actor
        .chat_state_handle
        .push_response_durably(
            vec![ConversationItem::assistant_tool_calls(vec![
                sampling_types::ToolCall {
                    id: CALL_ID.into(),
                    name: match kind {
                        PendingKind::Permission => "run",
                        PendingKind::Question => "ask_user_question",
                        PendingKind::PlanApproval => "plan_control",
                    }
                    .into(),
                    arguments: match kind {
                        PendingKind::PlanApproval => r#"{"action":"submit"}"#,
                        _ => "{}",
                    }
                    .into(),
                },
            ])],
            None,
        )
        .await
        .unwrap();
    actor
        .events
        .tool_started(
            match kind {
                PendingKind::Permission => "run",
                PendingKind::Question => "ask_user_question",
                PendingKind::PlanApproval => "plan_control",
            }
            .into(),
            CALL_ID.into(),
            Some(match kind {
                PendingKind::PlanApproval => serde_json::json!({"action": "submit"}),
                _ => serde_json::json!({}),
            }),
        )
        .await
        .unwrap();
    let turn_id = actor.events.current_turn().expect("active fixture Turn");
    *actor.turn_continuation.borrow_mut() = Some(TurnContinuation {
        turn_id,
        prompt_id: "fixture-prompt".into(),
        prompt_index: 0,
        origin: serde_json::to_string(&crate::session::PromptOrigin::User).unwrap(),
        next_step_index: 1,
        tool_turns_used: 1,
        tool_calls_used: 1,
        stop_continuations_used: 0,
        output_schema: None,
        resume_point: TurnResumePoint::Model,
        task_output_usage: None,
        accepted_response: None,
        structured_output_retries: 0,
        sampling_attempts_used: 0,
        sampling_max_attempts: None,
        sampling_replay_safe: true,
        sampling_deadline_ms: None,
        operations: vec![],
    });
    let mut guard = PendingInteractionGuard::new(
        actor.pending_interactions.clone(),
        actor.notifications.gateway.clone(),
        actor.session_info.id.clone(),
        CALL_ID.into(),
        kind,
    );
    guard.open_durably(&actor.chat_state_handle).await.unwrap();
    if before_intent {
        // Tree admission closes the child gate before its node intent exists.
        actor
            .state
            .lock()
            .await
            .runtime_suspend
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(
            guard
                .resolve_durably(&actor.chat_state_handle, InteractionResolution::Suspended)
                .await
                .unwrap()
        );
    }
    for event in [
        SessionRuntimeEvent::SuspendRequested {
            operation_id: "suspension-retry-op".into(),
            root_session_id: actor.session_id_string(),
        },
        SessionRuntimeEvent::CommandsSettled {
            operation_id: "suspension-retry-op".into(),
            commands: vec![],
        },
    ] {
        actor
            .chat_state_handle
            .record_timeline_event_durably(TimelineEventKind::Runtime(event))
            .await
            .unwrap();
    }
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
    let mut resolver_guard = None;
    let owner = if worker_resolution {
        // Keep the guard in an independent interaction owner so it can record
        // its durable suspension resolution while the turn owner drains.
        resolver_guard = Some(guard);
        tokio::task::spawn_local(async move {
            let _stopped = stopped_tx;
            std::future::pending::<()>().await;
        })
    } else {
        // The normal parked-turn cancellation path drops its guard when the
        // suspension owner aborts the foreground worker.
        tokio::task::spawn_local(async move {
            let _guard = guard;
            let _stopped = stopped_tx;
            std::future::pending::<()>().await;
        })
    };
    actor.state.lock().await.foreground = ForegroundState::RegularTurn(AgentTask {
        stopped: futures_util::FutureExt::shared(stopped_rx),
        prompt_id: "fixture-prompt".into(),
        origin: crate::session::PromptOrigin::User,
        turn_kind: crate::session::TurnKind::User,
        turn_start_ms: 0,
        usage_epoch: 0,
        handle: owner.abort_handle(),
        steering_open: true,
    });

    // Keep a pre-existing auxiliary owner alive beyond the first deadline.
    let lease = actor
        .session_activities
        .try_start("suspension-retry-test-owner")
        .expect("test owner admitted before suspension closes admission");
    let first_deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(250);
    let first_actor = actor.clone();
    let first = tokio::task::spawn_local(async move {
        request_suspension(&first_actor, "suspension-retry-op", first_deadline).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !actor.background_service_shutdown.is_cancelled() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first attempt reaches owner drain");
    if worker_resolution {
        assert!(
            resolver_guard
                .as_ref()
                .expect("independent interaction guard remains owned")
                .resolve_durably(&actor.chat_state_handle, InteractionResolution::Suspended,)
                .await
                .expect("interaction worker durably resolves during owner drain")
        );
        drop(resolver_guard.take());
    }
    tokio::time::sleep_until(first_deadline + std::time::Duration::from_millis(10)).await;
    drop(lease);
    let first_error = first
        .await
        .unwrap()
        .expect_err("first attempt must report its elapsed deadline");
    assert!(
        first_error.contains("owners settled after the deadline"),
        "{first_error}"
    );
    assert!(actor.pending_interactions.lock().unwrap().is_empty());
    assert!(actor
        .chat_state_handle
        .get_conversation()
        .await
        .iter()
        .all(|item| !matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == CALL_ID)));

    let node = request_suspension(
        &actor,
        "suspension-retry-op",
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .expect("retry of the same operation completes suspension");
    let runtime = actor.chat_state_handle.runtime_state().await.unwrap();
    assert_eq!(runtime.phase, RuntimePhase::Suspended);
    assert_eq!(runtime.checkpoint_seq, Some(node.checkpoint_seq));
    let checkpoint = runtime.checkpoint.expect("committed retry checkpoint");
    let cursor = checkpoint.turn.expect("original logical Turn remains open");
    let operation = cursor
        .operations
        .iter()
        .find(|operation| operation.call_id == CALL_ID)
        .expect("Timeline-open interaction is reconstructed on retry");
    assert_eq!(operation.state, expected_operation_state(kind));
    assert_eq!(operation.dependency_id.as_deref(), Some(CALL_ID));
    assert!(actor
        .chat_state_handle
        .get_conversation()
        .await
        .iter()
        .all(|item| !matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == CALL_ID)));

    let events = actor.chat_state_handle.timeline_events().await.unwrap();
    let opened_attempt = events.iter().find_map(|event| match &event.kind {
        TimelineEventKind::Interaction(InteractionEvent::Opened {
            tool_call_id,
            attempt_id,
            kind: interaction_kind,
        }) if tool_call_id == CALL_ID && *interaction_kind == expected_interaction_kind(kind) => {
            Some(attempt_id.clone())
        }
        _ => None,
    });
    let opened_attempt = opened_attempt.expect("the original interaction-open fact remains");
    let has_suspended_resolution = events.iter().any(|event| {
        matches!(&event.kind,
            TimelineEventKind::Interaction(InteractionEvent::Resolved {
                tool_call_id,
                attempt_id,
                kind: interaction_kind,
                resolution: InteractionResolution::Suspended,
            }) if tool_call_id == CALL_ID
                && attempt_id == &opened_attempt
                && *interaction_kind == expected_interaction_kind(kind)
        )
    });
    assert!(has_suspended_resolution);

    // Rebuild the canonical state and tracker, rather than restoring over the
    // old physical actor's still-active event tracker.
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let control = crate::session::control::SessionControlSnapshot::latest_from_timeline(&events)
        .unwrap()
        .unwrap();
    let restored_state = chat_state::ChatStateActor::spawn_from_timeline(
        events,
        actor.chat_state_handle.get_sampling_config().await.unwrap(),
        Box::new(chat_state::NullTimelinePersistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let (gateway_tx, _restored_gateway) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move { while persistence_rx.recv().await.is_some() {} });
    let mut restored_actor = crate::session::actor::tests::support::create_test_actor(
        0,
        256_000,
        85,
        gateway_tx,
        persistence_tx,
    )
    .await;
    restored_actor.control_revision.store(
        control.control_revision,
        std::sync::atomic::Ordering::Release,
    );
    *restored_actor.behavior.lock() =
        crate::session::behavior::BehaviorCoordinator::from_snapshot(control.behavior);
    restored_actor.session_info = actor.session_info.clone();
    restored_actor.session_directory = actor.session_directory.clone();
    restored_actor.events = crate::session::events::EventTracker::new(restored_state.clone());
    restored_actor.chat_state_handle = restored_state;
    restored_actor
        .state
        .lock()
        .await
        .runtime_suspend
        .store(true, std::sync::atomic::Ordering::Release);
    let actor = std::sync::Arc::new(restored_actor);
    let (completion_tx, mut completion_rx) = tokio::sync::mpsc::unbounded_channel::<(
        String,
        crate::session::commands::PromptTurnResult,
    )>();
    actor
        .prepare_runtime_restore(
            "suspension-retry-op",
            "suspension-retry-instance",
            completion_tx,
        )
        .await
        .expect("restoration rebuilds the suspended Turn");
    actor
        .commit_runtime_resume(
            "suspension-retry-op".into(),
            "suspension-retry-instance".into(),
        )
        .await
        .expect("restoration commits the same suspension operation");
    let restoring = actor.chat_state_handle.runtime_state().await.unwrap();
    assert_eq!(restoring.phase, RuntimePhase::Restoring);
    assert!(actor.restoration_release.borrow().is_some());
    // Hold only the execution start signal to model the valid scheduling gap
    // after durable ExecutionReleased but before the resumed worker is polled.
    let delayed_release = actor.restoration_release.borrow_mut().take().unwrap();
    actor
        .release_runtime_restore("suspension-retry-instance")
        .await
        .unwrap();
    assert_eq!(
        actor.chat_state_handle.runtime_state().await.unwrap().phase,
        RuntimePhase::Running
    );
    assert!(actor.pending_interactions.lock().unwrap().is_empty());
    assert_eq!(
        actor
            .turn_continuation
            .borrow()
            .as_ref()
            .unwrap()
            .operations[0]
            .state,
        expected_operation_state(kind)
    );
    for event in [
        SessionRuntimeEvent::SuspendRequested {
            operation_id: "second-cycle".into(),
            root_session_id: actor.session_id_string(),
        },
        SessionRuntimeEvent::CommandsSettled {
            operation_id: "second-cycle".into(),
            commands: vec![],
        },
    ] {
        actor
            .chat_state_handle
            .record_timeline_event_durably(TimelineEventKind::Runtime(event))
            .await
            .unwrap();
    }
    request_suspension(
        &actor,
        "second-cycle",
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .unwrap();
    drop(delayed_release);
    let checkpoint = actor
        .chat_state_handle
        .runtime_state()
        .await
        .unwrap()
        .checkpoint
        .unwrap();
    let operation = checkpoint
        .turn
        .as_ref()
        .unwrap()
        .operations
        .iter()
        .find(|op| op.call_id == CALL_ID)
        .unwrap();
    eprintln!(
        "AUDIT immediate re-suspend: {:?} -> {:?}",
        expected_operation_state(kind),
        operation.state
    );
    assert_eq!(
        operation.state,
        SuspendedOperationState::OutcomeUnknown,
        "audit probe asserts the suspected defect, not desired behavior"
    );
    assert!(actor.chat_state_handle.get_conversation().await.iter().any(|item|
        matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == CALL_ID)));
}

#[test]
fn audit_230_240_released_wait_resuspended_before_rebind() {
    run_local_test(|| {
        Box::pin(async {
            for kind in [
                PendingKind::Permission,
                PendingKind::Question,
                PendingKind::PlanApproval,
            ] {
                failed_first_suspend_then_restored_wait(kind, false, true).await;
            }
        })
    });
}
