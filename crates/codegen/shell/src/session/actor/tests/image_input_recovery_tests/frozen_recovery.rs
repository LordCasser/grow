use super::auxiliary::image_history;
use super::*;

async fn exercise_consumer(actor: &std::sync::Arc<SessionActor>, consumer: &str) {
    match consumer {
        "btw" => assert!(
            actor
                .handle_side_question("Explain the image")
                .await
                .unwrap()
                .contains("E42")
        ),
        "recap" => {
            actor.handle_recap(false, None).await;
            assert_eq!(actor.last_recap_main_turn.get(), 1);
        }
        "flush" => assert!(actor.run_memory_flush("frozen-regression", None).await),
        "coordination" => {
            let (inquiry, response) = image_context_inquiry(actor);
            actor.enqueue_coordination_inquiry(inquiry).await;
            let result = response.await.unwrap();
            assert_eq!(
                result.status,
                crate::coordination::InquiryStatus::Answered,
                "{result:?}"
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn selected_frozen_images_recover_on_all_auxiliary_routes_and_backends() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for consumer in ["btw", "recap", "flush", "coordination"] {
                for backend in [
                    sampling_types::ApiBackend::Messages,
                    sampling_types::ApiBackend::ChatCompletions,
                    sampling_types::ApiBackend::Responses,
                ] {
                    for tool_image in [false, true] {
                        for known in [false, true] {
                            let server = MockInferenceServer::start().await.unwrap();
                            if !known {
                                let path = match backend {
                                    sampling_types::ApiBackend::Messages => "/v1/messages",
                                    sampling_types::ApiBackend::ChatCompletions => {
                                        "/v1/chat/completions"
                                    }
                                    sampling_types::ApiBackend::Responses => "/v1/responses",
                                };
                                server.enqueue_response(path, multimodal_capability_rejection());
                            }
                            server.enqueue_response(
                                "/v1/messages",
                                messages_text_turn("Screenshot error E42.", "vision-model"),
                            );
                            server.set_response("## Evidence\nScreenshot error E42.");
                            let (actor, _gateway, persistence) =
                                actor_with_sampler_and_persistence(&server, Some("vision")).await;
                            let memory_root = tempfile::tempdir().unwrap();
                            *actor.memory.storage.borrow_mut() =
                                Some(memory::MemoryStorage::new_flat(
                                    memory_root.path(),
                                    memory_root.path(),
                                ));
                            replace_test_surface(
                                &actor.chat_state_handle,
                                image_history(tool_image),
                            )
                            .await;
                            let mut route =
                                actor.chat_state_handle.get_sampling_config().await.unwrap();
                            route.model = "text-model".into();
                            route.api_backend = backend.clone();
                            actor.chat_state_handle.replace_sampling_route(route);
                            let key = prepared_image_key(&actor).await;
                            if known {
                                actor
                                    .record_unsupported_model_image_input(key.clone())
                                    .await
                                    .unwrap();
                            }
                            let before = serde_json::to_value(
                                actor.chat_state_handle.get_conversation().await,
                            )
                            .unwrap();
                            exercise_consumer(&actor, consumer).await;
                            let requests = server.requests();
                            assert_eq!(
                                requests.len(),
                                if known { 2 } else { 3 },
                                "{consumer}, tool={tool_image}, known={known}"
                            );
                            let last = requests.last().unwrap().body.as_ref().unwrap();
                            assert_eq!(last["model"], "text-model");
                            assert_eq!(count_wire_images(last), 0);
                            assert!(last.to_string().contains("Screenshot error E42."));
                            if tool_image {
                                assert!(last.to_string().contains("tool evidence"));
                            }
                            assert!(actor.model_image_input_is_unsupported(&key).await);
                            assert!(
                                persistence.attempted_projections().is_empty(),
                                "Sideband must not write main projections"
                            );
                            assert_eq!(
                                serde_json::to_value(
                                    actor.chat_state_handle.get_conversation().await
                                )
                                .unwrap(),
                                before
                            );
                            if consumer == "flush" {
                                let files = std::fs::read_dir(
                                    actor.memory.storage().unwrap().sessions_dir(),
                                )
                                .unwrap()
                                .collect::<Vec<_>>();
                                assert_eq!(files.len(), 1);
                                assert!(
                                    std::fs::read_to_string(files[0].as_ref().unwrap().path())
                                        .unwrap()
                                        .contains("E42")
                                );
                            }
                        }
                    }
                }
            }
        }));
    });
}

#[test]
fn frozen_image_recovery_excludes_concurrent_input_and_reuses_description_cache() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Screenshot E42.", "vision-model"),
            );
            server.set_response("E42 is the old screenshot error.");
            let (actor, _gateway, persistence) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            replace_test_surface(&actor.chat_state_handle, image_history(true)).await;
            persistence.hold_next_sideband_attempt();
            let other = actor.clone();
            let answer = tokio::task::spawn_local(async move {
                other.handle_side_question("Explain old screenshot").await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !persistence.sideband_ack_is_pending() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("NEW INPUT MUST NOT LEAK"));
            let _ = actor.chat_state_handle.get_conversation().await;
            persistence.release_sideband_ack();
            assert!(answer.await.unwrap().unwrap().contains("E42"));
            let requests = server.requests();
            assert_eq!(requests.len(), 3);
            for request in &requests {
                assert!(
                    !request
                        .body
                        .as_ref()
                        .unwrap()
                        .to_string()
                        .contains("NEW INPUT MUST NOT LEAK")
                );
            }
            assert!(
                actor
                    .handle_side_question("Explain again")
                    .await
                    .unwrap()
                    .contains("E42")
            );
            let requests = server.requests();
            assert_eq!(
                requests.len(),
                4,
                "same immutable image source must reuse neutral description"
            );
            assert_eq!(count_wire_images(requests[3].body.as_ref().unwrap()), 0);
            assert!(
                requests[3]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("NEW INPUT MUST NOT LEAK")
            );
            assert!(persistence.attempted_projections().is_empty());
        }));
    });
}

#[test]
fn ordinary_bad_request_does_not_learn_capability_or_start_recovery() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", ScriptedResponse::json(400, json!({"error":{"message":"invalid temperature","type":"invalid_request_error"}})));
            let (actor, _gateway, persistence) = actor_with_sampler_and_persistence(&server, Some("vision")).await;
            replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
            let key = prepared_image_key(&actor).await;
            assert!(actor.handle_side_question("Explain").await.is_err());
            assert_eq!(server.requests().len(), 1);
            assert!(!actor.model_image_input_is_unsupported(&key).await);
            assert!(persistence.attempted_projections().is_empty());
        }));
    });
}

#[test]
fn cancelled_frozen_recovery_never_dispatches_its_held_description() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _gateway, persistence) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
            actor
                .record_unsupported_model_image_input(prepared_image_key(&actor).await)
                .await
                .unwrap();
            persistence.hold_next_sideband_attempt();
            let other = actor.clone();
            let answer =
                tokio::task::spawn_local(
                    async move { other.handle_side_question("Explain").await },
                );
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !persistence.sideband_ack_is_pending() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            actor.sideband_cancel.cancel();
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(1), answer)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
            persistence.release_sideband_ack();
            assert!(server.requests().is_empty());
            assert!(persistence.attempted_projections().is_empty());
            assert!(actor.image_projection_gate.try_write().is_ok());
        }));
    });
}

#[cfg(unix)]
#[test]
fn missing_frozen_image_description_fails_without_dropping_original_media() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let ocr = ocr_fixture("exit 1");
            crate::session::image_describe::TEST_LOCAL_OCR_PROGRAM
                .scope(ocr.path().join("tesseract"), async {
                    let server = MockInferenceServer::start().await.unwrap();
                    server.enqueue_response(
                        "/v1/messages",
                        ScriptedResponse::json(
                            503,
                            json!({"error":{"message":"vision unavailable","type":"server_error"}}),
                        ),
                    );
                    let (actor, _gateway, persistence) =
                        actor_with_sampler_and_persistence(&server, Some("vision")).await;
                    replace_test_surface(&actor.chat_state_handle, image_history(true)).await;
                    let key = prepared_image_key(&actor).await;
                    actor
                        .record_unsupported_model_image_input(key)
                        .await
                        .unwrap();
                    let before =
                        serde_json::to_value(actor.chat_state_handle.get_conversation().await)
                            .unwrap();
                    assert!(actor.handle_side_question("Explain").await.is_err());
                    assert_eq!(
                        server.requests().len(),
                        1,
                        "only the configured vision request is admitted"
                    );
                    assert!(persistence.attempted_projections().is_empty());
                    assert_eq!(
                        serde_json::to_value(actor.chat_state_handle.get_conversation().await)
                            .unwrap(),
                        before
                    );
                })
                .await;
        }));
    });
}

#[test]
fn recovered_pre_compaction_flush_retains_original_coordinates_on_disk() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Old screenshot E42.", "vision-model"),
            );
            server.set_response("## Evidence\nOld screenshot E42.");
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
            let (actor, _gateway, _) = actor_with_sampler_and_storage(
                &server,
                Some("vision"),
                Some(history.clone()),
                None,
            )
            .await;
            *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                &root.path().join("memory"),
                &root.path().join("memory"),
            ));
            replace_test_surface(&actor.chat_state_handle, image_history(true)).await;
            let frozen = actor.freeze_sideband_surface().await.unwrap();
            let expected_revision = frozen.surface_revision;
            let expected_ids = frozen.surface_ids;
            let expected_ref = frozen.input_ref;
            let snapshot = actor.snapshot_memory_flush_state().await.unwrap();
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("NEW INPUT MUST NOT LEAK"));
            let _ = actor.chat_state_handle.get_conversation().await;
            assert!(
                actor
                    .run_memory_flush("pre_compaction", Some(snapshot))
                    .await
            );
            let requests = server.requests();
            assert_eq!(requests.len(), 3);
            assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
            assert!(
                !requests[2]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("NEW INPUT MUST NOT LEAK")
            );
            let loaded = history.load_session_without_updates(&info).await.unwrap();
            let ledgers = history
                .read_sideband_ledgers_sync(&info, &loaded.timeline)
                .unwrap();
            let flush_spawn = loaded
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
            assert_eq!(flush_spawn.source_refs, vec![expected_ref.clone()]);
            let attempts = ledgers[&flush_spawn.sideband_id]
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::SidebandEventKind::Attempt(attempt) => Some(attempt),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(attempts.len(), 2);
            for attempt in attempts {
                assert_eq!(attempt.input_refs, vec![expected_ref.clone()]);
                assert_eq!(
                    attempt.assembly_manifest.source_revision,
                    Some(expected_revision)
                );
                assert!(
                    attempt
                        .assembly_manifest
                        .context_surface_ids
                        .iter()
                        .all(|id| expected_ids.contains(id))
                );
            }
            let files = std::fs::read_dir(actor.memory.storage().unwrap().sessions_dir())
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(files.len(), 1);
            assert!(
                std::fs::read_to_string(files[0].as_ref().unwrap().path())
                    .unwrap()
                    .contains("Old screenshot E42.")
            );
        }));
    });
}

#[test]
fn manual_flush_slash_admits_while_foreground_owns_a_turn() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("## Evidence\nE42 is retained.");
            let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, None).await;
            let memory_root = tempfile::tempdir().unwrap();
            *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
                memory_root.path(),
                memory_root.path(),
            ));
            let bridge = actor.agent.borrow().tool_bridge().clone();
            actor.register_memory_tools(&bridge).await.unwrap();
            replace_test_surface(
                &actor.chat_state_handle,
                vec![ConversationItem::user("remember E42")],
            )
            .await;
            actor.state.lock().await.foreground =
                ForegroundState::RegularTurn(running_task_stub("busy-flush-main"));
            let before =
                serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap();
            actor
                .execute_out_of_band_slash_command(crate::session::HostCommandInvocation {
                    command: "/flush".into(),
                    description: "flush".into(),
                    invocation_id: "busy-flush".into(),
                })
                .await
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .memory
                    .flush_count
                    .load(std::sync::atomic::Ordering::Relaxed)
                    == 0
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(server.requests().len(), 1);
            assert!(actor.state.lock().await.pending_inputs.is_empty());
            assert_eq!(
                serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                before
            );
        }));
    });
}

#[test]
fn recovered_flush_text_uses_the_existing_input_ceiling() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for fits in [true, false] {
                let server = MockInferenceServer::start().await.unwrap();
                server.enqueue_response("/v1/messages", multimodal_capability_rejection());
                let description = "visible image evidence ".repeat(if fits { 300 } else { 7_000 });
                server.enqueue_response(
                    "/v1/messages",
                    messages_text_turn(&description, "vision-model"),
                );
                server.set_response("## Evidence\nE42 was retained.");
                let (actor, _gateway, persistence) =
                    actor_with_sampler_and_persistence(&server, Some("vision")).await;
                let root = tempfile::tempdir().unwrap();
                *actor.memory.storage.borrow_mut() =
                    Some(memory::MemoryStorage::new_flat(root.path(), root.path()));
                replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
                assert_eq!(
                    actor.run_memory_flush("budget-regression", None).await,
                    fits
                );
                assert_eq!(server.requests().len(), if fits { 3 } else { 2 });
                assert_eq!(
                    actor.memory.storage().unwrap().sessions_dir().exists(),
                    fits
                );
                assert!(persistence.attempted_projections().is_empty());
                if fits {
                    assert_eq!(
                        count_wire_images(server.requests()[2].body.as_ref().unwrap()),
                        0
                    );
                }
            }
        }));
    });
}

#[test]
fn unavailable_flush_model_never_falls_back_to_primary_provider() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _gateway, _) =
                actor_with_sampler_and_storage(&server, None, None, Some("missing/provider-alias"))
                    .await;
            let root = tempfile::tempdir().unwrap();
            *actor.memory.storage.borrow_mut() =
                Some(memory::MemoryStorage::new_flat(root.path(), root.path()));
            replace_test_surface(
                &actor.chat_state_handle,
                vec![ConversationItem::user("remember E42")],
            )
            .await;
            assert!(!actor.run_memory_flush("invalid-model", None).await);
            assert!(server.requests().is_empty());
            assert!(!actor.memory.storage().unwrap().sessions_dir().exists());
        }));
    });
}

#[test]
fn identical_old_images_outside_flush_window_do_not_start_description_work() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Latest screenshot E42.", "vision-model"),
            );
            server.set_response("## Evidence\nE42 was retained.");
            let (actor, _gateway, _) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            let root = tempfile::tempdir().unwrap();
            *actor.memory.storage.borrow_mut() =
                Some(memory::MemoryStorage::new_flat(root.path(), root.path()));
            let mut history = image_history(false);
            history.push(ConversationItem::assistant("old content ".repeat(20_000)));
            history.extend(image_history(false));
            replace_test_surface(&actor.chat_state_handle, history).await;
            actor
                .record_unsupported_model_image_input(prepared_image_key(&actor).await)
                .await
                .unwrap();
            assert!(
                actor
                    .run_memory_flush("selected-source-regression", None)
                    .await
            );
            assert_eq!(
                server.requests().len(),
                2,
                "only the latest selected image source needs a description"
            );
            assert_eq!(
                count_wire_images(server.requests()[1].body.as_ref().unwrap()),
                0
            );
        }));
    });
}

#[test]
fn memory_off_during_image_rejection_does_not_start_recovery_provider() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let mut rejection = server.expect_response_blocked(
                "memory flush image rejection",
                test_support::InferenceRequestMatcher::auxiliary(
                    test_support::InferenceEndpoint::Messages,
                ),
                multimodal_capability_rejection(),
            );
            server.set_response("A description that must not be requested.");
            let (actor, _gateway, _) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            let root = tempfile::tempdir().unwrap();
            *actor.memory.storage.borrow_mut() =
                Some(memory::MemoryStorage::new_flat(root.path(), root.path()));
            replace_test_surface(&actor.chat_state_handle, image_history(false)).await;
            let other = actor.clone();
            let flush = tokio::task::spawn_local(async move {
                other.run_memory_flush("disabled-recovery", None).await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), rejection.wait_blocked())
                .await
                .unwrap();
            super::disabled_memory::turn_memory_off(&actor).await;
            rejection.release();
            assert!(
                !tokio::time::timeout(std::time::Duration::from_secs(5), flush)
                    .await
                    .unwrap()
                    .unwrap()
            );
            assert_eq!(
                server.requests().len(),
                1,
                "a disabled flush cannot dispatch its vision fallback"
            );
            assert!(actor.memory.storage().is_none());
        }));
    });
}
