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

async fn failed_first_suspend_then_restored_wait(kind: PendingKind, worker_resolution: bool) {
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
    // Tree owner closes runtime admission before the node writes SuspendRequested.
    actor
        .state
        .lock()
        .await
        .runtime_suspend
        .store(true, std::sync::atomic::Ordering::Release);
    guard
        .resolve_durably(&actor.chat_state_handle, InteractionResolution::Suspended)
        .await
        .unwrap();
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
    let surface = actor.chat_state_handle.get_conversation().await;
    assert!(surface.iter().any(|item| matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == CALL_ID) && item.text_content().contains("OutcomeUnknown")), "probe expects pre-intent wait to become unknown");
    request_suspension(
        &actor,
        "suspension-retry-op",
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .unwrap();
    let runtime = actor.chat_state_handle.runtime_state().await.unwrap();
    assert_eq!(runtime.phase, RuntimePhase::Suspended);
    let cursor = runtime.checkpoint.unwrap().turn.unwrap();
    assert!(!cursor.operations.iter().any(|op| op.call_id == CALL_ID
        && matches!(
            op.state,
            SuspendedOperationState::AwaitUser | SuspendedOperationState::AwaitPermission
        )));
    eprintln!(
        "AUDIT pre-intent Suspended: unknown result committed; retry checkpoint has no human wait"
    );
}
#[test]
fn audit_post_240_pre_intent_resolution_loses_wait() {
    run_local_test(|| {
        Box::pin(async {
            failed_first_suspend_then_restored_wait(PendingKind::Permission, false).await;
        })
    });
}
