// Included temporarily inside session::actor::suspension for the audit run.
use super::*;
use crate::session::actor::tests::support::{begin_test_causal_turn, build_actor};
use chat_state::session_runtime::TurnContinuation;
use chat_state::{NotificationSource, NotificationSourceVersion, TimelineEventKind as K};

fn run(
    test: impl FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> + Send + 'static,
) {
    std::thread::Builder::new()
        .stack_size(48 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(tokio::task::LocalSet::new().run_until(test()));
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn seed_settlement(actor: &SessionActor, commands: Vec<RuntimeCommandSettlement>) {
    for event in [
        SessionRuntimeEvent::SuspendRequested {
            operation_id: "audit-op".into(),
            root_session_id: actor.session_id_string(),
        },
        SessionRuntimeEvent::CommandsSettled {
            operation_id: "audit-op".into(),
            commands,
        },
    ] {
        actor
            .chat_state_handle
            .record_timeline_event_durably(K::Runtime(event))
            .await
            .unwrap();
    }
}

#[test]
fn audit_existing_completion_blocks_suspension_and_retry() {
    run(|| {
        Box::pin(async {
            let (actor, _gateway) = build_actor().await;
            let snapshot = tools::computer::types::TaskSnapshot {
                task_id: "audit-bg".into(),
                command: "printf done".into(),
                display_command: None,
                cwd: "/tmp".into(),
                start_time: std::time::UNIX_EPOCH,
                end_time: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
                output: "done".into(),
                output_file: "/tmp/audit-bg.txt".into(),
                truncated: false,
                exit_code: Some(0),
                signal: None,
                completed: true,
                kind: tools::computer::types::TaskKind::Bash,
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: Some(actor.session_id_string()),
                goal_id: None,
                goal_definition_revision: None,
                description: None,
                is_backgrounded: true,
            };
            let source = NotificationSource::TaskCompleted {
                task_id: snapshot.task_id.clone(),
                task_kind: chat_state::NotificationTaskKind::Task,
                owner: chat_state::NotificationOwner::Session,
            };
            let normal_body =
                tools::reminders::task_completion::format_bash_completion(&snapshot, None, None);
            actor
                .receive_notification(
                    source.clone(),
                    NotificationSourceVersion::Ordinal { value: 1 },
                    normal_body.clone(),
                )
                .await
                .unwrap();
            seed_settlement(
                &actor,
                vec![RuntimeCommandSettlement {
                    command_id: snapshot.task_id,
                    call_id: "audit-bg-call".into(),
                    outcome: CommandSettlementStatus::Completed,
                    output_file: snapshot.output_file.to_string_lossy().into_owned(),
                    exit_code: snapshot.exit_code,
                    signal: snapshot.signal,
                    output: snapshot.output,
                    is_backgrounded: true,
                    owner: chat_state::NotificationOwner::Session,
                    task_kind: chat_state::NotificationTaskKind::Task,
                }],
            )
            .await;
            for attempt in 1..=2 {
                let error = actor
                    .suspend_node(
                        "audit-op".into(),
                        actor.session_id_string(),
                        tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                    )
                    .await
                    .expect_err("current code must reproduce the receipt conflict");
                eprintln!("audit receipt conflict attempt {attempt}: {error}");
                let runtime = actor.chat_state_handle.runtime_state().await.unwrap();
                assert_eq!(runtime.phase, RuntimePhase::Suspending);
                assert!(runtime.checkpoint_seq.is_none());
            }
            let pending = actor
                .chat_state_handle
                .pending_notifications()
                .await
                .unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(
                crate::session::notification_inbox::read_payload(
                    &actor.session_directory,
                    &pending[0].payload_ref
                )
                .unwrap(),
                normal_body
            );
        })
    });
}

#[test]
fn audit_retry_loses_permission_wait_after_owner_drain_timeout() {
    run(|| {
        Box::pin(async {
            let (actor, _gateway) = build_actor().await;
            begin_test_causal_turn(&actor).await;
            actor
                .chat_state_handle
                .push_response_durably(
                    vec![ConversationItem::assistant_tool_calls(vec![
                        sampling_types::ToolCall {
                            id: "audit-permission-call".into(),
                            name: "run".into(),
                            arguments: "{}".into(),
                        },
                    ])],
                    None,
                )
                .await
                .unwrap();
            actor
                .events
                .tool_started(
                    "run".into(),
                    "audit-permission-call".into(),
                    Some(serde_json::json!({})),
                )
                .await
                .unwrap();
            let turn_id = actor.events.current_turn().unwrap();
            *actor.turn_continuation.borrow_mut() = Some(TurnContinuation {
                turn_id,
                prompt_id: "fixture-prompt".into(),
                prompt_index: 0,
                origin: "test".into(),
                next_step_index: 1,
                tool_turns_used: 1,
                tool_calls_used: 1,
                stop_continuations_used: 0,
                output_schema: None,
                resume_point: TurnResumePoint::Model,
                task_output_usage: None,
                accepted_response: None,
                structured_output_retries: 0,
                sampling_attempts_used: 0,
                sampling_max_attempts: None,
                sampling_replay_safe: true,
                sampling_deadline_ms: None,
                operations: vec![],
            });
            let mut guard = crate::session::pending_interaction::PendingInteractionGuard::new(
                actor.pending_interactions.clone(),
                actor.notifications.gateway.clone(),
                actor.session_info.id.clone(),
                "audit-permission-call".into(),
                crate::session::pending_interaction::PendingKind::Permission,
            );
            guard.open_durably(&actor.chat_state_handle).await.unwrap();
            let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
            let worker = tokio::task::spawn_local(async move {
                let _guard = guard;
                let _stopped = stopped_tx;
                std::future::pending::<()>().await;
            });
            actor.state.lock().await.foreground = ForegroundState::RegularTurn(AgentTask {
                stopped: futures_util::FutureExt::shared(stopped_rx),
                prompt_id: "fixture-prompt".into(),
                origin: crate::session::PromptOrigin::User,
                turn_kind: crate::session::TurnKind::User,
                turn_start_ms: 0,
                usage_epoch: 0,
                handle: worker.abort_handle(),
                steering_open: true,
            });
            // Existing auxiliary work may legitimately finish after the caller's deadline.
            let lease = actor
                .session_activities
                .try_start("audit-slow-owner")
                .unwrap();
            seed_settlement(&actor, vec![]).await;
            let first_actor = actor.clone();
            let first = tokio::task::spawn_local(async move {
                first_actor
                    .suspend_node(
                        "audit-op".into(),
                        first_actor.session_id_string(),
                        tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                    )
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !actor.background_service_shutdown.is_cancelled() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            drop(lease);
            let first_error = first
                .await
                .unwrap()
                .expect_err("drain must outlive the first deadline");
            assert!(
                first_error.contains("owners settled after the deadline"),
                "{first_error}"
            );
            assert!(actor.pending_interactions.lock().unwrap().is_empty());
            assert!(!actor.chat_state_handle.get_conversation().await.iter().any(|item| matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == "audit-permission-call")));
            let second = actor
                .suspend_node(
                    "audit-op".into(),
                    actor.session_id_string(),
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await;
            eprintln!("audit permission first={first_error}; retry={second:?}");
            let surface = actor.chat_state_handle.get_conversation().await;
            let result = surface.iter().find(|item| matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == "audit-permission-call"))
            .expect("current retry incorrectly manufactures a ToolResult for an unanswered permission");
            assert!(
                result.text_content().contains("OutcomeUnknown"),
                "{}",
                result.text_content()
            );
            eprintln!("audit permission result: {}", result.text_content());
            if let Some(checkpoint) = actor
                .chat_state_handle
                .runtime_state()
                .await
                .unwrap()
                .checkpoint
            {
                eprintln!(
                    "audit permission checkpoint operations: {:?}",
                    checkpoint.turn.unwrap().operations
                );
            }
        })
    });
}
