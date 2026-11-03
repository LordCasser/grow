use super::*;

#[test]
fn ephemeral_daily_log_skip_does_not_checkpoint_flush_as_written() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("## Evidence\nKeep this fact.");
            let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, None).await;
            let memory_root = tempfile::tempdir().unwrap();
            *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new(
                std::path::Path::new("/tmp/grow-ephemeral-flush"),
                Some(memory_root.path()),
            ));
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::user("Remember this fact."),
                    ConversationItem::assistant("It is important."),
                ],
            )
            .await;

            assert!(!actor.run_memory_flush("ephemeral-regression", None).await);
            assert_eq!(
                actor
                    .memory
                    .flush_error_count
                    .load(std::sync::atomic::Ordering::Relaxed),
                0,
                "an ephemeral policy skip is not a flush error"
            );
            assert!(
                !memory_root.path().join("sessions").exists(),
                "the ephemeral policy must not create the expected daily log"
            );
        }));
    });
}

pub(super) fn image_history(tool_image: bool) -> Vec<ConversationItem> {
    let image = inline_data_uri(&test_image_content());
    if tool_image {
        vec![
            ConversationItem::user("source context"),
            ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                id: "read_image".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result_with_images(
                "read_image",
                "tool evidence",
                vec![sampling_types::ContentPart::Image {
                    url: image.into(),
                    description: None,
                }],
            ),
        ]
    } else {
        let mut user = ConversationItem::user("source context");
        user.add_image(image);
        vec![user]
    }
}

async fn run_auxiliary(actor: &SessionActor, flush: bool) {
    if flush {
        let _ = actor.run_memory_flush("image-regression", None).await;
    } else {
        actor.handle_recap(false, None).await;
    }
}

fn assert_described_images(flush: bool) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for backend in [
                sampling_types::ApiBackend::Messages,
                sampling_types::ApiBackend::ChatCompletions,
                sampling_types::ApiBackend::Responses,
            ] {
                for tool_image in [false, true] {
                    for known_text in [false, true] {
                        let server = MockInferenceServer::start().await.unwrap();
                        server.enqueue_response(
                            "/v1/messages",
                            messages_text_turn("Screenshot error E42.", "vision-model"),
                        );
                        server.set_response("## Evidence\nScreenshot error E42.");
                        let (actor, _gateway, persistence) =
                            actor_with_sampler_and_persistence(&server, Some("vision")).await;
                        let memory_root = tempfile::tempdir().unwrap();
                        *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                            memory_root.path(),
                            memory_root.path(),
                        ));
                        replace_test_surface(&actor.chat_state_handle, image_history(tool_image))
                            .await;
                        actor
                            .project_conversation_images_for_text_model(
                                &prepared_image_key(&actor).await,
                            )
                            .await
                            .unwrap();
                        let mut config =
                            actor.chat_state_handle.get_sampling_config().await.unwrap();
                        config.model = "text-model".into();
                        config.api_backend = backend.clone();
                        actor.chat_state_handle.replace_sampling_route(config);
                        let key = prepared_image_key(&actor).await;
                        if known_text {
                            actor
                                .record_unsupported_model_image_input(key.clone())
                                .await
                                .unwrap();
                        }
                        let before =
                            serde_json::to_value(actor.chat_state_handle.get_conversation().await)
                                .unwrap();
                        run_auxiliary(&actor, flush).await;
                        let requests = server.requests();
                        assert_eq!(requests.len(), 2);
                        let body = requests[1].body.as_ref().unwrap();
                        assert_eq!(
                            count_wire_images(body),
                            0,
                            "flush={flush}, {backend:?}, tool={tool_image}, known={known_text}"
                        );
                        assert!(body.to_string().contains("Screenshot error E42."));
                        assert_eq!(body["model"], "text-model");
                        if tool_image {
                            assert!(body.to_string().contains("tool evidence"));
                        }
                        assert!(
                            body.get("tools")
                                .is_none_or(|tools| tools.as_array().unwrap().is_empty())
                        );
                        assert_eq!(persistence.attempted_projections().len(), 1);
                        assert_eq!(
                            actor.model_image_input_is_unsupported(&key).await,
                            known_text
                        );
                        assert_eq!(
                            serde_json::to_value(actor.chat_state_handle.get_conversation().await)
                                .unwrap(),
                            before
                        );
                        if flush {
                            let files =
                                std::fs::read_dir(actor.memory.storage().unwrap().sessions_dir())
                                    .unwrap()
                                    .collect::<Vec<_>>();
                            assert_eq!(files.len(), 1);
                            let text =
                                std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
                            assert!(text.contains("Screenshot error E42."));
                        } else {
                            assert_eq!(actor.last_recap_main_turn.get(), 1);
                        }
                    }
                }
            }
        }));
    });
}

#[test]
fn recap_uses_descriptions_after_model_switch_on_all_backends() {
    assert_described_images(false);
}

#[test]
fn memory_flush_uses_descriptions_after_model_switch_on_all_backends() {
    assert_described_images(true);
}

#[test]
fn previous_auxiliary_assembly_replays_the_text_only_content_type_rejection() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for flush in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                server.enqueue_response("/v1/messages", messages_text_turn("Screenshot E42.", "vision-model"));
                server.enqueue_response("/v1/messages", ScriptedResponse::json(400, json!({
                    "error": {"type": "invalid_request_error", "message": "1210: messages.content.type 参数非法，取值范围 ['text']"}
                })));
                server.set_response("NO_REPLY");
                let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, Some("vision")).await;
                let memory_root = tempfile::tempdir().unwrap();
                *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(memory_root.path(), memory_root.path()));
                replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
                actor.project_conversation_images_for_text_model(&prepared_image_key(&actor).await).await.unwrap();
                let raw = actor.chat_state_handle.materialize_timeline(actor.session_info.id.to_string()).await.unwrap();
                // Replay the prior actor assembly using the unchanged real builders.
                // They must still preserve media when their caller chooses raw context.
                let items = if flush {
                    crate::session::helpers::memory_flush::build_flush_request_items(
                        &raw.surface, &raw.surface_ids, crate::session::helpers::memory_flush::FLUSH_SYSTEM_PROMPT,
                    ).unwrap().0
                } else {
                    crate::session::helpers::session_recap::budget_recap_items(raw.surface, "system-reminder", true, 256_000)
                };
                let prepared = actor.prepare_chat_completion_config(false).await;
                let request = ConversationRequest { items, model: Some(prepared.model.clone()), ..Default::default() };
                let client = sampler::SamplingClient::new(prepared).unwrap();
                let error = client.conversation_collect(request).await.unwrap_err();
                assert!(error.to_string().contains("messages.content.type"));
                assert_eq!(count_wire_images(server.requests()[1].body.as_ref().unwrap()), 1);
                run_auxiliary(&actor, flush).await;
                let requests = server.requests();
                assert_eq!(requests.len(), 3);
                assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
            }
        }));
    });
}

#[test]
fn auxiliary_image_context_waits_for_projection_ack_and_handles_failure() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for flush in [false, true] {
                for fail in [false, true] {
                    let server = MockInferenceServer::start().await.unwrap();
                    server.enqueue_response(
                        "/v1/messages",
                        messages_text_turn("Screenshot E42.", "vision-model"),
                    );
                    server.set_response("NO_REPLY");
                    let (actor, _gateway, persistence) =
                        actor_with_sampler_and_persistence(&server, Some("vision")).await;
                    let memory_root = tempfile::tempdir().unwrap();
                    *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                        memory_root.path(),
                        memory_root.path(),
                    ));
                    replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
                    let key = prepared_image_key(&actor).await;
                    let mut later_route =
                        actor.chat_state_handle.get_sampling_config().await.unwrap();
                    let prepared_model = later_route.model.clone();
                    actor
                        .record_unsupported_model_image_input(key.clone())
                        .await
                        .unwrap();
                    persistence.hold_next_sideband_attempt();
                    if fail {
                        persistence.fail_projection_commits();
                    } else {
                        persistence.hold_projection_acknowledgement();
                    }
                    let main_actor = actor.clone();
                    let conversion = tokio::task::spawn_local(async move {
                        main_actor
                            .project_conversation_images_for_text_model(&key)
                            .await
                    });
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        while !persistence.sideband_ack_is_pending() {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    let auxiliary_actor = actor.clone();
                    let mut auxiliary = tokio::task::spawn_local(async move {
                        run_auxiliary(&auxiliary_actor, flush).await;
                    });
                    assert!(
                        tokio::time::timeout(std::time::Duration::from_millis(20), &mut auxiliary)
                            .await
                            .is_err()
                    );
                    assert!(server.requests().is_empty());
                    persistence.release_sideband_ack();
                    if !fail {
                        wait_for_image_projection(&persistence).await;
                        assert!(
                            tokio::time::timeout(
                                std::time::Duration::from_millis(20),
                                &mut auxiliary
                            )
                            .await
                            .is_err()
                        );
                        assert_eq!(server.requests().len(), 1);
                        if flush {
                            later_route.model = "later-model".into();
                            later_route.base_url = "http://127.0.0.1:1".into();
                            actor.chat_state_handle.replace_sampling_route(later_route);
                        }
                        persistence.hold_next_sideband_attempt();
                        persistence.release_projection_acknowledgement();
                        conversion.await.unwrap().unwrap();
                        tokio::time::timeout(std::time::Duration::from_secs(5), async {
                            while !persistence.sideband_ack_is_pending() {
                                tokio::task::yield_now().await;
                            }
                        })
                        .await
                        .unwrap();
                        assert!(actor.image_projection_gate.try_write().is_ok());
                        persistence.release_sideband_ack();
                    } else {
                        assert!(conversion.await.unwrap().is_err());
                    }
                    tokio::time::timeout(std::time::Duration::from_secs(5), auxiliary)
                        .await
                        .unwrap()
                        .unwrap();
                    let requests = server.requests();
                    assert_eq!(requests.len(), if fail { 1 } else { 2 });
                    if !fail {
                        let body = requests[1].body.as_ref().unwrap();
                        assert_eq!(body["model"], prepared_model);
                        assert_eq!(count_wire_images(body), 0);
                        assert!(body.to_string().contains("Screenshot E42."));
                    }
                    assert!(
                        !actor
                            .memory
                            .is_flushing
                            .load(std::sync::atomic::Ordering::Relaxed)
                    );
                    assert!(actor.image_projection_gate.try_write().is_ok());
                }
            }
        }));
    });
}

#[test]
fn auxiliary_image_context_waiting_observes_session_cancellation() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for flush in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, None).await;
                let memory_root = tempfile::tempdir().unwrap();
                *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                    memory_root.path(),
                    memory_root.path(),
                ));
                replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
                let projection = actor.image_projection_gate.write().await;
                let auxiliary_actor = actor.clone();
                let mut auxiliary = tokio::task::spawn_local(async move {
                    run_auxiliary(&auxiliary_actor, flush).await;
                });
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(20), &mut auxiliary)
                        .await
                        .is_err()
                );
                actor.sideband_cancel.cancel();
                tokio::time::timeout(std::time::Duration::from_secs(1), auxiliary)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(server.requests().is_empty());
                assert!(
                    !actor
                        .memory
                        .is_flushing
                        .load(std::sync::atomic::Ordering::Relaxed)
                );
                drop(projection);
            }
        }));
    });
}

#[test]
fn auxiliary_ignores_unresolved_images_outside_the_retained_budget() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for flush in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                server.set_response("NO_REPLY");
                let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, None).await;
                let mut history = image_history(false);
                history.push(ConversationItem::assistant("old evidence ".repeat(10_000)));
                history.push(ConversationItem::user("latest retained evidence"));
                history.extend((0..22).map(|_| ConversationItem::assistant("recent result")));
                let memory_root = tempfile::tempdir().unwrap();
                *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                    memory_root.path(),
                    memory_root.path(),
                ));
                replace_test_surface(&actor.chat_state_handle, history).await;
                let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                config.context_window = std::num::NonZeroU64::new(8_000).unwrap();
                actor.chat_state_handle.replace_sampling_route(config);
                actor
                    .record_unsupported_model_image_input(prepared_image_key(&actor).await)
                    .await
                    .unwrap();
                run_auxiliary(&actor, flush).await;
                let requests = server.requests();
                assert_eq!(requests.len(), 1);
                let body = requests[0].body.as_ref().unwrap();
                assert_eq!(count_wire_images(body), 0);
                assert!(body.to_string().contains("latest retained evidence"));
            }
        }));
    });
}

#[test]
fn memory_flush_keeps_pre_compaction_sources_and_writes_durable_text() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;
            let root = tempfile::tempdir().unwrap();
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
                root.path().join("history"),
            );
            let info = SessionInfo {
                id: acp::SessionId::new("test-actor"),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            };
            storage
                .init_session(&info, crate::agent::models::ModelId::new("test-model"))
                .await
                .unwrap();
            let seed = chat_state::Timeline::from_seed(vec![ConversationItem::system(
                "test system prompt",
            )])
            .unwrap();
            for event in seed.events() {
                storage
                    .append_timeline_event_durable(&info, event)
                    .await
                    .unwrap();
            }
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Archived screenshot E42.", "vision-model"),
            );
            server.set_response("## Evidence\nArchived screenshot E42.");
            let (actor, _gateway, _) = actor_with_sampler_and_storage(
                &server,
                Some("vision"),
                Some(storage.clone()),
                None,
            )
            .await;
            let memory_root = root.path().join("memory");
            *actor.memory.storage.borrow_mut() =
                Some(memory::MemoryStorage::new_flat(&memory_root, &memory_root));
            replace_test_surface(&actor.chat_state_handle, image_history(true)).await;
            let key = prepared_image_key(&actor).await;
            actor
                .record_unsupported_model_image_input(key.clone())
                .await
                .unwrap();
            actor
                .project_conversation_images_for_text_model(&key)
                .await
                .unwrap();
            let source = actor
                .chat_state_handle
                .materialize_timeline(info.id.to_string())
                .await
                .unwrap();
            let snapshot = actor.snapshot_memory_flush_state().await.unwrap();
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user(
                    "newer live input must not enter the flush",
                ));
            let before =
                serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap();
            assert!(actor.run_memory_flush("frozen", Some(snapshot)).await);
            let requests = server.requests();
            assert_eq!(requests.len(), 2);
            let body = requests[1].body.as_ref().unwrap();
            let wire = body.to_string();
            assert_eq!(count_wire_images(body), 0);
            assert!(wire.contains("Archived screenshot E42.") && wire.contains("tool evidence"));
            assert!(!wire.contains("newer live input"));
            assert_eq!(
                serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                before
            );
            let loaded = storage.load_session_without_updates(&info).await.unwrap();
            let spawn = loaded
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
                .unwrap();
            assert_eq!(spawn.source_refs, vec![source.input_ref.clone()]);
            let ledgers = storage
                .read_sideband_ledgers_sync(&info, &loaded.timeline)
                .unwrap();
            crate::session::storage::validate_sideband_ledgers(
                &info.id.to_string(),
                &loaded.timeline,
                &ledgers,
            )
            .unwrap();
            let attempt = ledgers[&spawn.sideband_id]
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::SidebandEventKind::Attempt(attempt) => Some(attempt),
                    _ => None,
                })
                .unwrap();
            assert_eq!(attempt.input_refs, vec![source.input_ref]);
            assert_eq!(
                attempt.assembly_manifest.source_revision,
                Some(source.surface_revision)
            );
            assert_eq!(
                attempt.assembly_manifest.context_surface_ids,
                source
                    .surface_ids
                    .iter()
                    .copied()
                    .zip(&source.surface)
                    .filter_map(
                        |(id, item)| (!matches!(item, ConversationItem::System(_))).then_some(id)
                    )
                    .collect::<Vec<_>>()
            );
            let files = std::fs::read_dir(actor.memory.storage().unwrap().sessions_dir())
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(files.len(), 1);
            assert!(
                std::fs::read_to_string(files[0].as_ref().unwrap().path())
                    .unwrap()
                    .contains("Archived screenshot E42.")
            );
        }));
    });
}

#[test]
fn memory_flush_checks_the_actual_override_model_image_key() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for override_known_text in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                server.enqueue_response("/v1/messages", multimodal_capability_rejection());
                let (actor, _gateway, _) =
                    actor_with_sampler_and_storage(&server, None, None, Some("flush-model")).await;
                let memory_root = tempfile::tempdir().unwrap();
                *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                    memory_root.path(),
                    memory_root.path(),
                ));
                let mut info = crate::agent::config::ModelInfo::baseline("flush-model");
                info.base_url = server.url();
                info.api_backend = sampling_types::ApiBackend::Messages;
                actor.models_manager.insert_test_entry(
                    "flush-model",
                    crate::agent::config::ModelEntry {
                        info,
                        api_key: Some("test-key".into()),
                        env_key: None,
                        auth_provider: None,
                    },
                );
                replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
                let mut prepared = actor.prepare_chat_completion_config(false).await;
                if override_known_text {
                    prepared.model = "flush-model".into();
                }
                let key = sampling_types::model_image_input_key_from_parts(
                    &prepared.model,
                    &prepared.api_backend,
                    &prepared.base_url,
                    &prepared.query_params,
                );
                actor
                    .record_unsupported_model_image_input(key)
                    .await
                    .unwrap();
                assert!(!actor.run_memory_flush("override", None).await);
                let requests = server.requests();
                assert_eq!(requests.len(), usize::from(!override_known_text));
                if !override_known_text {
                    assert_eq!(requests[0].body.as_ref().unwrap()["model"], "flush-model");
                    assert_eq!(count_wire_images(requests[0].body.as_ref().unwrap()), 1);
                }
            }
        }));
    });
}
