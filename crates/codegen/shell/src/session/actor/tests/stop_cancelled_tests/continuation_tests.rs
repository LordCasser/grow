use super::*;

#[derive(Clone, Copy)]
enum CancellationWriteFailure {
    None,
    Terminal,
    ObserveHook,
}

fn retained_sampling_state(
    actor: &SessionActor,
    turn_id: chat_state::TurnId,
) -> (
    chat_state::session_runtime::TurnContinuation,
    sampler::RecoveryBudget,
) {
    let snapshot = sampler::RecoveryBudgetSnapshot {
        attempts: 2,
        max_attempts: Some(3),
        deadline_ms: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
                + 60_000,
        ),
        replay_safe: false,
    };
    let budget = sampler::RecoveryBudget::restore(snapshot).unwrap();
    let cursor = chat_state::session_runtime::TurnContinuation {
        turn_id,
        prompt_id: "test-active-turn".into(),
        prompt_index: 0,
        origin: serde_json::to_string(&crate::session::PromptOrigin::User).unwrap(),
        next_step_index: 1,
        tool_turns_used: 1,
        tool_calls_used: 0,
        stop_continuations_used: 0,
        output_schema: None,
        resume_point: chat_state::session_runtime::TurnResumePoint::Model,
        task_output_usage: None,
        accepted_response: None,
        structured_output_retries: 0,
        sampling_attempts_used: snapshot.attempts,
        sampling_max_attempts: snapshot.max_attempts,
        sampling_replay_safe: snapshot.replay_safe,
        sampling_deadline_ms: snapshot.deadline_ms,
        operations: Vec::new(),
    };
    *actor.turn_continuation.borrow_mut() = Some(cursor.clone());
    *actor.sampling_continuation.borrow_mut() = Some(budget.clone());
    (cursor, budget)
}

async fn cancellation_retirement_at_terminal_ack(failure: CancellationWriteFailure) {
    let (gateway, _gateway_rx) = mpsc::unbounded_channel();
    let (persistence, _persistence_rx) = mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 256_000, 85, gateway, persistence).await;
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let (persistence, mut records) =
        chat_state::MockTimelinePersistence::new_with_manual_timeline_ack_after(1);
    let (chat_events, _chat_rx) = mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn(
        vec![sampling_types::ConversationItem::system(
            "test system prompt",
        )],
        config,
        Box::new(persistence),
        chat_events,
        tokio_util::sync::CancellationToken::new(),
    );
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    let state = actor.chat_state_handle.clone();
    let (terminal_tx, terminal_rx) = tokio::sync::oneshot::channel();
    let ack_worker = tokio::task::spawn_local(async move {
        let mut terminal_tx = Some(terminal_tx);
        loop {
            let Some(ack) = records.next_timeline_ack().await else {
                continue;
            };
            let pending = state
                .pending_timeline_commit()
                .expect("pending durable write");
            if matches!(
                pending.event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. })
            ) {
                terminal_tx
                    .take()
                    .expect("one cancellation terminal")
                    .send(ack)
                    .unwrap();
            } else {
                let fail_hook = matches!(failure, CancellationWriteFailure::ObserveHook)
                    && matches!(
                        pending.event.kind,
                        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            event: chat_state::HookEventType::StopCancelled,
                            ..
                        })
                    );
                let result = if fail_hook {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected observe-hook write failure",
                    ))
                } else {
                    Ok(())
                };
                let _ = ack.send(result);
            }
            records.drain();
        }
    });
    let actor = std::sync::Arc::new(actor);
    begin_test_active_causal_turn(&actor).await;
    let turn_id = actor.events.current_turn().unwrap();
    let (cursor, old_budget) = retained_sampling_state(&actor, turn_id);
    let old_snapshot = old_budget.snapshot();
    let cancel_actor = actor.clone();
    let cancel = tokio::task::spawn_local(async move {
        cancel_actor
            .cancel_running_task(false, false, false, Some("esc".into()))
            .await
    });
    let terminal_ack = tokio::time::timeout(std::time::Duration::from_secs(5), terminal_rx)
        .await
        .expect("cancellation reaches terminal persistence")
        .unwrap();
    assert!(!cancel.is_finished());
    assert!(
        actor.step_control_gate.try_lock().is_err(),
        "next Turn admission remains fenced before terminal ACK"
    );
    assert_eq!(actor.turn_continuation.borrow().as_ref(), Some(&cursor));
    assert_eq!(
        actor
            .sampling_continuation
            .borrow()
            .as_ref()
            .unwrap()
            .snapshot(),
        old_snapshot
    );
    let terminal_result = if matches!(failure, CancellationWriteFailure::Terminal) {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected cancellation terminal failure",
        ))
    } else {
        Ok(())
    };
    terminal_ack.send(terminal_result).unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), cancel)
        .await
        .expect("cancellation observes terminal ACK")
        .unwrap();
    assert_eq!(
        result.is_ok(),
        matches!(failure, CancellationWriteFailure::None)
    );
    let terminal_failed = matches!(failure, CancellationWriteFailure::Terminal);
    if terminal_failed {
        assert_eq!(actor.turn_continuation.borrow().as_ref(), Some(&cursor));
        assert_eq!(
            actor
                .sampling_continuation
                .borrow()
                .as_ref()
                .unwrap()
                .snapshot(),
            old_snapshot
        );
    } else {
        assert!(actor.turn_continuation.borrow().is_none());
        assert!(actor.sampling_continuation.borrow().is_none());
    }
    assert_eq!(
        old_budget.snapshot(),
        old_snapshot,
        "retirement cannot reset an admitted attempt's shared budget"
    );
    let events = actor.chat_state_handle.timeline_events().await;
    if matches!(failure, CancellationWriteFailure::None) {
        assert_eq!(
            events
                .unwrap()
                .iter()
                .filter(|event| matches!(
                    event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. })
                ))
                .count(),
            1
        );
    } else {
        assert!(
            events.is_none(),
            "failed persistence owner cannot publish a successful snapshot"
        );
    }
    if let Err(error) = result {
        assert!(
            crate::session::commands::is_fatal_turn_boundary_error(&error),
            "{error:?}"
        );
    }
    ack_worker.abort();
    assert!(ack_worker.await.unwrap_err().is_cancelled());
}

#[test]
fn cancelled_turn_retires_continuations_only_after_terminal_ack() {
    run_with_session_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tokio::task::LocalSet::new().run_until(async {
                cancellation_retirement_at_terminal_ack(CancellationWriteFailure::None).await;
                cancellation_retirement_at_terminal_ack(CancellationWriteFailure::Terminal).await;
                cancellation_retirement_at_terminal_ack(CancellationWriteFailure::ObserveHook)
                    .await;
            }));
    });
}

#[test]
fn non_turn_stop_preserves_retained_continuations() {
    run_with_session_stack(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tokio::task::LocalSet::new().run_until(async {
                for foreground in [
                    ForegroundState::Idle,
                    ForegroundState::ApplyingControl,
                    ForegroundState::Compaction,
                    ForegroundState::Settling {
                        prompt_id: "settling-turn".into(),
                        origin: crate::session::PromptOrigin::User,
                        turn_kind: crate::session::TurnKind::User,
                    },
                ] {
                    let (actor, _gateway_rx) = build_actor().await;
                    let (cursor, budget) = retained_sampling_state(&actor, chat_state::TurnId(1));
                    let snapshot = budget.snapshot();
                    actor.state.lock().await.foreground = foreground;
                    actor
                        .cancel_running_task(false, false, false, Some("esc".into()))
                        .await
                        .unwrap();
                    assert_eq!(actor.turn_continuation.borrow().as_ref(), Some(&cursor));
                    assert_eq!(
                        actor
                            .sampling_continuation
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .snapshot(),
                        snapshot
                    );
                    assert_eq!(budget.snapshot(), snapshot);
                }
            }));
    });
}
