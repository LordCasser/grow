use super::super::SessionActor;
use super::support::build_actor;

#[tokio::test(flavor = "current_thread")]
async fn late_observation_after_unknown_result_preserves_the_original_call_on_resume() {
    use chat_state::session_runtime::SessionRuntimeEvent;
    use chat_state::{NotificationSource, NotificationSourceVersion, TimelineEventKind};
    use sampling_types::{ConversationItem, ToolCall};

    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor
                .chat_state_handle
                .push_response_durably(vec![ConversationItem::assistant_tool_calls(vec![
                    ToolCall {
                        id: "remote-write-call".into(),
                        name: "remote_write".into(),
                        arguments: "{\"operation\":\"remote-write-once\"}".into(),
                    },
                ])], None)
                .await
                .unwrap();
            actor
                .chat_state_handle
                .push_tool_result_durably(ConversationItem::tool_result(
                    "remote-write-call",
                    "OutcomeUnknown: inspect remote-write-once before any new write",
                ))
                .await
                .unwrap();
            actor.state.lock().await.runtime_suspend.store(true, std::sync::atomic::Ordering::Release);
            for event in [
                SessionRuntimeEvent::SuspendRequested {
                    operation_id: "unknown-suspend".into(),
                    root_session_id: actor.session_id_string(),
                },
                SessionRuntimeEvent::CommandsSettled {
                    operation_id: "unknown-suspend".into(),
                    commands: vec![],
                },
            ] {
                actor.chat_state_handle.record_timeline_event_durably(TimelineEventKind::Runtime(event)).await.unwrap();
            }
            actor.chat_state_handle.checkpoint_runtime(
                "unknown-suspend".into(),
                actor.runtime_source_hash().await.unwrap(),
                None,
                None,
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            ).await.unwrap();
            let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();
            actor.prepare_runtime_restore("unknown-suspend", "unknown-instance", completion_tx).await.unwrap();
            actor.commit_runtime_resume("unknown-suspend".into(), "unknown-instance".into()).await.unwrap();
            actor.release_runtime_restore("unknown-instance").await.unwrap();
            assert!(actor.state.lock().await.execution_open());

            // A verifier's later observation is a new attributable inbox fact,
            // never a replacement tool_result for the original uncertain call.
            let source = NotificationSource::ParentMessage {
                parent_session_id: "verifier-session".into(),
                message_id: "verified-remote-write-call".into(),
                interrupt: false,
            };
            let version = NotificationSourceVersion::Ordinal { value: chat_state::PARENT_MESSAGE_SOURCE_VERSION };
            let body = "Verified remote-write-call / remote-write-once: the server committed exactly one write.";
            let receipt = actor.receive_notification(source.clone(), version.clone(), body.into()).await.unwrap();
            assert_eq!(actor.receive_notification(source, version, body.into()).await.unwrap(), receipt);
            let pending = actor.chat_state_handle.pending_notifications().await.unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(crate::session::notification_inbox::read_payload(&actor.session_directory, &pending[0].payload_ref).unwrap(), body);
            let surface = actor.chat_state_handle.get_conversation().await;
            let results = surface.iter().filter(|item| matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == "remote-write-call")).collect::<Vec<_>>();
            assert_eq!(results.len(), 1);
            assert!(results[0].text_content().contains("OutcomeUnknown"));
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(events.iter().filter(|event| matches!(event.kind,
                TimelineEventKind::Notification(chat_state::NotificationEvent::Received { .. })
            )).count(), 1);
            assert!(!events.iter().any(|event| matches!(event.kind, TimelineEventKind::Request(_))));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_source_hash_is_64_hex() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            let hash = actor
                .runtime_source_hash()
                .await
                .expect("runtime source hash");
            assert_eq!(hash.len(), 64);
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn restore_source_drift_and_foreign_operation_leave_the_checkpoint_closed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            let node = request_and_checkpoint(&actor, "original-operation").await;
            let before = actor.chat_state_handle.timeline_events().await.unwrap();
            let (completion_tx, mut completion_rx) = tokio::sync::mpsc::unbounded_channel();

            let foreign = actor
                .prepare_runtime_restore("another-operation", "instance", completion_tx.clone())
                .await
                .unwrap_err();
            assert!(foreign.contains("does not own"), "{foreign}");
            let drift = actor
                .prepare_runtime_restore("original-operation", "instance", completion_tx)
                .await
                .unwrap_err();
            assert!(drift.contains("changed"), "{drift}");

            let runtime = actor.chat_state_handle.runtime_state().await.unwrap();
            assert_eq!(
                runtime.phase,
                chat_state::session_runtime::RuntimePhase::Suspended
            );
            assert_eq!(runtime.checkpoint_seq, Some(node.checkpoint_seq));
            assert!(runtime.instance_id.is_none());
            assert!(!runtime.resume_committed);
            assert!(actor.restoration_release.borrow().is_none());
            assert!(actor.turn_continuation.borrow().is_none());
            assert!(completion_rx.try_recv().is_err());
            assert_eq!(
                serde_json::to_value(actor.chat_state_handle.timeline_events().await.unwrap())
                    .unwrap(),
                serde_json::to_value(before).unwrap()
            );
        })
        .await;
}

async fn request_and_checkpoint(
    actor: &SessionActor,
    operation_id: &str,
) -> chat_state::session_runtime::RuntimeNodeRef {
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
            chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                operation_id: operation_id.to_string(),
                root_session_id: actor.session_id_string(),
            },
        ))
        .await
        .expect("suspend request must pass Timeline validation");

    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
            chat_state::session_runtime::SessionRuntimeEvent::CommandsSettled {
                operation_id: operation_id.to_string(),
                commands: Vec::new(),
            },
        ))
        .await
        .expect("command settlement must pass Timeline validation");

    let checkpoint = actor
        .chat_state_handle
        .checkpoint_runtime(
            operation_id.to_string(),
            "a".repeat(64),
            None,
            None,
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
        .expect("checkpoint must pass Timeline validation");

    chat_state::session_runtime::RuntimeNodeRef {
        session_id: actor.session_id_string(),
        checkpoint_seq: checkpoint.seq,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn checkpoint_resolves_all_registered_waiters_once_and_suspended_state_is_immediate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            let mut receivers = Vec::new();

            for _ in 0..3 {
                let (sender, receiver) = tokio::sync::oneshot::channel();
                actor.handle_wait_runtime_suspended(sender).await;
                receivers.push(receiver);
            }
            assert_eq!(actor.runtime_suspended_waiters.borrow().len(), 3);

            // Both transitions are committed through ChatState's Timeline validator.
            let expected = request_and_checkpoint(&actor, "suspend-op").await;
            actor.resolve_runtime_suspended_waiters(expected.clone());
            assert!(actor.runtime_suspended_waiters.borrow().is_empty());
            for receiver in receivers {
                assert_eq!(receiver.await.expect("waiter reply"), Ok(expected.clone()));
            }

            // A duplicate notification cannot reply twice; a newly arriving waiter
            // reads the already committed checkpoint and receives its original seq.
            actor.resolve_runtime_suspended_waiters(expected.clone());
            let (sender, receiver) = tokio::sync::oneshot::channel();
            actor.handle_wait_runtime_suspended(sender).await;
            assert!(actor.runtime_suspended_waiters.borrow().is_empty());
            assert_eq!(
                receiver.await.expect("immediate reply"),
                Ok(expected.clone())
            );

            let runtime = actor.chat_state_handle.runtime_state().await.unwrap();
            assert_eq!(runtime.operation_id.as_deref(), Some("suspend-op"));
            assert_eq!(runtime.checkpoint_seq, Some(expected.checkpoint_seq));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_waiter_does_not_cancel_the_suspension_owner() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                    chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                        operation_id: "owned-suspend".into(),
                        root_session_id: actor.session_id_string(),
                    },
                ))
                .await
                .expect("suspend request must pass Timeline validation");

            let (sender, receiver) = tokio::sync::oneshot::channel();
            actor.handle_wait_runtime_suspended(sender).await;
            assert_eq!(actor.runtime_suspended_waiters.borrow().len(), 1);
            drop(receiver);

            let runtime = actor.chat_state_handle.runtime_state().await.unwrap();
            assert_eq!(
                runtime.phase,
                chat_state::session_runtime::RuntimePhase::Suspending
            );
            assert_eq!(runtime.operation_id.as_deref(), Some("owned-suspend"));

            // The owner can still commit its checkpoint after an initializer drops
            // its reply receiver; resolving the closed channel simply drains it.
            let node = request_and_checkpoint_existing(&actor, "owned-suspend").await;
            actor.resolve_runtime_suspended_waiters(node);
            assert!(actor.runtime_suspended_waiters.borrow().is_empty());
        })
        .await;
}

async fn request_and_checkpoint_existing(
    actor: &SessionActor,
    operation_id: &str,
) -> chat_state::session_runtime::RuntimeNodeRef {
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
            chat_state::session_runtime::SessionRuntimeEvent::CommandsSettled {
                operation_id: operation_id.to_string(),
                commands: Vec::new(),
            },
        ))
        .await
        .expect("command settlement must pass Timeline validation");
    let checkpoint = actor
        .chat_state_handle
        .checkpoint_runtime(
            operation_id.to_string(),
            "a".repeat(64),
            None,
            None,
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
        .expect("checkpoint must pass Timeline validation");
    chat_state::session_runtime::RuntimeNodeRef {
        session_id: actor.session_id_string(),
        checkpoint_seq: checkpoint.seq,
    }
}
