use super::*;

#[tokio::test]
async fn cold_source_cancel_retry_recovers_already_admitted_target_result() {
    use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
    let home = tempfile::tempdir().unwrap();
    let source = CoordinationRuntime::new(home.path().to_owned());
    let target = CoordinationRuntime::new(home.path().to_owned());
    source.ensure_started().await.unwrap();
    target.ensure_started().await.unwrap();
    source.publish_sessions(vec![snapshot("source", "/repo")]);
    target.publish_sessions(vec![snapshot("target", "/repo")]);
    let id = uuid::Uuid::now_v7().to_string();
    let info = crate::session::info::Info {
        id: acp_transport::protocol::SessionId::new("source"),
        cwd: "/repo".into(),
    };
    let storage = JsonlStorageAdapter::with_root(home.path().to_owned());
    storage
        .init_session(&info, crate::session::persistence::default_model_id())
        .await
        .unwrap();
    let fact = chat_state::Timeline::default()
        .record(
            super::super::super::InquiryEvent::OutgoingStarted {
                inquiry_id: id.clone(),
                target_session_id: "target".into(),
                question: "original work?".into(),
            }
            .timeline_kind(),
        )
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &fact)
        .await
        .unwrap();
    let mut incoming = target.take_inquiry_receiver().unwrap();
    let (first, ()) = tokio::join!(
        source.ask_session(
            &id,
            "source",
            "target",
            "original work?",
            None,
            Default::default()
        ),
        async {
            let inquiry = incoming.recv().await.unwrap();
            assert!(inquiry.cancellation.admit_success());
            inquiry
                .respond_to
                .send(InquiryOutcome::answered(
                    &id,
                    "already admitted original answer".into(),
                ))
                .unwrap();
        }
    );
    let first = first.unwrap();
    drop(source);
    let restored = CoordinationRuntime::new(home.path().to_owned());
    restored.ensure_started().await.unwrap();
    restored.publish_sessions(vec![snapshot("source", "/repo")]);
    let intent = CancellationToken::new();
    intent.cancel();
    let recovered = timeout(
        Duration::from_secs(5),
        restored.ask_session(&id, "source", "target", "original work?", None, intent),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        recovered, first,
        "durable start is possible prior dispatch, not proof that target work never began"
    );
    assert!(
        incoming.try_recv().is_err(),
        "retry must not run the Sideband again"
    );
}

#[tokio::test]
async fn cancel_during_partial_inquiry_frame_preserves_the_original_reader() {
    use tokio::io::AsyncWriteExt;
    use tokio::sync::oneshot;
    let home = tempfile::tempdir().unwrap();
    let source = CoordinationRuntime::new(home.path().to_owned());
    source.ensure_started().await.unwrap();
    source.publish_sessions(vec![snapshot("source", "/repo")]);
    let target = CoordinationRuntime::new(home.path().to_owned());
    let endpoint = PrivateEndpoint::new().unwrap();
    let listener = LocalListener::bind(endpoint.path()).unwrap();
    target.shared.endpoint.set(endpoint).unwrap();
    target.publish_sessions(vec![snapshot("target", "/repo")]);
    target.shared.publish().unwrap();
    let manifest = target.shared.current_manifest();
    let identity = manifest.clone();
    let id = uuid::Uuid::now_v7().to_string();
    let response_id = id.clone();
    let (partial, written) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let hello = || ServerHello {
            protocol_version: PROTOCOL_VERSION,
            peer_id: identity.peer_id.clone(),
            incarnation: identity.incarnation.clone(),
            accepted: true,
            error: None,
        };
        let (mut original, _) = listener.accept().await.unwrap();
        let _: ClientHello = read_json(&mut original).await.unwrap();
        write_json(&mut original, &hello()).await.unwrap();
        assert!(
            matches!(read_json::<_, Request>(&mut original).await.unwrap(), Request::Ask { inquiry_id, .. } if inquiry_id == response_id)
        );
        let progress = serde_json::to_vec(&Response::Progress {
            inquiry_id: response_id.clone(),
            phase: InquiryPhase::Running,
        })
        .unwrap();
        let payload = serde_json::to_vec(&Response::Inquiry {
            outcome: InquiryOutcome::answered(&response_id, "original admitted answer".into()),
        })
        .unwrap();
        let length = (payload.len() as u32).to_be_bytes();
        // One prelude makes the next header bytes available when the client
        // finishes the Progress frame and polls its next biased read branch.
        let mut prelude = (progress.len() as u32).to_be_bytes().to_vec();
        prelude.extend_from_slice(&progress);
        prelude.extend_from_slice(&length[..2]);
        original.write_all(&prelude).await.unwrap();
        original.flush().await.unwrap();
        partial.send(()).unwrap();
        let (mut cancel, _) = listener.accept().await.unwrap();
        let _: ClientHello = read_json(&mut cancel).await.unwrap();
        write_json(&mut cancel, &hello()).await.unwrap();
        assert!(
            matches!(read_json::<_, Request>(&mut cancel).await.unwrap(), Request::Cancel { inquiry_id, target_session_id }
            if inquiry_id == response_id && target_session_id == "target")
        );
        write_json(
            &mut cancel,
            &Response::Cancellation {
                inquiry_id: response_id,
                accepted: false,
            },
        )
        .await
        .unwrap();
        original.write_all(&length[2..]).await.unwrap();
        original.write_all(&payload).await.unwrap();
        original.flush().await.unwrap();
    });
    let intent = InquiryCancellation::new();
    let request_intent = intent.clone();
    let shared = source.shared.clone();
    let (progress, mut phases) = mpsc::unbounded_channel();
    let request = tokio::spawn(async move {
        let mut dispatched = false;
        request_inquiry(
            &shared,
            &manifest,
            "source",
            &Request::Ask {
                inquiry_id: id,
                target_session_id: "target".into(),
                question: "original question".into(),
            },
            Some(&progress),
            &request_intent,
            &mut dispatched,
        )
        .await
    });
    written.await.unwrap();
    assert_eq!(phases.recv().await, Some(InquiryPhase::Running));
    intent.cancel(InquiryCancellationReason::Explicit);
    let answer = timeout(Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(answer.status, InquiryStatus::Answered);
    assert_eq!(answer.answer.as_deref(), Some("original admitted answer"));
    server.await.unwrap();
}

async fn query_during_source_startup(cancel_during_admission: bool) {
    use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::oneshot;

    let home = tempfile::tempdir().unwrap();
    let source = CoordinationRuntime::new(home.path().to_owned());
    let target = CoordinationRuntime::new(home.path().to_owned());
    source.ensure_started().await.unwrap();
    target.ensure_started().await.unwrap();
    source.publish_sessions(vec![snapshot("source", "/repo")]);
    target.publish_sessions(vec![snapshot("target", "/repo")]);
    let mut incoming = target.take_inquiry_receiver().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let info = crate::session::info::Info {
        id: acp_transport::protocol::SessionId::new("source"),
        cwd: "/repo".into(),
    };
    let storage = JsonlStorageAdapter::with_root(home.path().to_owned());
    storage
        .init_session(&info, crate::session::persistence::default_model_id())
        .await
        .unwrap();
    let event = chat_state::Timeline::default()
        .record(
            super::super::super::inquiry::InquiryEvent::OutgoingStarted {
                inquiry_id: id.clone(),
                target_session_id: "target".into(),
                question: "work?".into(),
            }
            .timeline_kind(),
        )
        .unwrap();
    let admissions = Arc::new(AtomicUsize::new(0));
    let (committed_tx, committed_rx) = oneshot::channel();
    let (ack_tx, ack_rx) = oneshot::channel();
    let cancellation = CancellationToken::new();
    let ask = {
        let handle = source.handle();
        let id = id.clone();
        let admissions = Arc::clone(&admissions);
        let cancellation = cancellation.clone();
        tokio::spawn(async move {
            handle
                .ask_session_with_admission(
                    &id,
                    "source",
                    "target",
                    "work?",
                    None,
                    cancellation,
                    async move {
                        admissions.fetch_add(1, Ordering::SeqCst);
                        storage
                            .append_timeline_event_durable(&info, &event)
                            .await
                            .unwrap();
                        committed_tx.send(()).unwrap();
                        ack_rx.await.unwrap();
                        Ok(())
                    },
                )
                .await
        })
    };
    committed_rx.await.unwrap();
    let query = {
        let handle = source.handle();
        let id = id.clone();
        tokio::spawn(async move { handle.get_inquiry(&id, "source").await })
    };
    let state = timeout(Duration::from_secs(2), query)
        .await
        .unwrap()
        .unwrap()
        .expect("a query must not classify a live durable start as a cold-source orphan");
    assert_eq!(state.phase, InquiryPhase::Discovering);
    assert!(state.outcome.is_none());
    assert!(
        incoming.try_recv().is_err(),
        "dispatch preceded durable ACK"
    );
    let conflict = timeout(
        Duration::from_secs(2),
        source.handle().ask_session_with_admission(
            &id,
            "source",
            "changed-target",
            "work?",
            None,
            Default::default(),
            async { panic!("a conflicting retry must not run admission") },
        ),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(conflict.code, CoordinationErrorCode::Conflict);
    let (retry_progress_tx, mut retry_progress_rx) = mpsc::unbounded_channel();
    let retry = {
        let handle = source.handle();
        let id = id.clone();
        let admissions = Arc::clone(&admissions);
        tokio::spawn(async move {
            handle
                .ask_session_with_admission(
                    &id,
                    "source",
                    "target",
                    "work?",
                    Some(retry_progress_tx),
                    Default::default(),
                    async move {
                        admissions.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
        })
    };
    assert_eq!(
        timeout(Duration::from_secs(2), retry_progress_rx.recv())
            .await
            .unwrap(),
        Some(InquiryPhase::Discovering),
        "the retry must join the live startup before its ACK is released"
    );
    assert_eq!(admissions.load(Ordering::SeqCst), 1);
    if cancel_during_admission {
        assert!(
            timeout(
                Duration::from_secs(2),
                source.cancel_session(&id, "source", "target")
            )
            .await
            .unwrap()
            .unwrap(),
            "startup must remain cancellable before durable ACK"
        );
    }
    ack_tx.send(()).unwrap();
    if !cancel_during_admission {
        let inquiry = timeout(Duration::from_secs(2), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        inquiry
            .respond_to
            .send(InquiryOutcome::answered(&id, "one answer".into()))
            .unwrap();
    }
    let outcome = ask.await.unwrap().unwrap();
    assert_eq!(
        outcome.status,
        if cancel_during_admission {
            InquiryStatus::Cancelled
        } else {
            InquiryStatus::Answered
        }
    );
    assert_eq!(retry.await.unwrap().unwrap(), outcome);
    assert_eq!(admissions.load(Ordering::SeqCst), 1);
    assert!(incoming.try_recv().is_err());
}

#[tokio::test]
async fn query_observes_live_source_startup_before_durable_ack() {
    query_during_source_startup(false).await;
}

#[tokio::test]
async fn cancellation_during_source_admission_never_dispatches() {
    query_during_source_startup(true).await;
}

#[tokio::test]
async fn failed_source_admission_does_not_dispatch_or_leave_a_live_record() {
    let home = tempfile::tempdir().unwrap();
    let runtime = CoordinationRuntime::new(home.path().to_owned());
    runtime.ensure_started().await.unwrap();
    runtime.publish_sessions(vec![
        snapshot("source", "/repo"),
        snapshot("target", "/repo"),
    ]);
    let mut incoming = runtime.take_inquiry_receiver().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let error = runtime
        .handle()
        .ask_session_with_admission(
            &id,
            "source",
            "target",
            "work?",
            None,
            Default::default(),
            std::future::ready(Err(CoordinationError::new(
                CoordinationErrorCode::AuditFailure,
                "commit failed",
            ))),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, CoordinationErrorCode::AuditFailure);
    assert!(runtime.shared.outgoing.lock().await.is_empty());
    assert!(incoming.try_recv().is_err());
    assert_eq!(
        runtime
            .handle()
            .get_inquiry(&id, "source")
            .await
            .unwrap_err()
            .code,
        CoordinationErrorCode::NotFound
    );
    let retry_error = runtime
        .handle()
        .ask_session_with_admission(
            &id,
            "source",
            "target",
            "work?",
            None,
            Default::default(),
            std::future::ready(Err(CoordinationError::new(
                CoordinationErrorCode::AuditFailure,
                "retry reached admission",
            ))),
        )
        .await
        .unwrap_err();
    assert_eq!(retry_error.message, "retry reached admission");
    assert!(runtime.shared.outgoing.lock().await.is_empty());
    assert!(incoming.try_recv().is_err());
}

#[tokio::test]
async fn uuid_text_aliases_cannot_create_separate_inquiry_identities() {
    let home = tempfile::tempdir().unwrap();
    let runtime = CoordinationRuntime::new(home.path().to_owned());
    runtime.ensure_started().await.unwrap();
    runtime.publish_sessions(vec![
        snapshot("source", "/repo"),
        snapshot("target", "/repo"),
    ]);
    let id = uuid::Uuid::parse_str("019a0000-abcd-7abc-8def-0123456789ab").unwrap();
    assert!(validate_inquiry_id(&id.to_string()).is_ok());
    for alias in [
        id.to_string().to_uppercase(),
        id.simple().to_string(),
        id.urn().to_string(),
        id.braced().to_string(),
    ] {
        assert_eq!(uuid::Uuid::parse_str(&alias).unwrap(), id);
        assert_eq!(
            runtime
                .ask_session(
                    &alias,
                    "source",
                    "target",
                    "work?",
                    None,
                    Default::default()
                )
                .await
                .unwrap_err()
                .code,
            CoordinationErrorCode::InvalidRequest
        );
        assert_eq!(
            runtime
                .handle()
                .get_inquiry(&alias, "source")
                .await
                .unwrap_err()
                .code,
            CoordinationErrorCode::InvalidRequest
        );
        assert_eq!(
            runtime
                .cancel_session(&alias, "source", "target")
                .await
                .unwrap_err()
                .code,
            CoordinationErrorCode::InvalidRequest
        );
        let response = request_peer(
            &runtime.shared,
            &runtime.shared.current_manifest(),
            "source",
            Request::Cancel {
                inquiry_id: alias,
                target_session_id: "target".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(response, Response::Error { code, .. } if code == "invalid_inquiry_id"));
    }
    assert!(runtime.shared.inquiries.lock().await.is_empty());
    assert!(runtime.shared.outgoing.lock().await.is_empty());
}

#[tokio::test]
async fn same_id_in_two_source_sessions_keeps_answers_and_cancellation_isolated() {
    let home = tempfile::tempdir().unwrap();
    let source = CoordinationRuntime::new(home.path().to_owned());
    let target = CoordinationRuntime::new(home.path().to_owned());
    source.ensure_started().await.unwrap();
    target.ensure_started().await.unwrap();
    source.publish_sessions(vec![
        snapshot("a", "/repo"),
        snapshot("b", "/repo"),
        snapshot("c", "/repo"),
    ]);
    target.publish_sessions(vec![snapshot("target", "/repo")]);
    let mut incoming = target.take_inquiry_receiver().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let ask = |session: &'static str| {
        let handle = source.handle();
        let id = id.clone();
        tokio::spawn(async move {
            handle
                .ask_session(&id, session, "target", "work?", None, Default::default())
                .await
        })
    };
    let a = ask("a");
    let first = timeout(Duration::from_secs(2), incoming.recv())
        .await
        .unwrap()
        .unwrap();
    let b = ask("b");
    let second = timeout(Duration::from_secs(2), incoming.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.source_session_id, "a");
    assert_eq!(second.source_session_id, "b");
    assert!(!cancel_inquiry(&target.shared, "c", &id, "target").await);
    assert!(!cancel_inquiry(&target.shared, "a", &id, "wrong-target").await);
    assert!(source.cancel_session(&id, "a", "target").await.unwrap());
    timeout(Duration::from_secs(2), first.cancellation.cancelled())
        .await
        .unwrap();
    assert!(!second.cancellation.is_cancelled());
    first
        .respond_to
        .send(first.cancellation.outcome(&id))
        .unwrap();
    second
        .respond_to
        .send(InquiryOutcome::answered(&id, "answer b".into()))
        .unwrap();
    assert_eq!(a.await.unwrap().unwrap().status, InquiryStatus::Cancelled);
    assert_eq!(
        b.await.unwrap().unwrap().answer.as_deref(),
        Some("answer b")
    );
    assert_eq!(
        source
            .handle()
            .get_inquiry(&id, "a")
            .await
            .unwrap()
            .outcome
            .unwrap()
            .status,
        InquiryStatus::Cancelled
    );
    assert_eq!(
        source
            .handle()
            .get_inquiry(&id, "b")
            .await
            .unwrap()
            .outcome
            .unwrap()
            .answer
            .as_deref(),
        Some("answer b")
    );
    assert_eq!(
        source
            .handle()
            .get_inquiry(&id, "c")
            .await
            .unwrap_err()
            .code,
        CoordinationErrorCode::NotFound
    );
    let reply = request_peer(
        &source.shared,
        &target.shared.current_manifest(),
        "b",
        Request::Cancel {
            inquiry_id: id,
            target_session_id: "target".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        reply,
        Response::Cancellation {
            accepted: false,
            ..
        }
    ));
    assert!(incoming.try_recv().is_err());
}

#[tokio::test]
async fn source_reincarnation_reuses_the_original_target_result() {
    let home = tempfile::tempdir().unwrap();
    let source = CoordinationRuntime::new(home.path().to_owned());
    let target = CoordinationRuntime::new(home.path().to_owned());
    source.ensure_started().await.unwrap();
    target.ensure_started().await.unwrap();
    source.publish_sessions(vec![snapshot("source", "/repo")]);
    target.publish_sessions(vec![snapshot("target", "/repo")]);
    let mut incoming = target.take_inquiry_receiver().unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let answer = async {
        let inbound = incoming.recv().await.unwrap();
        inbound
            .respond_to
            .send(InquiryOutcome::answered(&id, "one answer".into()))
            .unwrap();
    };
    let (first, ()) = tokio::join!(
        source.ask_session(&id, "source", "target", "work?", None, Default::default()),
        answer
    );
    drop(source);
    let restored = CoordinationRuntime::new(home.path().to_owned());
    restored.ensure_started().await.unwrap();
    restored.publish_sessions(vec![snapshot("source", "/repo")]);
    let second = timeout(
        Duration::from_secs(2),
        restored.ask_session(&id, "source", "target", "work?", None, Default::default()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(second, first.unwrap());
    assert!(
        incoming.try_recv().is_err(),
        "a new peer identity must not dispatch another inquiry"
    );
}

#[tokio::test]
async fn cold_source_started_fact_pins_target_and_question_without_a_terminal() {
    use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
    let home = tempfile::tempdir().unwrap();
    let info = crate::session::info::Info {
        id: acp_transport::protocol::SessionId::new("source"),
        cwd: "/repo".into(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    let storage = JsonlStorageAdapter::with_root(home.path().to_owned());
    storage
        .init_session(&info, crate::session::persistence::default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    // Even a later conflicting start must not replace the first binding.
    for (target, question) in [("original", "work?"), ("changed", "different?")] {
        let event = timeline
            .record(
                super::super::super::inquiry::InquiryEvent::OutgoingStarted {
                    inquiry_id: id.clone(),
                    target_session_id: target.into(),
                    question: question.into(),
                }
                .timeline_kind(),
            )
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
    }
    drop(storage);
    let runtime = CoordinationRuntime::new(home.path().to_owned());
    runtime.ensure_started().await.unwrap();
    runtime.publish_sessions(vec![snapshot("source", "/repo")]);
    for (target, question) in [("changed", "work?"), ("original", "different?")] {
        assert_eq!(
            runtime
                .ask_session(&id, "source", target, question, None, Default::default())
                .await
                .unwrap_err()
                .code,
            CoordinationErrorCode::Conflict
        );
    }
    assert_eq!(
        runtime
            .handle()
            .get_inquiry(&id, "source")
            .await
            .unwrap_err()
            .code,
        CoordinationErrorCode::SourceUnavailable
    );
    assert!(runtime.shared.outgoing.lock().await.is_empty());
    assert_eq!(
        runtime
            .cancel_session(&id, "source", "changed")
            .await
            .unwrap_err()
            .code,
        CoordinationErrorCode::Conflict
    );
    // A completion for another payload must not poison recovery of this ID.
    let storage = JsonlStorageAdapter::with_root(home.path().to_owned());
    let event = timeline
        .record(
            super::super::super::inquiry::InquiryEvent::OutgoingCompleted {
                audit: super::super::super::inquiry::InquiryAudit {
                    target_session_id: "changed".into(),
                    question: "different?".into(),
                    outcome: InquiryOutcome::answered(&id, "unrelated answer".into()),
                },
            }
            .timeline_kind(),
        )
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &event)
        .await
        .unwrap();
    drop(storage);
    assert_eq!(
        runtime
            .handle()
            .get_inquiry(&id, "source")
            .await
            .unwrap_err()
            .code,
        CoordinationErrorCode::AuditFailure
    );
    assert_eq!(
        runtime
            .ask_session(&id, "source", "original", "work?", None, Default::default())
            .await
            .unwrap_err()
            .code,
        CoordinationErrorCode::AuditFailure
    );
    assert!(runtime.shared.outgoing.lock().await.is_empty());
}

#[tokio::test]
async fn pre_cancelled_source_does_not_dispatch_an_inquiry() {
    let home = tempfile::tempdir().unwrap();
    let runtime = CoordinationRuntime::new(home.path().to_owned());
    runtime.ensure_started().await.unwrap();
    runtime.publish_sessions(vec![
        snapshot("source", "/repo"),
        snapshot("target", "/repo"),
    ]);
    let mut incoming = runtime.take_inquiry_receiver().unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let id = uuid::Uuid::now_v7().to_string();
    let outcome = runtime
        .ask_session(&id, "source", "target", "work?", None, token)
        .await
        .unwrap();
    assert_eq!(outcome.status, InquiryStatus::Cancelled);
    assert!(incoming.try_recv().is_err());
    assert!(runtime.shared.inquiries.lock().await.is_empty());
}

#[tokio::test]
async fn wrong_response_ids_fail_without_relay_or_retry() {
    for kind in ["progress", "outcome", "cancel"] {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_owned());
        source.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        let target = CoordinationRuntime::new(home.path().to_owned());
        let endpoint = PrivateEndpoint::new().unwrap();
        let listener = LocalListener::bind(endpoint.path()).unwrap();
        target.shared.endpoint.set(endpoint).unwrap();
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        target.shared.publish().unwrap();
        let manifest = target.shared.current_manifest();
        let response = match kind {
            "progress" => Response::Progress {
                inquiry_id: uuid::Uuid::now_v7().to_string(),
                phase: InquiryPhase::Running,
            },
            "outcome" => Response::Inquiry {
                outcome: InquiryOutcome::answered(
                    uuid::Uuid::now_v7().to_string(),
                    "wrong answer".into(),
                ),
            },
            _ => Response::Cancellation {
                inquiry_id: uuid::Uuid::now_v7().to_string(),
                accepted: true,
            },
        };
        let identity = manifest.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _: ClientHello = read_json(&mut stream).await.unwrap();
            write_json(
                &mut stream,
                &ServerHello {
                    protocol_version: PROTOCOL_VERSION,
                    peer_id: identity.peer_id,
                    incarnation: identity.incarnation,
                    accepted: true,
                    error: None,
                },
            )
            .await
            .unwrap();
            let _: Request = read_json(&mut stream).await.unwrap();
            write_json(&mut stream, &response).await.unwrap();
        });
        let id = uuid::Uuid::now_v7().to_string();
        let (progress, mut phases) = mpsc::unbounded_channel();
        let error = if kind == "cancel" {
            source
                .handle()
                .cancel_session(&id, "source", "target")
                .await
                .unwrap_err()
        } else {
            match request_inquiry(
                &source.shared,
                &manifest,
                "source",
                &Request::Ask {
                    inquiry_id: id,
                    target_session_id: "target".into(),
                    question: "work?".into(),
                },
                Some(&progress),
                &InquiryCancellation::new(),
                &mut false,
            )
            .await
            .unwrap_err()
            {
                InquiryRequestError::Transport(error) => error,
                other => panic!("unexpected error: {other}"),
            }
        };
        assert_eq!(error.code, CoordinationErrorCode::TransportError);
        assert!(
            !error.retryable,
            "correlation failures must not cause redelivery"
        );
        assert!(
            phases.try_recv().is_err(),
            "mismatched progress must not be relayed"
        );
        server.await.unwrap();
    }
}
