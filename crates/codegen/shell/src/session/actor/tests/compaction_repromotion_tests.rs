//! Publication ownership after a promoted waiter is dropped.
use super::*;
use test_support::{InferenceEndpoint, InferenceRequestMatcher};

fn repromotion_after_waiter_drop(cancel_generation: bool, max_output_tokens: Option<u32>) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!(
                "REPROMOTION_EVIDENCE: {}",
                "old decisions and verified execution. ".repeat(40)
            );
            let mut provider = server.expect_response_blocked(
                "original background worker",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn(&[(&summary, END_TURN)], END_TURN),
            );
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor.compaction.threshold_percent.set(80);
            actor.compaction.pre_prune.set(false);
            seed_closed_compaction_range(&actor, 88_000).await;
            actor
                .chat_state_handle
                .record_provider_context_anchor(75_000);
            actor.state.lock().await.foreground =
                ForegroundState::RegularTurn(running_task_stub("repromotion"));
            assert!(!actor.background_compaction_boundary().await.unwrap());
            tokio::time::timeout(std::time::Duration::from_secs(5), provider.wait_blocked())
                .await
                .unwrap();
            let spawn_waiter = || {
                let actor = actor.clone();
                tokio::task::spawn_local(async move {
                    actor
                        .run_compact_only(compaction::AutoCompactTriggerInfo {
                            tokens_used: 85_000,
                            context_window: 100_000,
                            percentage: 85,
                            source: "repromotion_test",
                            max_output_tokens,
                        })
                        .await
                })
            };
            let first = spawn_waiter();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                wait_for_session_update(&mut notifications, "auto_compact_started"),
            )
            .await
            .unwrap();
            first.abort();
            assert!(first.await.unwrap_err().is_cancelled());
            assert!(
                actor.compaction.background.borrow().is_some(),
                "the same worker must be returned to its owner slot"
            );
            if cancel_generation {
                actor
                    .cancel_background_compaction("user_stop_after_waiter_drop")
                    .await
                    .unwrap();
            }
            let second = spawn_waiter();
            provider.release();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), second)
                .await
                .unwrap()
                .unwrap();
            // Always drain before asserting so a failed probe leaves no worker.
            actor
                .cancel_background_compaction_before("repromotion_test_cleanup", None)
                .await
                .unwrap();
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let promoted = events
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Compaction(
                            chat_state::CompactionEvent::Promoted { .. }
                        )
                    )
                })
                .count();
            let completed = events
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Compaction(
                            chat_state::CompactionEvent::Completed { .. }
                        )
                    )
                })
                .count();
            assert_eq!(
                server.messages_request_count(),
                1,
                "re-promotion must reuse the original provider attempt"
            );
            assert_eq!(promoted, 1, "promotion is a fact, not a count of waiters");
            if cancel_generation {
                let error = result.expect_err("a cancelled generation cannot publish");
                assert!(
                    !crate::session::commands::is_fatal_turn_boundary_error(&error),
                    "cancellation must not become an invalid duplicate promotion: {error:?}"
                );
                assert!(format!("{error:?}").contains("compact cancelled"));
                assert_eq!(completed, 0);
            } else {
                if max_output_tokens.is_some() {
                    assert!(
                        result.is_err(),
                        "completed promotion still cannot fit reserved output"
                    );
                    assert!(
                        format!("{:?}", result.unwrap_err()).contains("context_window_exceeded")
                    );
                } else {
                    assert!(
                        result.is_ok(),
                        "a new waiter must resume the existing promotion: {result:?}"
                    );
                }
                assert_eq!(completed, 1);
            }
            let installed = actor
                .chat_state_handle
                .get_conversation()
                .await
                .iter()
                .any(|item| item.text_content().contains("REPROMOTION_EVIDENCE"));
            assert_eq!(installed, !cancel_generation);
            let (updates, _) = drain_session_updates(&mut notifications);
            assert_eq!(
                updates
                    .iter()
                    .filter(|kind| kind.as_str() == "auto_compact_completed")
                    .count(),
                usize::from(!cancel_generation),
                "one UI Completed for a committed promotion: {updates:?}"
            );
            assert!(actor.compaction.pending_async_notice.get().is_none());
            assert!(actor.compaction.background.borrow().is_none());
        }));
    });
}

#[test]
fn dropped_promotion_waiter_can_await_the_same_generation_again() {
    repromotion_after_waiter_drop(false, None);
}

#[test]
fn cancelled_generation_cannot_publish_when_promoted_again() {
    repromotion_after_waiter_drop(true, None);
}

#[test]
fn promoted_replacement_keeps_completed_but_rejects_reserved_output_overflow() {
    repromotion_after_waiter_drop(false, Some(40_000));
}
