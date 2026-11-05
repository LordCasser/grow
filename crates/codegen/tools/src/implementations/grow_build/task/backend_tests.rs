use super::super::types::SubagentSnapshotStatus;
use super::*;
use std::sync::Arc;
use tokio::sync::mpsc;

fn durable_inspection(id: String) -> SubagentInspection {
    SubagentInspection {
        snapshot: SubagentSnapshot {
            subagent_id: id.clone(),
            description: "original task".into(),
            subagent_type: "coder".into(),
            status: super::super::types::SubagentSnapshotStatus::Cancelled {
                reason: Some("durable cancellation".into()),
            },
            started_at_epoch_ms: 1,
            duration_ms: 42,
        },
        parent_session_id: "parent".into(),
        child_session_id: id,
        fork_parent_prompt_id: Some("parent-prompt".into()),
        resumed_from: None,
    }
}

/// Helper: receive the next event, match the expected variant, or panic.
macro_rules! recv_event {
    ($rx:expr, Spawn) => {{
        let event = $rx.recv().await.unwrap();
        match event {
            SubagentEvent::Spawn(inner) => inner,
            _ => panic!("Expected SubagentEvent::Spawn, got different variant"),
        }
    }};
    ($rx:expr, $variant:ident) => {{
        let event = $rx.recv().await.unwrap();
        match event {
            SubagentEvent::$variant(inner) => inner,
            _ => panic!(
                "Expected SubagentEvent::{}, got different variant",
                stringify!($variant)
            ),
        }
    }};
}

#[tokio::test]
async fn model_control_backend_stamps_bound_source_and_preserves_host_receipt() {
    use sampling_types::ReasoningEffort;
    use tokio_util::sync::CancellationToken;

    for accepted in [true, false] {
        let (sender, mut commands) = mpsc::unbounded_channel();
        let backend =
            ChannelBackend::for_session(sender, "actual-parent").with_history_query(|_| async {
                panic!("read-only history cannot grant model control");
            });
        let cancellation = CancellationToken::new();
        let call = backend.set_model(
            "exact-child".into(),
            Some("catalog-model".into()),
            Some(ReasoningEffort::None),
            cancellation.clone(),
        );
        tokio::pin!(call);
        assert!(futures::poll!(call.as_mut()).is_pending());
        let request = recv_event!(commands, SetModel);
        assert_eq!(request.source_session_id, "actual-parent");
        assert_eq!(request.subagent_id, "exact-child");
        assert_eq!(request.model.as_deref(), Some("catalog-model"));
        assert_eq!(request.effort, Some(ReasoningEffort::None));
        cancellation.cancel();
        assert!(request.cancellation.is_cancelled());
        assert!(futures::poll!(call.as_mut()).is_pending());
        let admission = if accepted {
            Ok(())
        } else {
            Err("host refused admission".into())
        };
        request.respond_to.send(admission.clone()).unwrap();
        assert_eq!(call.await, admission);
        assert!(commands.try_recv().is_err());
    }
}

#[tokio::test]
async fn model_control_backend_rejects_unbound_observer_and_predispatch_cancellation() {
    use tokio_util::sync::CancellationToken;

    let (sender, mut commands) = mpsc::unbounded_channel();
    let observer =
        ChannelBackend::for_authorized_observer(sender.clone()).with_history_query(|_| async {
            panic!("history access is not control authority");
        });
    assert!(
        observer
            .set_model(
                "child".into(),
                Some("catalog-model".into()),
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
            .contains("Unbound")
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(
        ChannelBackend::for_session(sender, "parent")
            .set_model(
                "child".into(),
                Some("catalog-model".into()),
                None,
                cancellation,
            )
            .await
            .unwrap_err()
            .contains("cancelled")
    );
    assert!(commands.try_recv().is_err());
}

#[tokio::test]
async fn model_control_backend_closed_command_or_admission_channel_fails() {
    use tokio_util::sync::CancellationToken;

    let (sender, receiver) = mpsc::unbounded_channel();
    drop(receiver);
    let backend = ChannelBackend::for_session(sender, "parent");
    assert!(
        backend
            .set_model(
                "child".into(),
                Some("catalog-model".into()),
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
            .contains("before confirming admission")
    );

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(sender, "parent");
    let call = backend.set_model(
        "child".into(),
        Some("catalog-model".into()),
        None,
        CancellationToken::new(),
    );
    tokio::pin!(call);
    assert!(futures::poll!(call.as_mut()).is_pending());
    let request = recv_event!(receiver, SetModel);
    drop(request.respond_to);
    assert!(
        call.await
            .unwrap_err()
            .contains("before confirming admission")
    );
}

#[tokio::test]
async fn ask_cancellation_intent_preserves_the_host_response_receiver() {
    use super::super::interaction::{AgentInteraction, AgentInteractionOutput};
    for host_fails in [false, true] {
        let (sender, mut commands) = mpsc::unbounded_channel();
        let backend = ChannelBackend::for_session(sender, "actual-source");
        let cancellation = tokio_util::sync::CancellationToken::new();
        let call = backend.interact(
            "ask-operation".into(),
            Some("owned-child".into()),
            AgentInteraction::Ask {
                question: "original question".into(),
            },
            cancellation.clone(),
        );
        tokio::pin!(call);
        assert!(futures::poll!(call.as_mut()).is_pending());
        let request = recv_event!(commands, Interact);
        assert_eq!(request.source_session_id, "actual-source");
        cancellation.cancel();
        assert!(request.cancellation.is_cancelled());
        // Poll the public backend at the exact cancellation boundary: a local
        // intent cannot replace the still-owned host response with a fast Err.
        assert!(futures::poll!(call.as_mut()).is_pending());
        if host_fails {
            request
                .respond_to
                .send(Err("actual host terminal failure".into()))
                .unwrap();
            assert_eq!(call.await.unwrap_err(), "actual host terminal failure");
        } else {
            request
                .respond_to
                .send(Ok(AgentInteractionOutput {
                    id: request.id,
                    status: "answered".into(),
                    receipt_id: None,
                    subagent_task_name: None,
                    target_session_id: None,
                    answer: Some("host-admitted answer".into()),
                    error: None,
                }))
                .unwrap();
            let result = call.await.unwrap();
            assert_eq!(result.status, "answered");
            assert_eq!(result.answer.as_deref(), Some("host-admitted answer"));
        }
    }
}

#[tokio::test]
async fn channel_backend_spawn_success() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let req = recv_event!(rx, Spawn);
        assert_eq!(req.request.id, "test-id");
        assert_eq!(req.request.prompt, "do something");
        req.result_tx
            .send(SubagentResult {
                success: true,
                output: Arc::from("done"),
                subagent_id: "test-id".to_string(),
                child_session_id: "test-id".to_string(),
                tool_calls: 3,
                turns: 1,
                duration_ms: 500,
                ..Default::default()
            })
            .unwrap();
    });

    let request = SubagentRequest {
        id: "test-id".to_string(),

        origin_tool_call_id: None,
        prompt: "do something".to_string(),
        description: "test".to_string(),
        subagent_type: "general-purpose".to_string(),
        parent_session_id: "parent".to_string(),
        parent_step_id: None,
        parent_prompt_id: None,
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: false,
        surface_completion: true,
        await_to_completion: false,
        fork_context: false,
        owner: super::super::types::SubagentOwner::Task,
        goal_context: None,
        cancel_token: tokio_util::sync::CancellationToken::new(),
    };

    let result = backend.spawn(request).await.unwrap();
    assert!(result.success);
    assert_eq!(result.subagent_id, "test-id");
    assert_eq!(result.tool_calls, 3);

    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_spawn_closed_channel() {
    let (tx, rx) = mpsc::unbounded_channel::<SubagentEvent>();
    drop(rx);

    let backend = ChannelBackend::new(tx);

    let request = SubagentRequest {
        id: "test-id".to_string(),

        origin_tool_call_id: None,
        prompt: "do something".to_string(),
        description: "test".to_string(),
        subagent_type: "general-purpose".to_string(),
        parent_session_id: "parent".to_string(),
        parent_step_id: None,
        parent_prompt_id: None,
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: false,
        surface_completion: true,
        await_to_completion: false,
        fork_context: false,
        owner: super::super::types::SubagentOwner::Task,
        goal_context: None,
        cancel_token: tokio_util::sync::CancellationToken::new(),
    };

    let err = backend.spawn(request).await.unwrap_err();
    assert!(err.to_string().contains("channel closed"));
}

#[tokio::test]
async fn channel_backend_queries_history_after_registry_miss() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent").with_history_query(|id| async move {
        assert_eq!(id, "original-child");
        Ok(Some(durable_inspection(id)))
    });
    let responder = tokio::spawn(async move {
        let req = recv_event!(rx, Query);
        assert_eq!(req.parent_session_id.as_deref(), Some("parent"));
        req.respond_to.send(Ok(None)).unwrap();
    });
    let snapshot = backend
        .query("original-child", false, None)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        snapshot.status,
        super::super::types::SubagentSnapshotStatus::Cancelled { .. }
    ));
    responder.await.unwrap();
}

#[tokio::test]
async fn channel_backend_closed_sender_falls_back_for_query_and_inspect() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    let reads = Arc::new(AtomicUsize::new(0));
    let backend = ChannelBackend::for_session(tx, "parent").with_history_query({
        let reads = reads.clone();
        move |id| {
            let reads = reads.clone();
            async move {
                reads.fetch_add(1, Ordering::SeqCst);
                Ok(Some(durable_inspection(id)))
            }
        }
    });

    let queried = backend.query("child", false, None).await.unwrap().unwrap();
    assert!(matches!(
        queried.status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    let inspected = backend.inspect("child").await.unwrap().unwrap();
    assert!(matches!(
        inspected.snapshot.status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    assert_eq!(inspected.parent_session_id, "parent");
    assert_eq!(inspected.child_session_id, "child");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn channel_backend_inspect_falls_back_after_unauthorized_with_remaining_deadline() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent")
        .with_history_query(|id| async move { Ok(Some(durable_inspection(id))) });
    let responder = tokio::spawn(async move {
        let req = recv_event!(rx, Inspect);
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        req.respond_to
            .send(Err(SubagentObservationError::Unauthorized))
            .unwrap();
    });

    let started = tokio::time::Instant::now();
    let inspection = backend.inspect("original-child").await.unwrap().unwrap();
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(4));
    assert_eq!(inspection.child_session_id, "original-child");
    assert!(matches!(
        inspection.snapshot.status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    responder.await.unwrap();
}

#[tokio::test]
async fn channel_backend_dropped_query_response_falls_back_to_history() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent")
        .with_history_query(|id| async move { Ok(Some(durable_inspection(id))) });
    let responder = tokio::spawn(async move {
        let req = recv_event!(rx, Query);
        drop(req.respond_to);
    });

    let snapshot = backend
        .query("original-child", false, None)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    responder.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn channel_backend_history_uses_remaining_query_deadline() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent")
        .with_history_query(|_| async { std::future::pending().await });
    let responder = tokio::spawn(async move {
        let req = recv_event!(rx, Query);
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        req.respond_to
            .send(Err(SubagentObservationError::Unauthorized))
            .unwrap();
    });
    let started = tokio::time::Instant::now();
    assert!(matches!(
        backend.query("original-child", false, None).await,
        Err(SubagentObservationError::TimedOut)
    ));
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(5));
    responder.await.unwrap();
}

#[tokio::test]
async fn channel_backend_query_found() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let req = recv_event!(rx, Query);
        assert_eq!(req.subagent_id, "sub-1");
        assert!(req.block);
        assert_eq!(req.timeout_ms, Some(5000));
        req.respond_to
            .send(Ok(Some(SubagentSnapshot {
                subagent_id: "sub-1".to_string(),
                description: "find bugs".to_string(),
                subagent_type: "explore".to_string(),
                status: super::super::types::SubagentSnapshotStatus::Completed {
                    output: "result".to_string(),
                    tool_calls: 2,
                    turns: 1,
                    tokens_used: 42,
                    worktree_path: None,
                },
                started_at_epoch_ms: 1000,
                duration_ms: 200,
            })))
            .unwrap();
    });

    let snap = backend.query("sub-1", true, Some(5000)).await.unwrap();
    let snap = snap.expect("snapshot should be present");
    assert_eq!(snap.subagent_id, "sub-1");
    assert_eq!(snap.description, "find bugs");
    assert_eq!(snap.subagent_type, "explore");
    assert_eq!(snap.started_at_epoch_ms, 1000);
    assert_eq!(snap.duration_ms, 200);
    match &snap.status {
        super::super::types::SubagentSnapshotStatus::Completed {
            output,
            tool_calls,
            turns,
            tokens_used,
            worktree_path,
        } => {
            assert_eq!(output, "result");
            assert_eq!(*tool_calls, 2);
            assert_eq!(*turns, 1);
            assert_eq!(*tokens_used, 42);
            assert!(worktree_path.is_none());
        }
        other => panic!("Expected Completed, got {:?}", other),
    }

    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_query_non_blocking_passes_through() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let req = recv_event!(rx, Query);
        assert_eq!(req.subagent_id, "sub-nb");
        assert!(!req.block, "block should be false");
        assert_eq!(req.timeout_ms, None, "timeout_ms should be None");
        req.respond_to.send(Ok(None)).unwrap();
    });

    let snap = backend.query("sub-nb", false, None).await;
    assert!(snap.unwrap().is_none());

    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_query_not_found() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let req = recv_event!(rx, Query);
        req.respond_to.send(Ok(None)).unwrap();
    });

    let snap = backend.query("nonexistent", false, None).await;
    assert!(snap.unwrap().is_none());

    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_query_live_sender_without_reply_times_out() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let _request = recv_event!(rx, Query);
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    });

    let result = backend.query("silent", false, None).await;
    assert!(matches!(result, Err(SubagentObservationError::TimedOut)));
    handle.abort();
}

#[tokio::test]
async fn channel_backend_query_dropped_responder_is_unavailable() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let request = recv_event!(rx, Query);
        drop(request.respond_to);
    });

    let result = backend.query("dropped", false, None).await;
    assert!(matches!(result, Err(SubagentObservationError::Unavailable)));
    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_cancel_success() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let req = recv_event!(rx, Cancel);
        match &req.target {
            SubagentCancelTarget::SubagentId(id) => assert_eq!(id, "sub-cancel"),
            other => panic!("Expected SubagentId, got {:?}", other),
        }
        req.respond_to
            .send(SubagentCancelOutcome::Cancelled)
            .unwrap();
    });

    let outcome = backend.cancel("sub-cancel").await;
    assert!(matches!(outcome, Ok(SubagentCancelOutcome::Cancelled)));

    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_cancel_closed_channel() {
    let (tx, rx) = mpsc::unbounded_channel::<SubagentEvent>();
    drop(rx);

    let backend = ChannelBackend::new(tx);

    let outcome = backend.cancel("sub-cancel").await;
    assert!(matches!(
        outcome,
        Err(SubagentObservationError::Unavailable)
    ));
}

#[tokio::test]
async fn workflow_spawn_future_drop_cancels_but_task_drop_does_not() {
    fn request_for(owner: super::super::types::SubagentOwner) -> SubagentRequest {
        SubagentRequest {
            id: "drop-owner-test".to_string(),

            origin_tool_call_id: None,
            prompt: "test".to_string(),
            description: "test".to_string(),
            subagent_type: "general-purpose".to_string(),
            parent_session_id: "parent".to_string(),
            parent_step_id: None,
            parent_prompt_id: None,
            resume_from: None,
            cwd: None,
            runtime_overrides: Default::default(),
            run_in_background: false,
            surface_completion: false,
            await_to_completion: true,
            fork_context: false,
            owner,
            goal_context: None,
            cancel_token: tokio_util::sync::CancellationToken::new(),
        }
    }

    for (owner, should_cancel) in [
        (super::super::types::SubagentOwner::Task, false),
        (super::super::types::SubagentOwner::workflow("wf-1"), true),
    ] {
        let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
        let backend = Arc::new(ChannelBackend::new(tx));
        let request = request_for(owner);
        let cancel_token = request.cancel_token.clone();
        let task = tokio::spawn({
            let backend = backend.clone();
            async move { backend.spawn(request).await }
        });
        let spawned = recv_event!(rx, Spawn);
        task.abort();
        let _ = task.await;
        assert_eq!(
            cancel_token.is_cancelled(),
            should_cancel,
            "only workflow receiver drop owns cancellation"
        );
        drop(spawned.result_tx);
    }
}

#[tokio::test]
async fn channel_backend_spawn_result_dropped() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let req = recv_event!(rx, Spawn);
        drop(req.result_tx);
    });

    let request = SubagentRequest {
        id: "drop-test".to_string(),

        origin_tool_call_id: None,
        prompt: "test".to_string(),
        description: "test".to_string(),
        subagent_type: "general-purpose".to_string(),
        parent_session_id: "parent".to_string(),
        parent_step_id: None,
        parent_prompt_id: None,
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: false,
        surface_completion: true,
        await_to_completion: false,
        fork_context: false,
        owner: super::super::types::SubagentOwner::Task,
        goal_context: None,
        cancel_token: tokio_util::sync::CancellationToken::new(),
    };

    let err = backend.spawn(request).await.unwrap_err();
    assert!(
        err.to_string().contains("result channel dropped"),
        "error: {err}"
    );

    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_query_closed_channel() {
    let (tx, rx) = mpsc::unbounded_channel::<SubagentEvent>();
    drop(rx);

    let backend = ChannelBackend::new(tx);

    let result = backend.query("sub-1", false, None).await;
    assert!(matches!(result, Err(SubagentObservationError::Unavailable)));
}

// ── validate_type ────────────────────────────────────────────────

#[tokio::test]
async fn channel_backend_validate_type_round_trips_outcome() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        let event = rx.recv().await.unwrap();
        match event {
            SubagentEvent::ValidateType(req) => {
                assert_eq!(req.subagent_type, "explore");
                assert_eq!(req.parent_session_id, "parent-1");
                req.respond_to
                    .send(SubagentValidateTypeOutcome::Ok)
                    .unwrap();
            }
            _ => panic!("Expected ValidateType event"),
        }
    });

    let outcome = backend.validate_type("explore", "parent-1").await;
    assert!(matches!(outcome, SubagentValidateTypeOutcome::Ok));
    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_validate_type_propagates_unknown_outcome() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    let handle = tokio::spawn(async move {
        if let Some(SubagentEvent::ValidateType(req)) = rx.recv().await {
            req.respond_to
                .send(SubagentValidateTypeOutcome::Unknown {
                    available: vec!["explore".into(), "plan".into()],
                })
                .unwrap();
        }
    });

    let outcome = backend.validate_type("invented", "p").await;
    match outcome {
        SubagentValidateTypeOutcome::Unknown { available } => {
            assert_eq!(available, vec!["explore".to_string(), "plan".to_string()]);
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
    handle.await.unwrap();
}

#[tokio::test]
async fn channel_backend_validate_type_returns_validation_unavailable_when_channel_closed() {
    let (tx, rx) = mpsc::unbounded_channel::<SubagentEvent>();
    drop(rx);
    let backend = ChannelBackend::new(tx);
    let outcome = backend.validate_type("explore", "p").await;
    assert!(matches!(
        outcome,
        SubagentValidateTypeOutcome::ValidationUnavailable
    ));
}

#[tokio::test]
async fn channel_backend_validate_type_returns_validation_unavailable_when_responder_dropped() {
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);
    let handle = tokio::spawn(async move {
        if let Some(SubagentEvent::ValidateType(req)) = rx.recv().await {
            drop(req.respond_to);
        }
    });
    let outcome = backend.validate_type("explore", "p").await;
    assert!(matches!(
        outcome,
        SubagentValidateTypeOutcome::ValidationUnavailable,
    ));
    handle.await.unwrap();
}

use super::super::types::test_capture;

#[tokio::test(start_paused = true)]
async fn channel_backend_validate_type_logs_warn_on_timeout() {
    let captured = test_capture::capture();
    let (tx, mut rx) = mpsc::unbounded_channel::<SubagentEvent>();
    let backend = ChannelBackend::new(tx);

    // Coordinator receives but never replies; keeps the responder
    // alive so the timeout arm fires (not responder-dropped).
    let holder = tokio::spawn(async move {
        if let Some(SubagentEvent::ValidateType(req)) = rx.recv().await {
            std::mem::forget(req.respond_to);
            std::future::pending::<()>().await;
        }
    });

    let validate = tokio::spawn(async move { backend.validate_type("explore", "p").await });
    tokio::time::advance(VALIDATE_TYPE_TIMEOUT + std::time::Duration::from_millis(1)).await;
    let outcome = validate.await.unwrap();
    assert!(matches!(
        outcome,
        SubagentValidateTypeOutcome::ValidationUnavailable
    ));

    let mut events_rx = captured.events_rx;
    let mut saw_timeout_warn = false;
    while let Ok(event) = events_rx.try_recv() {
        if event.level == tracing::Level::WARN
            && event.fields.contains("coordinator validation timed out")
            && event.fields.contains("subagent_type=explore")
            && event.fields.contains("timeout_ms=")
        {
            saw_timeout_warn = true;
            break;
        }
    }
    assert!(saw_timeout_warn, "must emit WARN with timeout_ms field");

    holder.abort();
}

#[test]
fn parse_timeout_ms_returns_none_for_unset() {
    assert_eq!(parse_timeout_ms(None), None);
}

#[test]
fn parse_timeout_ms_returns_none_for_unparseable() {
    assert_eq!(parse_timeout_ms(Some("not-a-number")), None);
    assert_eq!(parse_timeout_ms(Some("")), None);
    assert_eq!(parse_timeout_ms(Some("3.14")), None);
    assert_eq!(parse_timeout_ms(Some("-100")), None);
}

#[test]
fn parse_timeout_ms_returns_none_for_zero() {
    assert_eq!(parse_timeout_ms(Some("0")), None);
}

#[test]
fn parse_timeout_ms_returns_value_for_positive_integer() {
    assert_eq!(parse_timeout_ms(Some("5000")), Some(5000));
    assert_eq!(parse_timeout_ms(Some("1")), Some(1));
}

fn listed_subagent(id: &str, status: tool_types::SubagentStatus) -> tool_types::SubagentSummary {
    tool_types::SubagentSummary {
        subagent_id: id.into(),
        direct_parent_session_id: "parent".into(),
        subagent_type: "coder".into(),
        description: format!("task {id}"),
        status,
        started_at_epoch_ms: Some(1),
        duration_ms: Some(2),
        error: None,
    }
}

#[tokio::test]
async fn list_subagents_merges_history_and_runtime_without_output_payloads() {
    use tool_types::{SubagentListScope, SubagentStatus};

    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent").with_history_list(|| async {
        Ok(vec![
            listed_subagent("completed", SubagentStatus::Completed),
            listed_subagent(
                "invalid-history",
                SubagentStatus::ObservationInvalidEvidence,
            ),
            listed_subagent(
                "open-only-in-history",
                SubagentStatus::ObservationUnavailable,
            ),
            listed_subagent("runtime-update", SubagentStatus::ObservationUnavailable),
        ])
    });
    let call = tokio::spawn(async move {
        backend
            .list_subagents(
                SubagentListScope::All,
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            )
            .await
    });
    let request = match rx.recv().await.unwrap() {
        SubagentEvent::ListSubagents(request) => request,
        _ => panic!("expected ListSubagents event"),
    };
    assert_eq!(request.parent_session_id, "parent");
    assert_eq!(request.scope, SubagentListScope::All);
    request
        .respond_to
        .send(vec![
            listed_subagent("completed", SubagentStatus::Running),
            listed_subagent("invalid-history", SubagentStatus::Running),
            listed_subagent("runtime-only", SubagentStatus::RunningProgressUnavailable),
            listed_subagent("runtime-update", SubagentStatus::Running),
        ])
        .unwrap();

    let summaries = call.await.unwrap().unwrap();
    let ids = summaries
        .iter()
        .map(|item| item.subagent_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "completed",
            "invalid-history",
            "open-only-in-history",
            "runtime-only",
            "runtime-update"
        ]
    );
    assert_eq!(summaries[0].status, SubagentStatus::Completed);
    assert_eq!(
        summaries[1].status,
        SubagentStatus::ObservationInvalidEvidence
    );
    assert_eq!(summaries[2].status, SubagentStatus::ObservationUnavailable);
    assert_eq!(
        summaries[3].status,
        SubagentStatus::RunningProgressUnavailable
    );
    assert_eq!(summaries[4].status, SubagentStatus::Running);
    assert!(
        summaries
            .iter()
            .all(|item| item.description.starts_with("task "))
    );
}

#[tokio::test]
async fn list_scopes_propagate_bound_history_failure() {
    use tool_types::SubagentListScope;

    for scope in [SubagentListScope::Active, SubagentListScope::All] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let backend = ChannelBackend::for_session(tx, "parent")
            .with_history_list(|| async { Err(SubagentObservationError::Unavailable) });
        let call = tokio::spawn(async move {
            backend
                .list_subagents(
                    scope,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await
        });
        let request = match rx.recv().await.unwrap() {
            SubagentEvent::ListSubagents(request) => request,
            _ => panic!("expected ListSubagents event"),
        };
        request.respond_to.send(Vec::new()).unwrap();
        assert!(matches!(
            call.await.unwrap(),
            Err(SubagentObservationError::Unavailable)
        ));
    }
}

#[tokio::test]
async fn active_list_without_history_hook_uses_runtime_fixture() {
    use tool_types::{SubagentListScope, SubagentStatus};

    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent");
    let call = tokio::spawn(async move {
        backend
            .list_subagents(
                SubagentListScope::Active,
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            )
            .await
    });
    let request = match rx.recv().await.unwrap() {
        SubagentEvent::ListSubagents(request) => request,
        _ => panic!("expected ListSubagents event"),
    };
    request
        .respond_to
        .send(vec![listed_subagent("runtime", SubagentStatus::Running)])
        .unwrap();
    let summaries = call.await.unwrap().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].subagent_id, "runtime");
}

#[tokio::test]
async fn list_active_does_not_mask_invalid_or_unauthorized_history() {
    use tool_types::{SubagentListScope, SubagentStatus};

    for error in [
        SubagentObservationError::InvalidEvidence,
        SubagentObservationError::Unauthorized,
    ] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let backend = ChannelBackend::for_session(tx, "parent")
            .with_history_list(move || async move { Err(error) });
        let call = tokio::spawn(async move {
            backend
                .list_subagents(
                    SubagentListScope::Active,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await
        });
        let request = match rx.recv().await.unwrap() {
            SubagentEvent::ListSubagents(request) => request,
            _ => panic!("expected ListSubagents event"),
        };
        request
            .respond_to
            .send(vec![listed_subagent("runtime", SubagentStatus::Running)])
            .unwrap();
        assert!(matches!(call.await.unwrap(), Err(actual) if actual == error));
    }
}

#[tokio::test(start_paused = true)]
async fn list_subagents_history_and_runtime_share_one_observation_deadline() {
    use tool_types::SubagentListScope;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent").with_history_list(|| async {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        Ok(Vec::new())
    });
    let call = tokio::spawn(async move {
        backend
            .list_subagents(
                SubagentListScope::All,
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            )
            .await
    });
    let _request = match rx.recv().await.unwrap() {
        SubagentEvent::ListSubagents(request) => request,
        _ => panic!("expected ListSubagents event"),
    };
    assert!(matches!(
        call.await.unwrap(),
        Err(SubagentObservationError::TimedOut)
    ));
}

#[tokio::test(start_paused = true)]
async fn list_subagents_uses_the_callers_remaining_deadline() {
    use tool_types::{SubagentListScope, SubagentStatus};

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    tokio::time::advance(std::time::Duration::from_secs(4)).await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent").with_history_list(|| async {
        Ok(vec![listed_subagent(
            "durable-finished",
            SubagentStatus::Completed,
        )])
    });
    let call = tokio::spawn(async move {
        backend
            .list_subagents(SubagentListScope::All, deadline)
            .await
    });
    let _request = match rx.recv().await.unwrap() {
        SubagentEvent::ListSubagents(request) => request,
        _ => panic!("expected ListSubagents event"),
    };

    let summaries = call.await.unwrap().unwrap();
    assert_eq!(tokio::time::Instant::now(), deadline);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].subagent_id, "durable-finished");
    assert_eq!(summaries[0].status, SubagentStatus::Completed);
}

fn status_detail(id: &str, status: tool_types::SubagentStatus) -> tool_types::SubagentDetails {
    tool_types::SubagentDetails::from_summary(
        listed_subagent(id, status),
        tool_types::SubagentStatistics::default(),
    )
}

#[tokio::test]
async fn get_statuses_batches_runtime_and_falls_back_only_for_unresolved_ids() {
    use tool_types::{SubagentStatistics, SubagentStatus};

    let history_ids = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent").with_history_status({
        let history_ids = history_ids.clone();
        move |ids| {
            let history_ids = history_ids.clone();
            async move {
                *history_ids.lock() = ids.clone();
                Ok(vec![
                    status_detail("missing", SubagentStatus::Cancelled),
                    tool_types::SubagentDetails::unknown(
                        "forbidden".into(),
                        SubagentStatus::Unauthorized,
                        "not authorized".into(),
                    ),
                ])
            }
        }
    });
    let ids = ["running", "missing", "invalid", "forbidden"]
        .map(str::to_owned)
        .to_vec();
    let call = tokio::spawn({
        let backend = backend.clone();
        let ids = ids.clone();
        async move {
            backend
                .get_statuses(
                    &ids,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await
        }
    });
    let request = match rx.recv().await.unwrap() {
        SubagentEvent::GetStatuses(request) => request,
        _ => panic!("expected GetStatuses event"),
    };
    assert_eq!(request.parent_session_id.as_deref(), Some("parent"));
    assert_eq!(request.subagent_ids, ids);
    request
        .respond_to
        .send(vec![
            Ok(Some(tool_types::SubagentDetails::from_summary(
                listed_subagent("running", SubagentStatus::Running),
                SubagentStatistics {
                    tool_call_count: Some(7),
                    turn_count: Some(3),
                    ..Default::default()
                },
            ))),
            Err(SubagentObservationError::Unavailable),
            Err(SubagentObservationError::InvalidEvidence),
            Err(SubagentObservationError::Unauthorized),
        ])
        .unwrap();

    let details = call.await.unwrap().unwrap();
    assert_eq!(
        *history_ids.lock(),
        vec!["missing".to_string(), "forbidden".to_string()]
    );
    assert_eq!(details.len(), 4);
    assert_eq!(details[0].status, SubagentStatus::Running);
    assert_eq!(details[0].statistics.tool_call_count, Some(7));
    assert_eq!(details[1].status, SubagentStatus::Cancelled);
    assert_eq!(
        details[2].status,
        SubagentStatus::ObservationInvalidEvidence
    );
    assert_eq!(details[3].status, SubagentStatus::Unauthorized);
    assert!(details.iter().all(|item| item.subagent_id != "unrequested"));
}

#[tokio::test]
async fn get_statuses_history_failure_keeps_runtime_successes() {
    use tool_types::{SubagentStatistics, SubagentStatus};

    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent")
        .with_history_status(|_| async { Err(SubagentObservationError::InvalidEvidence) });
    let ids = vec!["known".to_owned(), "unknown".to_owned()];
    let call = tokio::spawn({
        let backend = backend.clone();
        let ids = ids.clone();
        async move {
            backend
                .get_statuses(
                    &ids,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await
        }
    });
    let request = match rx.recv().await.unwrap() {
        SubagentEvent::GetStatuses(request) => request,
        _ => panic!("expected GetStatuses event"),
    };
    request
        .respond_to
        .send(vec![
            Ok(Some(tool_types::SubagentDetails::from_summary(
                listed_subagent("known", SubagentStatus::Completed),
                SubagentStatistics::terminal(3, 2, 100),
            ))),
            Err(SubagentObservationError::Unavailable),
        ])
        .unwrap();
    let details = call.await.unwrap().unwrap();
    assert_eq!(details[0].status, SubagentStatus::Completed);
    assert_eq!(details[0].statistics.tool_call_count, Some(3));
    assert_eq!(
        details[1].status,
        SubagentStatus::ObservationInvalidEvidence
    );
}

#[tokio::test(start_paused = true)]
async fn get_statuses_history_fallback_uses_the_remaining_deadline() {
    use tool_types::{SubagentStatistics, SubagentStatus};

    let (tx, mut rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent").with_history_status(|_| async {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        Ok(Vec::new())
    });
    let ids = vec!["known".to_owned(), "unavailable".to_owned()];
    let call = tokio::spawn({
        let backend = backend.clone();
        let ids = ids.clone();
        async move {
            backend
                .get_statuses(
                    &ids,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await
        }
    });
    let request = match rx.recv().await.unwrap() {
        SubagentEvent::GetStatuses(request) => request,
        _ => panic!("expected GetStatuses event"),
    };
    request
        .respond_to
        .send(vec![
            Ok(Some(tool_types::SubagentDetails::from_summary(
                listed_subagent("known", SubagentStatus::Running),
                SubagentStatistics::default(),
            ))),
            Err(SubagentObservationError::Unavailable),
        ])
        .unwrap();
    let details = call.await.unwrap().unwrap();
    assert_eq!(details.len(), 2);
    assert_eq!(details[0].status, SubagentStatus::Running);
    assert_eq!(details[1].status, SubagentStatus::ObservationTimedOut);
}

#[tokio::test]
async fn get_statuses_requires_a_bound_source_even_with_history_hook() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let backend = ChannelBackend::new(tx).with_history_status(|_| async { Ok(Vec::new()) });
    assert!(matches!(
        backend
            .get_statuses(
                &["child".to_owned()],
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            )
            .await,
        Err(SubagentObservationError::Unauthorized)
    ));
}
