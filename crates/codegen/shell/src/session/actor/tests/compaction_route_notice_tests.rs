//! Deferred async completion notices across model route changes.

use super::support::*;
use super::*;
use serde_json::json;
use test_support::{InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer};

#[test]
fn committed_async_notice_survives_same_window_model_selection_until_projection() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!(
                "background route notice summary: {}",
                "verified evidence from the closed range. ".repeat(40)
            );
            let mut blocked_summary = server.expect_response_blocked(
                "background summary",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn(&[(&summary, END_TURN)], END_TURN),
            );
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000)
                    .await;
            actor.compaction.threshold_percent.set(80);
            actor.compaction.pre_prune.set(false);
            seed_closed_compaction_range(&actor, 88_000).await;
            actor.chat_state_handle.record_provider_context_anchor(75_000);
            actor.state.lock().await.foreground =
                ForegroundState::RegularTurn(running_task_stub("route-notice-async"));

            actor.background_compaction_boundary().await.unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                blocked_summary.wait_blocked(),
            )
            .await
            .expect("background summary reaches provider");
            blocked_summary.release();
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                actor.session_activities.wait_idle(),
            )
            .await
            .expect("background summary settles before publication");
            assert!(actor.background_compaction_boundary().await.unwrap());
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|event| matches!(
                event.kind,
                chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Completed { .. }
                )
            )));
            assert!(actor.compaction.pending_async_notice.get().is_some());
            let (_, before_route) = drain_session_updates(&mut notifications);
            assert!(before_route.is_empty(), "surface commit is still deferred");
            actor.state.lock().await.foreground = ForegroundState::Idle;

            // Use the normal model-selection mailbox path so the ChatState,
            // model route, and sampler all move together at the same window.
            let mut selected = actor.reconstruct_full_config().await;
            selected.model = "same-window-next-model".into();
            selected.base_url = server.url();
            selected.api_backend = sampling_types::ApiBackend::Messages;
            selected.api_key = Some("test-key".into());
            selected.context_window = 100_000;
            let model_id = crate::agent::models::ModelId::new("test/same-window-next-model");
            let catalog = SessionActor::published_catalog_for_test(
                model_id.clone(),
                selected,
                None,
                std::time::Duration::from_secs(60),
                0,
                85,
            );
            let route = catalog.resolve_session_route(&model_id, None).unwrap();
            let (ack, applied) = tokio::sync::oneshot::channel();
            actor
                .admit_session_model_selection(route, Some(catalog), None, ack)
                .await;
            assert!(matches!(
                applied.await.unwrap().unwrap(),
                crate::session::DesiredStateOutcome::Applied(_)
            ));
            assert_eq!(actor.model_route.snapshot().model_id, model_id);
            assert_eq!(
                actor
                    .chat_state_handle
                    .get_sampling_config()
                    .await
                    .unwrap()
                    .context_window
                    .get(),
                100_000
            );
            assert!(actor.compaction.pending_async_notice.get().is_some());
            let (_, after_route_completions) = drain_session_updates(&mut notifications);
            assert!(
                after_route_completions.is_empty(),
                "model selection itself cannot publish an async completion"
            );

            // The minimal published catalog exposes fewer than two tools, so
            // the mock's coarse classifier labels this ordinary turn auxiliary.
            // The real turn loop and request model/content below prove its owner.
            let mut next = server.expect_response_blocked(
                "request after same-window model selection",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn(&[("response on the selected model", END_TURN)], END_TURN),
            );
            let mut turn = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { run_user_turn(&actor, "route-notice-projection").await }
            });
            tokio::select! {
                result = &mut turn => panic!("turn ended before ordinary request reached provider: {result:?}"),
                result = tokio::time::timeout(std::time::Duration::from_secs(5), next.wait_blocked()) => {
                    result.expect("ordinary request reaches provider");
                }
            }

            let requests = server.request_bodies();
            assert_eq!(requests.len(), 2, "only summary and following ordinary request");
            assert_eq!(requests[1]["model"], "same-window-next-model");
            assert!(requests[1].to_string().contains("write a long answer"));
            let projection = actor.chat_state_handle.get_projected_tokens().await;
            let (updates, completed) = drain_session_updates(&mut notifications);
            assert_eq!(completed.len(), 1, "the first request projection publishes once");
            assert_eq!(completed[0]["update"]["tokens_after"], json!(projection));
            assert!(completed[0]["update"]["async_compact"] == json!(true));
            assert!(actor.compaction.pending_async_notice.get().is_none());

            actor.publish_pending_async_compaction_notice().await;
            let (_, duplicate) = drain_session_updates(&mut notifications);
            assert!(duplicate.is_empty(), "a consumed notice cannot be published twice");

            next.release();
            tokio::time::timeout(std::time::Duration::from_secs(10), turn)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), next.wait_satisfied())
                .await
                .unwrap();
            assert!(updates.iter().any(|kind| kind == "auto_compact_completed"));
            assert_eq!(server.messages_request_count(), 2);
        }));
    });
}

#[test]
fn smaller_window_model_switch_replacement_supersedes_pending_async_notice() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let async_summary = format!(
                "background old summary: {}",
                "verified old evidence. ".repeat(40)
            );
            let mut blocked_summary = server.expect_response_blocked(
                "background summary before shrink",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn(&[(&async_summary, END_TURN)], END_TURN),
            );
            let synchronous_summary = format!(
                "replacement after route shrink: {}",
                "verified evidence from the newer replacement range. ".repeat(40)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[(&synchronous_summary, END_TURN)], END_TURN),
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
                ForegroundState::RegularTurn(running_task_stub("route-shrink-async"));

            actor.background_compaction_boundary().await.unwrap();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                blocked_summary.wait_blocked(),
            )
            .await
            .expect("background summary reaches provider");
            blocked_summary.release();
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                actor.session_activities.wait_idle(),
            )
            .await
            .expect("background summary settles before publication");
            assert!(actor.background_compaction_boundary().await.unwrap());
            assert!(actor.compaction.pending_async_notice.get().is_some());
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let initial_completed = events
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
            assert_eq!(initial_completed, 1, "the async replacement committed");
            let (_, before_switch) = drain_session_updates(&mut notifications);
            assert!(
                before_switch.is_empty(),
                "the async notice remains deferred"
            );

            // Close one more turn so the committed async range leaves a legal
            // older range for the model-switch replacement to summarize.
            actor
                .chat_state_handle
                .push_user_message(prompt("newer closed turn", 2));
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("newer response"));
            actor
                .chat_state_handle
                .push_user_message(prompt("current request", 3));
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("z".repeat(240_000)));
            let _ = actor.chat_state_handle.get_conversation_len().await;
            let projected_before_switch = actor.chat_state_handle.get_projected_tokens().await;
            assert!(
                projected_before_switch > 80_000,
                "fixture pressure must exceed the selected model window: {projected_before_switch}"
            );
            let smaller_window = 80_000;
            actor.compaction.previous_model.set(Some(
                crate::session::compaction_config::PreviousModelInfo {
                    model_slug: "large-model".into(),
                    context_window: 100_000,
                },
            ));
            actor.state.lock().await.foreground = ForegroundState::Idle;

            let mut selected = actor.reconstruct_full_config().await;
            selected.model = "small-window-model".into();
            selected.base_url = server.url();
            selected.api_backend = sampling_types::ApiBackend::Messages;
            selected.api_key = Some("test-key".into());
            selected.context_window = smaller_window;
            let model_id = crate::agent::models::ModelId::new("test/small-window-model");
            let catalog = SessionActor::published_catalog_for_test(
                model_id.clone(),
                selected,
                None,
                std::time::Duration::from_secs(60),
                0,
                85,
            );
            let route = catalog.resolve_session_route(&model_id, None).unwrap();
            let (ack, applied) = tokio::sync::oneshot::channel();
            actor
                .admit_session_model_selection(route, Some(catalog), None, ack)
                .await;
            assert!(matches!(
                applied.await.unwrap().unwrap(),
                crate::session::DesiredStateOutcome::Applied(_)
            ));
            actor.state.lock().await.foreground = ForegroundState::Idle;
            assert_eq!(
                actor
                    .chat_state_handle
                    .get_sampling_config()
                    .await
                    .unwrap()
                    .context_window
                    .get(),
                smaller_window
            );

            actor
                .maybe_compact_on_model_switch()
                .await
                .expect("the legal source and retained tail fit the smaller window");
            assert!(
                actor.compaction.pending_async_notice.get().is_none(),
                "the newer replacement supersedes the deferred async notice"
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let completed_events = events
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
            assert_eq!(completed_events, 2, "both replacements committed");
            assert_eq!(server.messages_request_count(), 2);
            let (updates, completed) = drain_session_updates(&mut notifications);
            assert_eq!(
                completed.len(),
                1,
                "only the newer synchronous completion is emitted"
            );
            assert_eq!(completed[0]["update"]["async_compact"], json!(false));
            assert!(updates.iter().any(|kind| kind == "auto_compact_completed"));
            assert!(
                actor
                    .chat_state_handle
                    .get_conversation()
                    .await
                    .iter()
                    .any(|item| {
                        item.text_content()
                            .contains("replacement after route shrink")
                    })
            );
        }));
    });
}
