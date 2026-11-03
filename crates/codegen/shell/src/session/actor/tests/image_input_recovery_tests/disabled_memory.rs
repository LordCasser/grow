use super::*;

pub(super) async fn turn_memory_off(actor: &std::sync::Arc<SessionActor>) {
    let invocation = crate::session::HostCommandInvocation {
        command: "/memory off".into(),
        description: "Toggle session memory".into(),
        invocation_id: "disable-memory-test".into(),
    };
    crate::session::actor::slash_exec::HOST_COMMAND_INVOCATION
        .scope(
            invocation,
            actor.execute_builtin_slash_command(
                crate::session::slash_commands::BuiltinAction::MemoryToggle { enabled: false },
            ),
        )
        .await
        .unwrap();
    assert!(!actor.memory.is_enabled());
}

fn install_memory_storage(actor: &SessionActor, root: &std::path::Path) -> std::path::PathBuf {
    let storage = memory::MemoryStorage::new_flat(root, root);
    let sessions_dir = storage.sessions_dir();
    *actor.memory.storage.borrow_mut() = Some(storage);
    sessions_dir
}

#[test]
fn memory_off_during_flush_snapshot_wait_prevents_provider_work() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("## Evidence\nThis should not be requested.");
            let (actor, _gateway, _) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            let root = tempfile::tempdir().unwrap();
            let sessions_dir = install_memory_storage(&actor, root.path());
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::user("source"),
                    ConversationItem::assistant("reply"),
                ],
            )
            .await;

            let image_gate = actor.image_projection_gate.write().await;
            let flushing_actor = actor.clone();
            let mut flush = tokio::task::spawn_local(async move {
                flushing_actor
                    .run_memory_flush("runtime-disabled-test", None)
                    .await
            });
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(25), &mut flush,)
                    .await
                    .is_err()
            );

            turn_memory_off(&actor).await;
            drop(image_gate);
            assert!(!flush.await.unwrap());

            assert!(server.requests().is_empty());
            assert!(!sessions_dir.exists());
        }));
    });
}

#[test]
fn memory_off_after_durable_flush_attempt_ack_cancels_before_http() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;

            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("## Evidence\nThis should not be requested.");
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
            let (actor, mut gateway, persistence) = actor_with_sampler_and_storage(
                &server,
                Some("vision"),
                Some(history.clone()),
                None,
            )
            .await;
            let sessions_dir = install_memory_storage(&actor, &root.path().join("memory"));
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::user("source"),
                    ConversationItem::assistant("reply"),
                ],
            )
            .await;
            persistence.hold_next_sideband_attempt();

            let flushing_actor = actor.clone();
            let flush = tokio::task::spawn_local(async move {
                flushing_actor
                    .run_memory_flush("runtime-disabled-test", None)
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !persistence.sideband_ack_is_pending() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(server.requests().is_empty());

            turn_memory_off(&actor).await;
            persistence.release_sideband_ack();
            assert!(!flush.await.unwrap());

            let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while let Some(message) = gateway.recv().await {
                    let acp_transport::AcpClientMessage::ExtNotification(args) = &message else {
                        continue;
                    };
                    if args.request.method.as_ref() != "grow/session_notification" {
                        continue;
                    }
                    let Ok(params) =
                        serde_json::from_str::<serde_json::Value>(args.request.params.get())
                    else {
                        continue;
                    };
                    let update = params
                        .get("update")
                        .and_then(|value| value.get("sessionUpdate"));
                    if update.and_then(serde_json::Value::as_str) == Some("memory_flush_completed")
                    {
                        return params;
                    }
                }
                panic!("gateway closed before MemoryFlushCompleted");
            })
            .await
            .expect("flush must report its durable cancellation failure");
            assert_eq!(
                result["update"]["result"],
                "skipped: memory was disabled before flush provider admission"
            );
            let loaded = history.load_session_without_updates(&info).await.unwrap();
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
            let ledgers = history
                .read_sideband_ledgers_sync(&info, &loaded.timeline)
                .unwrap();
            let events = ledgers.get(&spawn.sideband_id).unwrap_or_else(|| {
                assert_eq!(actor.session_info.id, info.id);
                assert_eq!(actor.session_info.cwd, info.cwd);
                let session_dir = history
                    .open_session(&info)
                    .unwrap()
                    .directory_handle()
                    .display_path()
                    .to_path_buf();
                let sideband_path = session_dir
                    .join(crate::session::storage::SIDEBANDS_DIR)
                    .join(&spawn.sideband_id)
                    .join(crate::session::storage::TIMELINE_FILE);
                let contents = std::fs::read_to_string(&sideband_path).ok();
                let entries = std::fs::read_dir(&session_dir)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect::<Vec<_>>();
                panic!(
                    "missing cancelled Sideband ledger {} among {:?}; session={session_dir:?}, sideband_path={sideband_path:?}, contents={contents:?}, entries={entries:?}",
                    spawn.sideband_id,
                    ledgers.keys(),
                )
            });
            let session_dir = history
                .open_session(&info)
                .unwrap()
                .directory_handle()
                .display_path()
                .to_path_buf();
            let sideband_path = session_dir
                .join(crate::session::storage::SIDEBANDS_DIR)
                .join(&spawn.sideband_id)
                .join(crate::session::storage::TIMELINE_FILE);
            assert!(sideband_path.exists());
            assert!(events.iter().any(|event| matches!(
                &event.kind,
                chat_state::SidebandEventKind::End(outcome)
                    if outcome.outcome == chat_state::SidebandOutcome::Cancelled
            )));

            assert!(server.requests().is_empty());
            assert!(!sessions_dir.exists());
        }));
    });
}

#[test]
fn memory_off_after_provider_acceptance_prevents_flush_write_and_storage_resurrection() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;
            use test_support::{InferenceEndpoint, InferenceRequestMatcher};

            let server = MockInferenceServer::start().await.unwrap();
            let mut provider = server.expect_response_blocked(
                "accepted memory flush response",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn(
                    "## Evidence\nThe accepted response must not be written.",
                    "test-model",
                ),
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
            let (actor, mut gateway, _) = actor_with_sampler_and_storage(
                &server,
                Some("vision"),
                Some(history.clone()),
                None,
            )
            .await;
            let memory_root = root.path().join("memory");
            let sessions_dir = install_memory_storage(&actor, &memory_root);
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::user("source"),
                    ConversationItem::assistant("reply"),
                ],
            )
            .await;

            let flushing_actor = actor.clone();
            let flush = tokio::task::spawn_local(async move {
                flushing_actor
                    .run_memory_flush("runtime-disabled-after-provider", None)
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(10), provider.wait_blocked())
                .await
                .expect("the flush request must reach and wait at the provider");
            assert_eq!(server.requests().len(), 1);
            assert!(server.requests()[0].body.is_some());

            turn_memory_off(&actor).await;
            provider.release();
            assert!(!flush.await.unwrap());

            let flush_result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while let Some(message) = gateway.recv().await {
                    let acp_transport::AcpClientMessage::ExtNotification(args) = &message else {
                        continue;
                    };
                    if args.request.method.as_ref() != "grow/session_notification" {
                        continue;
                    }
                    let Ok(params) =
                        serde_json::from_str::<serde_json::Value>(args.request.params.get())
                    else {
                        continue;
                    };
                    let Some(update) = params.get("update") else {
                        continue;
                    };
                    if update
                        .get("sessionUpdate")
                        .and_then(serde_json::Value::as_str)
                        == Some("memory_flush_completed")
                    {
                        return update
                            .get("result")
                            .and_then(serde_json::Value::as_str)
                            .unwrap()
                            .to_owned();
                    }
                }
                panic!("gateway closed before MemoryFlushCompleted");
            })
            .await
            .expect("flush completion must be reported");
            assert_eq!(flush_result, "storage not configured");

            assert_eq!(server.requests().len(), 1);
            assert!(!sessions_dir.exists());
            assert!(actor.memory.storage.borrow().is_none());
            assert!(!memory_root.exists());
            let loaded = history.load_session_without_updates(&info).await.unwrap();
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
            let ledgers = history
                .read_sideband_ledgers_sync(&info, &loaded.timeline)
                .unwrap();
            assert!(ledgers[&spawn.sideband_id].iter().any(|event| matches!(
                &event.kind,
                chat_state::SidebandEventKind::End(outcome)
                    if outcome.outcome == chat_state::SidebandOutcome::Completed
            )));
        }));
    });
}

#[test]
fn memory_off_before_compaction_keeps_preflush_from_sending_a_request() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("Compacted history.");
            let (actor, _gateway, _) = actor_with_sampler_and_storage_configured(
                &server,
                Some("vision"),
                None,
                None,
                Some((0, 0)),
            )
            .await;
            let root = tempfile::tempdir().unwrap();
            let sessions_dir = install_memory_storage(&actor, root.path());
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::user("source"),
                    ConversationItem::assistant("reply"),
                ],
            )
            .await;
            turn_memory_off(&actor).await;
            assert!(actor.run_compact(None).await.is_err());

            assert!(server.requests().iter().all(|request| {
                request.body.as_ref().is_none_or(|body| {
                    !body
                        .to_string()
                        .contains(crate::session::helpers::memory_flush::FLUSH_SYSTEM_PROMPT)
                })
            }));
            assert!(!sessions_dir.exists());
        }));
    });
}

#[test]
fn memory_off_before_background_preflush_trigger_skips_memory_request() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("Background compact summary.");
            let (actor, _gateway, _) = actor_with_sampler_and_storage_configured(
                &server,
                Some("vision"),
                None,
                None,
                Some((85, 0)),
            )
            .await;
            let root = tempfile::tempdir().unwrap();
            install_memory_storage(&actor, root.path());
            let mut route = actor.chat_state_handle.get_sampling_config().await.unwrap();
            route.context_window = std::num::NonZeroU64::new(100_000).unwrap();
            actor.chat_state_handle.replace_sampling_route(route);
            replace_test_surface(
                &actor.chat_state_handle,
                vec![ConversationItem::system("test system prompt")],
            )
            .await;
            let mut old_prompt =
                ConversationItem::user(format!("old closed turn. {}", "Old details. ".repeat(600)));
            old_prompt.set_prompt_index(0);
            old_prompt.add_image(inline_data_uri(&test_image_content()));
            actor.chat_state_handle.push_user_message(old_prompt);
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("x".repeat(200_000)));
            let mut recent_prompt = ConversationItem::user("recent retained turn");
            recent_prompt.set_prompt_index(1);
            actor.chat_state_handle.push_user_message(recent_prompt);
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("y".repeat(88_000)));
            actor
                .chat_state_handle
                .record_provider_context_anchor(76_000);
            assert!(actor.chat_state_handle.get_projected_tokens().await >= 75_000);
            actor.compaction.pre_prune.set(false);
            turn_memory_off(&actor).await;
            crate::session::actor::tests::support::begin_test_active_causal_turn(&actor).await;
            assert!(actor.events.end_step("continued"));

            actor.background_compaction_boundary().await.unwrap();

            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while server.requests().is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(server.requests().iter().all(|request| {
                request.body.as_ref().is_none_or(|body| {
                    !body
                        .to_string()
                        .contains(crate::session::helpers::memory_flush::FLUSH_SYSTEM_PROMPT)
                })
            }));
        }));
    });
}

#[test]
fn memory_off_before_error_recovery_preflush_trigger_skips_memory_request() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("Error recovery compact summary.");
            let (actor, _gateway, _) = actor_with_sampler_and_storage_configured(
                &server,
                Some("vision"),
                None,
                None,
                Some((85, 0)),
            )
            .await;
            let root = tempfile::tempdir().unwrap();
            install_memory_storage(&actor, root.path());
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::user("source"),
                    ConversationItem::assistant("reply"),
                ],
            )
            .await;
            turn_memory_off(&actor).await;

            let _ = actor
                .run_compact_only(crate::session::actor::compaction::AutoCompactTriggerInfo {
                    tokens_used: 90_000,
                    context_window: 100_000,
                    percentage: 90,
                    source: "sampler_error_recovery",
                    max_output_tokens: None,
                })
                .await;

            assert!(server.requests().iter().all(|request| {
                request.body.as_ref().is_none_or(|body| {
                    !body
                        .to_string()
                        .contains(crate::session::helpers::memory_flush::FLUSH_SYSTEM_PROMPT)
                })
            }));
        }));
    });
}
