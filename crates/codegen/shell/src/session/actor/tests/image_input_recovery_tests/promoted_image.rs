use super::*;

#[test]
fn promoted_background_compaction_recovers_1210_and_commits_a_fresh_text_transaction() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;
            use test_support::{InferenceEndpoint, InferenceRequestMatcher};

            let server = MockInferenceServer::start().await.unwrap();
            let mut rejection = server.expect_response_blocked(
                "background compact image rejection",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                ScriptedResponse::json(
                    400,
                    json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "code": "1210",
                            "message": "messages.content.type 参数非法，取值范围 ['text']"
                        }
                    }),
                ),
            );
            let mut vision = server.expect_response(
                "recover rejected image with vision",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn("The screenshot shows service error E42.", "vision-model"),
            );
            let summary = format!(
                "Compacted history with durable screenshot evidence. {}",
                "Verified decisions and context. ".repeat(30)
            );
            let mut retry = server.expect_response(
                "fresh compact transaction after image projection",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn(&summary, "test"),
            );

            let root = tempfile::tempdir().unwrap();
            let history = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
                root.path().join("history"),
            );
            let info = SessionInfo {
                id: acp::SessionId::new("test-actor"),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            };
            history
                .init_session(&info, crate::agent::models::ModelId::new("test-model"))
                .await
                .unwrap();
            let seed = chat_state::Timeline::from_seed(vec![ConversationItem::system(
                "test system prompt",
            )])
            .unwrap();
            for event in seed.events() {
                history
                    .append_timeline_event_durable(&info, event)
                    .await
                    .unwrap();
            }
            let (actor, _gateway, persistence) = actor_with_sampler_and_storage(
                &server,
                Some("vision"),
                Some(history.clone()),
                None,
            )
            .await;

            let mut old_task = ConversationItem::user(format!(
                "Old task context. {}",
                "Old details. ".repeat(600)
            ));
            old_task.set_prompt_index(0);
            old_task.add_image(inline_data_uri(&test_image_content()));
            let mut recent_task = ConversationItem::user("recent task");
            recent_task.set_prompt_index(1);
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    old_task,
                    ConversationItem::assistant("x".repeat(200_000)),
                    recent_task,
                    ConversationItem::assistant("y".repeat(88_000)),
                ],
            )
            .await;
            let mut sampling = actor.chat_state_handle.get_sampling_config().await.unwrap();
            sampling.context_window = std::num::NonZeroU64::new(100_000).unwrap();
            actor.chat_state_handle.replace_sampling_route(sampling);
            actor.compaction.threshold_percent.set(80);
            actor.compaction.pre_prune.set(false);
            actor
                .chat_state_handle
                .record_provider_context_anchor(76_000);
            actor.state.lock().await.foreground = ForegroundState::RegularTurn(
                crate::session::actor::tests::support::running_task_stub(
                    "promoted-image-compaction",
                ),
            );

            actor.background_compaction_boundary().await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10), rejection.wait_blocked())
                .await
                .expect("background summary must reach the provider and remain held");
            let initial_requests = server.requests();
            assert_eq!(initial_requests.len(), 1);
            assert!(count_wire_images(initial_requests[0].body.as_ref().unwrap()) > 0);

            let promotion = tokio::task::spawn_local({
                let actor = actor.clone();
                async move {
                    actor
                        .run_compact_only(
                            crate::session::actor::compaction::AutoCompactTriggerInfo {
                                tokens_used: 85_000,
                                context_window: 100_000,
                                percentage: 85,
                                source: "pre_sampling",
                                max_output_tokens: None,
                            },
                        )
                        .await
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .iter()
                        .any(|event| {
                            matches!(
                                event.kind,
                                chat_state::TimelineEventKind::Compaction(
                                    chat_state::CompactionEvent::Promoted { .. }
                                )
                            )
                        })
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the pending background transaction must be promoted");
            assert!(!promotion.is_finished());
            rejection.release();

            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                vision.wait_satisfied().await;
                retry.wait_satisfied().await;
            })
            .await
            .expect(
                "the rejected transaction must OCR the image and complete a new text transaction",
            );
            promotion.await.unwrap().unwrap();

            let requests = server.requests();
            assert_eq!(
                requests.len(),
                3,
                "rejection, vision recovery, fresh compaction"
            );
            let vision_body = requests[1].body.as_ref().unwrap();
            assert_eq!(vision_body["model"], "vision-model");
            assert!(count_wire_images(vision_body) > 0);
            let retry_body = requests[2].body.as_ref().unwrap();
            assert_eq!(retry_body["model"], "test");
            assert_eq!(count_wire_images(retry_body), 0);
            assert!(
                retry_body
                    .to_string()
                    .contains("The screenshot shows service error E42.")
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let started = events
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Compaction(
                        chat_state::CompactionEvent::Started { id, .. },
                    ) => Some(id),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(started.len(), 2, "recovery must start a fresh transaction");
            assert_ne!(started[0], started[1]);
            assert!(events.iter().any(|event| matches!(
                event.kind,
                chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Failed { .. }
                )
            )));
            assert!(events.iter().any(|event| matches!(
                event.kind,
                chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Completed { .. }
                )
            )));
            assert!(events.iter().any(|event| matches!(
                event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )));
            let key = prepared_image_key(&actor).await;
            assert!(actor.model_image_input_is_unsupported(&key).await);
            let text = actor
                .chat_state_handle
                .get_conversation()
                .await
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                text.contains(summary.trim()),
                "completed compacted conversation omitted accepted summary: {text}"
            );
            assert!(
                persistence
                    .attempted_projections()
                    .iter()
                    .all(|(_, acknowledged)| *acknowledged)
            );

            let loaded = history.load_session_without_updates(&info).await.unwrap();
            let ledger = history
                .read_sideband_ledgers_sync(&info, &loaded.timeline)
                .unwrap();
            let image_description = loaded
                .timeline
                .events()
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Sideband(spawn)
                        if spawn.purpose == chat_state::SidebandPurpose::ImageDescription =>
                    {
                        Some(spawn)
                    }
                    _ => None,
                });
            let image_description = image_description.expect("OCR must have a durable Sideband");
            assert!(
                ledger[&image_description.sideband_id]
                    .iter()
                    .any(|event| matches!(
                        &event.kind,
                        chat_state::SidebandEventKind::Result(result)
                            if result.raw_output.contains("The screenshot shows service error E42.")
                    ))
            );
            assert!(
                ledger[&image_description.sideband_id]
                    .iter()
                    .any(|event| matches!(
                        &event.kind,
                        chat_state::SidebandEventKind::End(end)
                            if end.outcome == chat_state::SidebandOutcome::Completed
                    ))
            );
        }));
    });
}
