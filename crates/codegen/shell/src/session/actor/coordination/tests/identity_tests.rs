use super::*;

#[tokio::test(flavor = "current_thread")]
async fn same_id_receipts_from_two_sessions_survive_independent_recovery() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _) = crate::session::actor::tests::support::build_actor().await;
            let (first, _) = inquiry(1);
            let (mut second, _) = inquiry(2);
            second.inquiry_id = first.inquiry_id.clone();
            actor
                .record_incoming_coordination_notice(&first)
                .await
                .unwrap();
            actor
                .record_incoming_coordination_notice(&second)
                .await
                .unwrap();
            assert_eq!(actor.pending_coordination_notices().await.unwrap().len(), 2);
            actor
                .recover_interrupted_coordination_inquiries()
                .await
                .unwrap();
            assert!(
                actor
                    .pending_coordination_notices()
                    .await
                    .unwrap()
                    .is_empty()
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let completed: Vec<_> = events
                .iter()
                .filter_map(|event| {
                    match crate::coordination::InquiryEvent::from_timeline(event).unwrap() {
                        Some(crate::coordination::InquiryEvent::Incoming { audit, .. })
                            if audit.outcome.is_some() =>
                        {
                            Some(audit.source_session_id)
                        }
                        _ => None,
                    }
                })
                .collect();
            assert_eq!(
                completed,
                vec![first.source_session_id, second.source_session_id]
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cold_target_reuses_terminal_or_interrupted_receipt_without_a_new_sideband() {
    use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
    tokio::task::LocalSet::new()
        .run_until(async {
            for completed in [false, true] {
                let home = tempfile::tempdir().unwrap();
                let (actor, _) = crate::session::actor::tests::support::build_actor().await;
                let (original, _) = inquiry(1);
                actor
                    .record_incoming_coordination_notice(&original)
                    .await
                    .unwrap();
                if completed {
                    actor
                        .record_coordination_terminal_notice(
                            &original,
                            &crate::coordination::InquiryOutcome::answered(
                                &original.inquiry_id,
                                "saved answer".into(),
                            ),
                        )
                        .await
                        .unwrap();
                }
                let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                let storage = JsonlStorageAdapter::with_root(home.path().to_owned());
                storage
                    .init_session(
                        &actor.session_info,
                        crate::session::persistence::default_model_id(),
                    )
                    .await
                    .unwrap();
                for event in actor.chat_state_handle.timeline_events().await.unwrap() {
                    storage
                        .append_timeline_event_durable(&actor.session_info, &event)
                        .await
                        .unwrap();
                }
                let session_id = actor.session_info.id.to_string();
                drop(storage);
                drop(actor);
                let timeline =
                    crate::session::storage::load_timeline_by_id_at(&session_id, home.path())
                        .unwrap()
                        .unwrap();
                let (mut restored, _) = crate::session::actor::tests::support::build_actor().await;
                let (events, _) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut restored).unwrap().chat_state_handle =
                    chat_state::ChatStateActor::spawn_from_timeline(
                        timeline.events().to_vec(),
                        config,
                        Box::new(chat_state::NullTimelinePersistence),
                        events,
                        Default::default(),
                    )
                    .await
                    .unwrap();
                restored
                    .recover_interrupted_coordination_inquiries()
                    .await
                    .unwrap();
                let before = restored
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .len();
                // Prevent any new queue from sampling even in the failing regression.
                restored.coordination_inquiry_active.set(true);
                // Both runtimes are cold and the source has only its start receipt:
                // the real IPC retry must reach the restored target's durable guard.
                let source_info = crate::session::info::Info {
                    id: acp::SessionId::new(original.source_session_id.clone()),
                    cwd: original.source_cwd.clone(),
                };
                let storage = JsonlStorageAdapter::with_root(home.path().to_owned());
                storage
                    .init_session(
                        &source_info,
                        crate::session::persistence::default_model_id(),
                    )
                    .await
                    .unwrap();
                let mut source_timeline = chat_state::Timeline::default();
                let event = source_timeline
                    .record(
                        crate::coordination::InquiryEvent::OutgoingStarted {
                            inquiry_id: original.inquiry_id.clone(),
                            target_session_id: session_id.clone(),
                            question: original.question.clone(),
                        }
                        .timeline_kind(),
                    )
                    .unwrap();
                storage
                    .append_timeline_event_durable(&source_info, &event)
                    .await
                    .unwrap();
                drop(storage);
                let source_runtime =
                    crate::coordination::CoordinationRuntime::new(home.path().to_owned());
                let target_runtime =
                    crate::coordination::CoordinationRuntime::new(home.path().to_owned());
                source_runtime.ensure_started().await.unwrap();
                target_runtime.ensure_started().await.unwrap();
                let snapshot = |id: String| crate::coordination::LocalSessionSnapshot {
                    session_id: id,
                    canonical_cwd: "/repo".into(),
                    main_agent: "test".into(),
                    activity: crate::agent::roster::RosterActivity::Idle,
                    subagents: Default::default(),
                };
                source_runtime.publish_sessions(vec![snapshot(original.source_session_id.clone())]);
                target_runtime.publish_sessions(vec![snapshot(session_id.clone())]);
                let mut incoming = target_runtime.take_inquiry_receiver().unwrap();
                let target_actor = restored.clone();
                let dispatch = tokio::task::spawn_local(async move {
                    target_actor
                        .enqueue_coordination_inquiry(incoming.recv().await.unwrap())
                        .await;
                });
                let ipc_outcome = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    source_runtime.ask_session(
                        &original.inquiry_id,
                        &original.source_session_id,
                        &session_id,
                        &original.question,
                        None,
                        Default::default(),
                    ),
                )
                .await
                .unwrap()
                .unwrap();
                dispatch.await.unwrap();
                let (mut retry, response) = inquiry(1);
                retry.inquiry_id = original.inquiry_id.clone();
                retry.source_peer_id = "new-process-after-source-restart".into();
                restored.enqueue_coordination_inquiry(retry).await;
                let outcome = tokio::time::timeout(std::time::Duration::from_millis(300), response)
                    .await
                    .expect("durable receipt must answer without queueing")
                    .unwrap();
                assert_eq!(outcome, ipc_outcome);
                if completed {
                    assert_eq!(outcome.answer.as_deref(), Some("saved answer"));
                } else {
                    assert_eq!(
                        outcome.error.unwrap().code,
                        crate::coordination::CoordinationErrorCode::TargetRestarted
                    );
                }
                assert_eq!(
                    restored
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    before
                );
                assert!(restored.coordination_inquiries.borrow().is_empty());
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn target_open_receipt_and_conflicting_payload_never_enqueue_twice() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _) = crate::session::actor::tests::support::build_actor().await;
            let (original, _) = inquiry(1);
            actor
                .record_incoming_coordination_notice(&original)
                .await
                .unwrap();
            actor.coordination_inquiry_active.set(true);
            let before = actor
                .chat_state_handle
                .timeline_events()
                .await
                .unwrap()
                .len();
            for conflict in [false, true] {
                let (mut retry, response) = inquiry(1);
                retry.inquiry_id = original.inquiry_id.clone();
                if conflict {
                    retry.question.push_str("changed");
                }
                actor.enqueue_coordination_inquiry(retry).await;
                let outcome = tokio::time::timeout(std::time::Duration::from_millis(300), response)
                    .await
                    .expect("duplicate receipt must not enter the queue")
                    .unwrap();
                assert_eq!(
                    outcome.error.unwrap().code,
                    if conflict {
                        crate::coordination::CoordinationErrorCode::Conflict
                    } else {
                        crate::coordination::CoordinationErrorCode::Busy
                    }
                );
            }
            assert_eq!(
                actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .len(),
                before
            );
            assert!(actor.coordination_inquiries.borrow().is_empty());
        })
        .await;
}
