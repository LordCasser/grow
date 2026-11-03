use super::support::*;
use super::*;
use test_support::{MockInferenceServer, ScriptedResponse};

fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

async fn idle_flush_actor(
    server: &MockInferenceServer,
    timeout: std::time::Duration,
) -> (
    std::sync::Arc<SessionActor>,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
    tokio::sync::mpsc::UnboundedSender<SessionCommand>,
    tokio::sync::mpsc::UnboundedSender<chat_state::ChatStateEvent>,
) {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut actor, event_rx) =
        create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            match message {
                PersistenceMsg::SidebandDurablyAndAck { respond_to, .. }
                | PersistenceMsg::ReplaceRewindPointsAndAck { respond_to, .. }
                | PersistenceMsg::WriteRewindTransactionAndAck { respond_to, .. }
                | PersistenceMsg::ClearRewindTransactionAndAck { respond_to } => {
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. }
                | PersistenceMsg::AppendUpdateBufferedAndAck { respond_to, .. } => {
                    let _ = respond_to.send(Ok(()));
                }
                _ => {}
            }
        }
    });

    let mut route = actor.chat_state_handle.get_sampling_config().await.unwrap();
    route.base_url = server.url();
    route.api_backend = sampling_types::ApiBackend::Messages;
    actor.chat_state_handle.replace_sampling_route(route);
    let (sampler_event_tx, mut sampler_event_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.sampler_handle = sampler::SamplerActor::spawn(
        sampler::SamplerConfig {
            api_key: Some("loopback".into()),
            base_url: server.url(),
            model: "test-model".into(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: sampling_types::ApiBackend::Messages,
            auth_scheme: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: 256_000,
            force_http1: false,
            max_retries: Some(0),
            stream_tool_calls: false,
            idle_timeout_secs: Some(30),
            reasoning_effort: None,
            origin_client: None,
            bearer_resolver: None,
            compactions_remaining: None,
            compaction_at_tokens: None,
            doom_loop_recovery: None,
        },
        sampler::RetryPolicy::default(),
        sampler_event_tx,
    );
    let memory_root = tempfile::tempdir().unwrap();
    *actor.memory.storage.borrow_mut() = Some(memory::MemoryStorage::new_flat(
        memory_root.path(),
        memory_root.path(),
    ));
    actor.idle_flush_timeout = Some(timeout);
    let user_prompt = |text: &str, index: usize| {
        let mut item = crate::sampling::ConversationItem::user(text);
        item.set_prompt_index(index);
        item
    };
    seed_test_timeline(
        &actor,
        vec![
            user_prompt("inherited fork context one", 0),
            crate::sampling::ConversationItem::assistant("inherited fork answer one"),
            user_prompt("inherited fork context two", 1),
            crate::sampling::ConversationItem::assistant("inherited fork answer two"),
            user_prompt("inherited fork context three", 2),
            crate::sampling::ConversationItem::assistant("inherited fork answer three"),
        ],
        &[
            "inherited fork context one",
            "inherited fork context two",
            "inherited fork context three",
        ],
    )
    .await;
    let fork_baseline = actor.chat_state_handle.get_conversation_len().await;
    actor
        .last_idle_flush_conversation_len
        .store(fork_baseline, std::sync::atomic::Ordering::Relaxed);
    actor
        .chat_state_handle
        .push_user_message(crate::sampling::ConversationItem::user(
            "remember this idle flush fact",
        ));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while actor.chat_state_handle.get_conversation_len().await == fork_baseline {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("new post-fork input reaches the Timeline surface");

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (chat_event_tx, chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let run_actor = std::sync::Arc::new(actor);
    let task_actor = run_actor.clone();
    let event_actor = run_actor.clone();
    tokio::task::spawn_local(async move {
        while let Some(event) = sampler_event_rx.recv().await {
            event_actor.handle_sampling_event(event).await;
        }
    });
    let task = tokio::task::spawn_local(async move {
        super::super::run_loop::run_session(
            task_actor,
            cmd_rx,
            chat_event_rx,
            event_rx,
            None,
            std::sync::Arc::new(parking_lot::Mutex::new(
                workspace::file_system::CodebaseIndexManager::new(),
            )),
            std::env::temp_dir(),
            crate::session::fs_watch::FsWatchCapabilities::none(),
        )
        .await;
    });
    (run_actor, task, memory_root, cmd_tx, chat_event_tx)
}

#[test]
fn idle_flush_waits_for_activity_and_retries_provider_failure() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                ScriptedResponse::json(500, serde_json::json!({"error":"mock failure"})),
            );
            server.set_response("## Memory\nKeep this fact.");
            let (actor, task, _memory_root, commands, chat_events) =
                idle_flush_actor(&server, std::time::Duration::from_millis(100)).await;
            let fork_baseline = actor
                .last_idle_flush_conversation_len
                .load(std::sync::atomic::Ordering::Relaxed);

            actor.state.lock().await.foreground = ForegroundState::Compaction;
            tokio::time::sleep(std::time::Duration::from_millis(140)).await;
            assert_eq!(
                server.messages_request_count(),
                0,
                "a busy foreground must not flush"
            );
            actor.state.lock().await.foreground = ForegroundState::Idle;
            actor.idle_arbiter.notify_one();

            let activity = actor
                .session_activities
                .try_start("test_busy_activity")
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(140)).await;
            assert_eq!(
                server.messages_request_count(),
                0,
                "an active session must not flush"
            );
            drop(activity);

            let query_commands = commands.clone();
            let query_task = tokio::task::spawn_local(async move {
                loop {
                    let (respond_to, response) = tokio::sync::oneshot::channel();
                    if query_commands
                        .send(SessionCommand::GetCurrentModel {
                            responds_to: respond_to,
                        })
                        .is_err()
                    {
                        break;
                    }
                    let _ = response.await;
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            });

            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .memory
                    .flush_error_count
                    .load(std::sync::atomic::Ordering::Relaxed)
                    == 0
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the first idle provider request should fail");
            query_task.abort();
            assert_eq!(
                actor
                    .last_idle_flush_conversation_len
                    .load(std::sync::atomic::Ordering::Relaxed),
                fork_baseline,
                "provider failure must not advance the successful checkpoint"
            );
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while server.messages_request_count() < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("failed flush should be retried after another idle interval");
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .memory
                    .flush_success_count
                    .load(std::sync::atomic::Ordering::Relaxed)
                    == 0
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("retried flush must complete successfully before branch changes");
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .last_idle_flush_conversation_len
                    .load(std::sync::atomic::Ordering::Relaxed)
                    <= fork_baseline
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("successful retry should checkpoint the frozen history");
            let bodies = server.request_bodies();
            assert!(
                bodies
                    .last()
                    .unwrap()
                    .to_string()
                    .contains("remember this idle flush fact")
            );
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            assert_eq!(
                server.messages_request_count(),
                2,
                "successful checkpoint should suppress duplicate flushes"
            );

            // `rewind_durably` and completed compaction both publish this
            // reset event. Rebase the existing length checkpoint before a
            // shorter branch receives new user input.
            let (respond_to, rewind_result) = tokio::sync::oneshot::channel();
            commands
                .send(SessionCommand::Rewind {
                    request: crate::session::RewindRequest {
                        target_prompt_index: 1,
                        force: true,
                        mode: crate::session::RewindMode::ConversationOnly,
                    },
                    respond_to,
                })
                .unwrap();
            let rewind = tokio::time::timeout(std::time::Duration::from_secs(5), rewind_result)
                .await
                .expect("run loop should process rewind command")
                .unwrap()
                .expect("actual rewind transaction should complete");
            assert!(rewind.success);
            let rewind_len = actor.chat_state_handle.get_conversation_len().await;
            let previous_checkpoint = actor
                .last_idle_flush_conversation_len
                .load(std::sync::atomic::Ordering::Relaxed);
            assert!(rewind_len + 1 < previous_checkpoint);
            chat_events
                .send(chat_state::ChatStateEvent::ConversationReset {
                    new_len: rewind_len,
                })
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .last_idle_flush_conversation_len
                    .load(std::sync::atomic::Ordering::Relaxed)
                    != rewind_len
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("run loop should apply rewind branch reset");
            actor
                .chat_state_handle
                .push_user_message(crate::sampling::ConversationItem::user(
                    "remember the new rewind branch",
                ));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while server.messages_request_count() < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("rewind reset should make new shorter-branch input eligible");
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .memory
                    .flush_success_count
                    .load(std::sync::atomic::Ordering::Relaxed)
                    < 2
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("rewound branch flush should settle before compaction");

            // Compaction publishes a reset even when replacement context has
            // the same length. Model a stale checkpoint from the prior branch
            // and deliver the actual reset event to the run loop.
            let compacted_len = actor.chat_state_handle.get_conversation_len().await;
            actor
                .last_idle_flush_conversation_len
                .store(compacted_len + 20, std::sync::atomic::Ordering::Relaxed);
            chat_events
                .send(chat_state::ChatStateEvent::ConversationReset {
                    new_len: compacted_len,
                })
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .last_idle_flush_conversation_len
                    .load(std::sync::atomic::Ordering::Relaxed)
                    != compacted_len
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("run loop should apply compacted branch reset");
            actor
                .chat_state_handle
                .push_user_message(crate::sampling::ConversationItem::user(
                    "remember after compaction",
                ));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while server.messages_request_count() < 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("compaction reset should allow the next same-branch input");
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while actor
                    .memory
                    .flush_success_count
                    .load(std::sync::atomic::Ordering::Relaxed)
                    < 3
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("post-compaction flush should checkpoint the new context");
            task.abort();
        }));
    });
}

#[test]
fn zero_idle_flush_timeout_does_not_call_provider() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (_actor, task, _memory_root, _commands, _chat_events) =
                idle_flush_actor(&server, std::time::Duration::ZERO).await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            assert_eq!(server.messages_request_count(), 0);
            task.abort();
        }));
    });
}
