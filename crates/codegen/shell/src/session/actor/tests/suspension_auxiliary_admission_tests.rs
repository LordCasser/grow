use super::support::*;
use super::*;

fn with_session_runtime<F: std::future::Future<Output = ()>>(test: impl FnOnce() -> F + Send) {
    // Session actors use a larger stack than the default test thread.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(tokio::task::LocalSet::new().run_until(test()));
            })
            .unwrap()
            .join()
            .unwrap();
    });
}

#[test]
fn suspension_closes_auxiliary_admission_before_its_worker_runs() {
    with_session_runtime(|| async {
        let (actor, _gateway) = build_actor().await;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        actor
            .request_node_suspension(
                "early-close".into(),
                actor.session_id_string(),
                tokio::time::Instant::now() + Duration::from_secs(5),
                respond_to,
            )
            .await;
        // spawn_local does not synchronously poll the suspension worker.
        // Admission must already be closed before its first durable append.
        assert!(actor.session_activities.try_start("late-command").is_none());
        drop(response);
        actor
            .runtime_suspension_worker
            .abort_and_join()
            .await
            .unwrap();
    });
}

#[test]
fn expired_suspension_admission_keeps_the_healthy_writer_available() {
    with_session_runtime(|| async {
        let (actor, _gateway) = build_actor().await;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        actor
            .request_node_suspension(
                "expired-request".into(),
                actor.session_id_string(),
                tokio::time::Instant::now() - Duration::from_secs(1),
                respond_to,
            )
            .await;
        assert!(response.await.unwrap().is_err());
        assert!(actor.chat_state_handle.runtime_state().await.is_ok());
        assert!(actor.chat_state_handle.pending_timeline_commit().is_none());
        assert!(!actor.runtime_suspension_worker.is_running());
    });
}

#[test]
fn late_suggestions_during_suspension_do_not_panic_or_bypass_owner_drain() {
    with_session_runtime(|| async {
        let (gateway, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
        let (persistence, mut persisted) = tokio::sync::mpsc::unbounded_channel();
        tokio::task::spawn_local(async move {
            while let Some(message) = persisted.recv().await {
                if let PersistenceMsg::FlushAndAck { respond_to } = message {
                    let _ = respond_to.send(());
                }
            }
        });
        let (mut actor, events) = create_test_actor_ex(0, 256_000, 85, gateway, persistence).await;
        actor.startup_hints.is_subagent = true;
        let actor = Arc::new(actor);
        // A previously admitted owner keeps the real suspension incomplete.
        let existing = actor
            .session_activities
            .try_start("existing-owner")
            .unwrap();
        let (_chat_events, chat_rx) = tokio::sync::mpsc::unbounded_channel();
        let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let running = actor.clone();
        let loop_task = tokio::task::spawn_local(async move {
            let root = running.session_dir.clone();
            super::super::run_loop::run_session(
                running,
                command_rx,
                chat_rx,
                events,
                None,
                Arc::new(parking_lot::Mutex::new(CodebaseIndexManager::default())),
                root,
                Default::default(),
            )
            .await;
        });
        let (respond_to, mut suspended) = tokio::sync::oneshot::channel();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        commands
            .send(SessionCommand::SuspendRuntimeNode {
                operation_id: "late-suggestion".into(),
                root_session_id: actor.session_id_string(),
                deadline,
                respond_to,
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !actor.sideband_cancel.is_cancelled() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let (respond_to, mut waiting) = tokio::sync::oneshot::channel();
        commands
            .send(SessionCommand::WaitRuntimeSuspended { respond_to })
            .unwrap();

        for prompt in [false, true] {
            let (respond_to, response) = tokio::sync::oneshot::channel();
            commands
                .send(if prompt {
                    SessionCommand::SuggestPrompt {
                        model_override: None,
                        respond_to,
                    }
                } else {
                    SessionCommand::AISuggest {
                        prefix: "echo ".into(),
                        cwd: actor.session_dir.to_string_lossy().into_owned(),
                        model_override: None,
                        respond_to,
                    }
                })
                .unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), response)
                    .await
                    .expect("late suggestion must settle promptly")
                    .expect("closed admission is a normal no-suggestion response"),
                None
            );
        }
        let (responds_to, observed) = tokio::sync::oneshot::channel();
        commands
            .send(SessionCommand::GetActiveAgent { responds_to })
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), observed)
                .await
                .unwrap()
                .unwrap()
                .is_some()
        );
        assert!(
            !loop_task.is_finished(),
            "mailbox must remain alive for lifecycle settlement"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut suspended)
                .await
                .is_err()
        );
        assert_ne!(
            actor.chat_state_handle.runtime_state().await.unwrap().phase,
            chat_state::session_runtime::RuntimePhase::Suspended
        );
        assert!(
            !actor
                .chat_state_handle
                .timeline_events()
                .await
                .unwrap()
                .iter()
                .any(|event| matches!(event.kind, chat_state::TimelineEventKind::Sideband(_))),
            "rejected suggestions must not start model Sidebands"
        );

        // Expiry is not permission to discard an admitted owner. Keep the
        // exact original response and waiter while the caller times out.
        assert!(
            tokio::time::timeout_at(deadline, &mut suspended)
                .await
                .is_err()
        );
        assert!(matches!(
            waiting.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(!loop_task.is_finished());

        drop(existing);
        let _original = tokio::time::timeout(Duration::from_secs(5), suspended)
            .await
            .unwrap()
            .unwrap();
        // Retry the same operation after the old physical owner has drained;
        // it may observe a late checkpoint or finish the original prefix.
        let (respond_to, response) = tokio::sync::oneshot::channel();
        commands
            .send(SessionCommand::SuspendRuntimeNode {
                operation_id: "late-suggestion".into(),
                root_session_id: actor.session_id_string(),
                deadline: tokio::time::Instant::now() + Duration::from_secs(5),
                respond_to,
            })
            .unwrap();
        let node = tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let waiter = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(waiter.checkpoint_seq, node.checkpoint_seq);
        assert_eq!(node.session_id, actor.session_id_string());
        assert_eq!(
            actor.chat_state_handle.runtime_state().await.unwrap().phase,
            chat_state::session_runtime::RuntimePhase::Suspended
        );
        let (respond_to, response) = tokio::sync::oneshot::channel();
        commands
            .send(SessionCommand::RetireSuspendedRuntime {
                operation_id: "late-suggestion".into(),
                deadline: tokio::time::Instant::now() + Duration::from_secs(5),
                respond_to,
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), loop_task)
            .await
            .unwrap()
            .unwrap();
    });
}
