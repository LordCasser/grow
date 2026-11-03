use super::*;

#[test]
fn manual_compaction_flushes_its_frozen_pre_compaction_snapshot_to_memory_and_jsonl() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;
            use test_support::{InferenceEndpoint, InferenceRequestMatcher};

            let root = tempfile::tempdir().unwrap();
            let history = crate::session::storage::JsonlStorageAdapter::with_root(
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

            let server = MockInferenceServer::start().await.unwrap();
            let valid_markdown = "## Technical context\nThe session retains its original design decisions and verified evidence.\n\n## Decisions & rationale\nThe user chose to compact only after preserving useful facts in session memory.";
            let first_auxiliary_response = server.expect_response(
                "auxiliary Sideband response one",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn(valid_markdown, "test-model"),
            );
            let second_auxiliary_response = server.expect_response(
                "auxiliary Sideband response two",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn(valid_markdown, "test-model"),
            );
            let (mut actor, _gateway, _projection) = actor_with_sampler_and_storage_configured(
                &server,
                None,
                Some(history.clone()),
                None,
                Some((30, 1)),
            )
            .await;
            *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                &root.path().join("memory"),
                &root.path().join("memory"),
            ));
            actor.compaction.threshold_percent.set(30);
            let mut sampling_route = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .unwrap();
            sampling_route.context_window = std::num::NonZeroU64::new(100_000).unwrap();
            sampling_route.api_backend = sampling_types::ApiBackend::Messages;
            sampling_route.base_url = server.url();
            actor
                .chat_state_handle
                .replace_sampling_route(sampling_route);
            // Build a normal user/assistant Surface with closed turns. Its size
            // gives the real compaction planner a source range above its minimum.
            for prompt_index in 0..6 {
                let mut user = ConversationItem::user(format!(
                    "OLD_COMPACTION_SOURCE_{prompt_index}: preserve this verified history."
                ));
                user.set_prompt_index(prompt_index);
                actor.chat_state_handle.push_user_message(user);
                let _ = actor.chat_state_handle.get_conversation().await;
                actor.chat_state_handle.push_assistant_response(
                    ConversationItem::assistant(format!(
                        "Historical evidence {prompt_index}: {}",
                        "the validated design decision remains useful. ".repeat(500)
                    )),
                );
                let _ = actor.chat_state_handle.get_conversation().await;
            }
            actor
                .chat_state_handle
                .record_provider_context_anchor(40_000);
            let original_input_ref = actor
                .chat_state_handle
                .materialize_timeline(actor.session_info.id.to_string())
                .await
                .unwrap()
                .input_ref;

            let compact_result = actor
                .run_compact(Some("COMPACTION_SUMMARY_REQUEST: keep the decisions.".into()))
                .await;
            assert!(compact_result.is_ok(), "{compact_result:?}");
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                while actor
                    .memory
                    .flush_success_count
                    .load(std::sync::atomic::Ordering::Relaxed)
                    == 0
                {
                    assert_eq!(
                        actor
                            .memory
                            .flush_error_count
                            .load(std::sync::atomic::Ordering::Relaxed),
                        0,
                        "one auxiliary call must be the accepted memory flush"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("pre-compaction flush should write memory");
            let requests = server.requests();
            assert!(
                requests.len() >= 2,
                "expected flush and summary calls; {}",
                server.request_log_summary()
            );
            let flush_request = requests
                .iter()
                .find(|request| {
                    request.body.as_ref().is_some_and(|body| {
                        body.to_string().contains("Extract ALL useful information")
                    })
                })
                .expect("one auxiliary request is the memory flush");
            let flush_body = flush_request.body.as_ref().unwrap().to_string();
            assert!(flush_body.contains("OLD_COMPACTION_SOURCE_5"));
            assert!(!flush_body.contains("COMPACTION_SUMMARY_REQUEST"));
            let summary_request = requests
                .iter()
                .find(|request| {
                    request.body.as_ref().is_some_and(|body| {
                        body.to_string()
                            .contains("COMPACTION_SUMMARY_REQUEST: keep the decisions")
                    })
                })
                .expect("manual compaction sends its real summary request");
            first_auxiliary_response.assert_satisfied();
            second_auxiliary_response.assert_satisfied();
            let compacted_surface = actor.chat_state_handle.get_conversation().await;
            assert!(compacted_surface.len() < 14);
            let compacted_input_ref = actor
                .chat_state_handle
                .materialize_timeline(actor.session_info.id.to_string())
                .await
                .unwrap()
                .input_ref;
            assert_ne!(compacted_input_ref, original_input_ref);
            let loaded = history
                .load_session_without_updates(&actor.session_info)
                .await
                .unwrap();
            let memory_flush_spawn = loaded
                .timeline
                .events()
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Sideband(spawn)
                        if spawn.purpose == chat_state::SidebandPurpose::MemoryFlush =>
                    {
                        Some(spawn)
                    }
                    _ => None,
                })
                .expect("memory flush Sideband Spawn must be in session JSONL");
            assert_eq!(memory_flush_spawn.source_refs, vec![original_input_ref.clone()]);
            let ledgers = history
                .read_sideband_ledgers_sync(&actor.session_info, &loaded.timeline)
                .unwrap();
            let attempts = ledgers[&memory_flush_spawn.sideband_id]
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::SidebandEventKind::Attempt(attempt) => Some(attempt),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(!attempts.is_empty());
            for attempt in attempts {
                assert_eq!(attempt.input_refs, vec![original_input_ref.clone()]);
            }
            let memory_files = std::fs::read_dir(actor.memory.storage().unwrap().sessions_dir())
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(memory_files.len(), 1);
            assert!(
                std::fs::read_to_string(memory_files[0].as_ref().unwrap().path())
                    .unwrap()
                    .contains("retains its original design decisions")
            );
        }));
    });
}
