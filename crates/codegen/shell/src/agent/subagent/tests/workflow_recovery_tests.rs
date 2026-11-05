use super::*;
use chat_state::{
    NotificationEvent, NotificationPayloadRef, NotificationSource, NotificationSourceVersion,
    Timeline, TimelineEventKind, TurnEvent, TurnId, TurnInputKind, WorkflowEvent,
    WorkflowExecutionStatus, WorkflowTurnHandoff,
};

fn workflow_spawn(run_id: &str) -> TimelineEventKind {
    TimelineEventKind::Workflow(WorkflowEvent::Spawned {
        run_id: run_id.into(),
        execution_epoch: 0,
        name: "recovery".into(),
        objective: "settle the captured execution".into(),
        script_hash: "0".repeat(64),
        args_hash: "0".repeat(64),
        initial_manifest: serde_json::json!({}),
    })
}

async fn start_notification_turn(
    parent: &chat_state::ChatStateHandle,
    parent_id: &str,
    turn: TurnId,
) -> sampling_types::AgentMessage {
    let reply_to = sampling_types::AgentMessageRef {
        source_session_id: parent_id.into(),
        message_id: "original-message".into(),
    };
    let body = "the exact reply remains available after child reconciliation";
    let receipt = parent
        .receive_notification_durably(
            parent_id.into(),
            NotificationSource::AgentReply {
                source_session_id: "reply-session".into(),
                message_id: "reply-message".into(),
                reply_to: reply_to.clone(),
            },
            NotificationSourceVersion::Ordinal {
                value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
            },
            NotificationPayloadRef {
                blake3: blake3::hash(body.as_bytes()).to_hex().to_string(),
                bytes: body.len() as u64,
            },
        )
        .await
        .unwrap();
    let TimelineEventKind::Notification(NotificationEvent::Received { id, .. }) = receipt.kind
    else {
        panic!("expected an AgentReply receipt");
    };
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    parent
        .record_timeline_event_durably(TimelineEventKind::Turn(TurnEvent::Started {
            id: turn,
            prompt_id: format!("notification-{}", turn.0),
            input_ids: Vec::new(),
            identity: crate::session::PromptOrigin::NotificationDrain
                .turn_identity(crate::session::TurnKind::Internal),
            model_id: "model".into(),
            input_message_count: timeline.surface_len(),
            prompt_index: 0,
            prompt_text: "notification available".into(),
            input_kind: TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    sampling_types::AgentMessage {
        receipt_id: id,
        source_session_id: "reply-session".into(),
        target_session_id: parent_id.into(),
        message_id: "reply-message".into(),
        reply_to: Some(reply_to),
        message: body.into(),
    }
}

async fn assert_notification_turn_consumable(
    parent: &chat_state::ChatStateHandle,
    turn: TurnId,
    message: sampling_types::AgentMessage,
) {
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert_eq!(timeline.active_turn(), Some(turn));
    let turn_correlation = turn.0.to_string();
    assert!(timeline.events().iter().all(|event| match &event.kind {
        TimelineEventKind::Turn(TurnEvent::Ended { id, .. }) => *id != turn,
        TimelineEventKind::Recovery(recovery) => {
            recovery.correlation_id.as_deref() != Some(turn_correlation.as_str())
        }
        _ => true,
    }));
    assert_eq!(timeline.pending_notifications().len(), 1);
    assert_eq!(timeline.pending_notifications()[0].id, message.receipt_id);
    let source = NotificationSource::AgentReply {
        source_session_id: message.source_session_id.clone(),
        message_id: message.message_id.clone(),
        reply_to: message.reply_to.clone().unwrap(),
    };
    let input = ConversationItem::received_agent_message(message.clone());
    let consumed = parent
        .record_timeline_event_durably(TimelineEventKind::Notification(
            NotificationEvent::Consumed {
                notification_ids: vec![message.receipt_id],
                turn,
                input: Some(input.clone()),
            },
        ))
        .await
        .expect("the original live Turn must durably consume the exact AgentMessage");
    let TimelineEventKind::Notification(NotificationEvent::Consumed {
        input: Some(actual),
        ..
    }) = consumed.kind
    else {
        panic!("expected the durable AgentMessage consumption fact");
    };
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(input).unwrap()
    );
    assert!(parent.pending_notifications().await.unwrap().is_empty());
    assert_eq!(
        parent
            .consumed_notification_turn(
                source,
                NotificationSourceVersion::Ordinal {
                    value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
                }
            )
            .await,
        Some(Some(turn))
    );
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert_eq!(timeline.active_turn(), Some(turn));
}

#[tokio::test]
async fn cold_reconciliation_preserves_live_notification_turn() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = "parent-live-notification";
    let mut loaded = Timeline::default();
    loaded.record(workflow_spawn("wf_historical")).unwrap();
    let historical_workflows = vec![(
        "wf_historical".into(),
        loaded
            .workflow_lifecycle("wf_historical")
            .unwrap()
            .execution_epoch,
    )];
    // This parent has no child entities, so the regression cannot depend on
    // missing child files or on a backend manufacturing a terminal.
    let (parent, mut persistence) = recovery_parent_from_timeline(parent_dir.path(), loaded).await;
    parent
        .record_timeline_event_durably(workflow_spawn("wf_live"))
        .await
        .unwrap();
    // Bootstrap has already settled an unblocked historical Workflow. The
    // captured target must now be a no-op while new actor work stays active.
    let bootstrapped = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert_eq!(
        bootstrapped
            .workflow_lifecycle("wf_historical")
            .unwrap()
            .status,
        Some(WorkflowExecutionStatus::Interrupted)
    );
    let turn = TurnId(901);
    let message = start_notification_turn(&parent, parent_id, turn).await;
    let before = serde_json::to_value(parent.timeline_events().await.unwrap()).unwrap();
    persistence.drain();
    let (backend_tx, mut backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx, parent_id,
    );
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &Default::default(),
        false,
        true,
        &historical_workflows,
        &backend,
        parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    assert!(backend_rx.try_recv().is_err());
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert_eq!(serde_json::to_value(timeline.events()).unwrap(), before);
    assert!(timeline.events().iter().any(|event| matches!(
        &event.kind,
        TimelineEventKind::Workflow(WorkflowEvent::Ended {
            run_id,
            execution_epoch: 0,
            status: WorkflowExecutionStatus::Interrupted,
            handoff: WorkflowTurnHandoff::Completion,
            execution_suspended: false,
            ..
        }) if run_id == "wf_historical"
    )));
    assert!(timeline.workflow_lifecycle("wf_live").unwrap().open);
    assert_notification_turn_consumable(&parent, turn, message).await;
    assert!(persistence.drain().iter().all(|record| !matches!(
        record,
        chat_state::PersistenceRecord::Timeline(event)
            if matches!(event.kind, TimelineEventKind::Recovery(_)
                | TimelineEventKind::Turn(TurnEvent::Ended { .. }))
    )));
}

#[tokio::test]
async fn resident_reconciliation_grants_no_workflow_settlement() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = "parent-resident-workflow";
    let (parent, _persistence) =
        recovery_parent_from_timeline(parent_dir.path(), Timeline::default()).await;
    // Resident work begins under this actor, after its cold bootstrap.
    parent
        .record_timeline_event_durably(workflow_spawn("wf_resident"))
        .await
        .unwrap();
    parent
        .record_timeline_event_durably(workflow_spawn("wf_live"))
        .await
        .unwrap();
    let turn = TurnId(902);
    let message = start_notification_turn(&parent, parent_id, turn).await;
    let before = serde_json::to_value(parent.timeline_events().await.unwrap()).unwrap();
    let (backend_tx, _backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx, parent_id,
    );
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &Default::default(),
        false,
        false,
        &[],
        &backend,
        parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    assert_eq!(
        serde_json::to_value(parent.timeline_events().await.unwrap()).unwrap(),
        before
    );
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert!(timeline.workflow_lifecycle("wf_resident").unwrap().open);
    assert!(timeline.workflow_lifecycle("wf_live").unwrap().open);
    assert_notification_turn_consumable(&parent, turn, message).await;
}

#[tokio::test]
#[serial_test::serial]
async fn cold_reconciliation_waits_for_canonical_child_then_settles_only_captured_epoch() {
    const CHILD_MARKER: &str = "GROW_WORKFLOW_RECOVERY_ISOLATED_HOME_CHILD";
    const TEST_NAME: &str = concat!(
        module_path!(),
        "::cold_reconciliation_waits_for_canonical_child_then_settles_only_captured_epoch"
    );
    if std::env::var_os(CHILD_MARKER).is_none() {
        // Production child discovery uses the process-cached GROW_HOME.
        let home = tempfile::tempdir().unwrap();
        let test_name = TEST_NAME.strip_prefix("shell::").unwrap_or(TEST_NAME);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env("GROW_HOME", home.path())
            .env(CHILD_MARKER, "1")
            .output()
            .expect("run Workflow recovery with isolated production child storage");
        assert!(
            output.status.success(),
            "isolated Workflow recovery failed\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    use crate::session::storage::StorageAdapter;

    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-workflow-{}", uuid::Uuid::now_v7());
    let mut spawn = recovery_spawn(
        "workflow-child",
        &format!("child-workflow-{}", uuid::Uuid::now_v7()),
    );
    spawn.security_parent_session_id = parent_id.clone();
    spawn.child_cwd = parent_dir.path().to_str().unwrap().into();
    spawn.workflow_run_id = Some("wf_historical".into());
    spawn.surface_completion = false;
    let mut loaded = Timeline::default();
    loaded.record(workflow_spawn("wf_historical")).unwrap();
    let spawned = loaded
        .record(TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(spawn.clone())),
        ))
        .unwrap();
    let historical_workflows = vec![(
        "wf_historical".into(),
        loaded
            .workflow_lifecycle("wf_historical")
            .unwrap()
            .execution_epoch,
    )];
    let (parent, _persistence) = recovery_parent_from_timeline(parent_dir.path(), loaded).await;
    let child_info = SessionInfo {
        id: acp::SessionId::new(spawn.child_session_id.clone()),
        cwd: spawn.child_cwd.clone(),
    };
    let child_dir = crate::session::persistence::session_dir(&child_info);
    std::fs::create_dir_all(&child_dir).unwrap();
    write_recovery_child(&child_dir, &parent_id, spawned.seq, &spawn);
    parent
        .record_timeline_event_durably(workflow_spawn("wf_live"))
        .await
        .unwrap();
    let turn = TurnId(903);
    let message = start_notification_turn(&parent, &parent_id, turn).await;
    let before_events = parent.timeline_events().await.unwrap();
    let before = before_events.len();
    let (backend_tx, mut backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx,
        parent_id.clone(),
    );
    let inspection_spawn = spawn.clone();
    let inspection_parent = parent_id.clone();
    let responder = tokio::spawn(async move {
        for running in [true, false] {
            let Some(SubagentEvent::Inspect(request)) = backend_rx.recv().await else {
                panic!("expected backend inspection");
            };
            let inspection = running.then(|| SubagentInspection {
                snapshot: SubagentSnapshot {
                    subagent_id: inspection_spawn.subagent_id.clone(),
                    description: inspection_spawn.description.clone(),
                    subagent_type: inspection_spawn.subagent_type.clone(),
                    status: SubagentSnapshotStatus::Running {
                        turn_count: 0,
                        tool_call_count: 0,
                        tokens_used: 0,
                        context_window_tokens: 10_000,
                        context_usage_pct: 0,
                        tools_used: Vec::new(),
                        error_count: 0,
                    },
                    started_at_epoch_ms: 1,
                    duration_ms: 1,
                },
                parent_session_id: inspection_parent.clone(),
                child_session_id: inspection_spawn.child_session_id.clone(),
                fork_parent_prompt_id: None,
                resumed_from: None,
            });
            request.respond_to.send(Ok(inspection)).unwrap();
        }
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &Default::default(),
        false,
        true,
        &historical_workflows,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(timeline.events()).unwrap(),
        serde_json::to_value(&before_events).unwrap()
    );
    assert!(timeline.workflow_lifecycle("wf_historical").unwrap().open);
    assert!(timeline.workflow_lifecycle("wf_live").unwrap().open);
    assert_eq!(timeline.active_turn(), Some(turn));

    // The second pass sees no backend runtime. Only this actual canonical
    // child result can authorize closing the parent spawn and its Workflow.
    let storage =
        crate::session::storage::JsonlStorageAdapter::with_explicit_session_dir(child_dir);
    storage.claim_recovery_writer(&child_info).unwrap();
    let opened = storage.open_session(&child_info).unwrap();
    let mut child = opened
        .validated_timeline(&spawn.child_session_id)
        .unwrap()
        .timeline;
    let result = child
        .record(TimelineEventKind::SubagentResult(
            chat_state::SubagentResultEvent {
                subagent_id: spawn.subagent_id.clone(),
                outcome: chat_state::SubagentOutcome::Completed,
                duration_ms: 12,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                error: None,
                output_ref: None,
                planning_summary: None,
            },
        ))
        .unwrap();
    storage
        .append_timeline_event_durable(&child_info, &result)
        .await
        .unwrap();
    drop(opened);
    drop(storage);

    reconcile_orphaned_subagents_with_backend(
        &Default::default(),
        false,
        true,
        &historical_workflows,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    responder.await.unwrap();
    let timeline = Timeline::from_events(parent.timeline_events().await.unwrap()).unwrap();
    assert_eq!(timeline.events().len(), before + 2);
    assert_eq!(
        serde_json::to_value(&timeline.events()[..before]).unwrap(),
        serde_json::to_value(&before_events).unwrap(),
        "historical facts and the live Turn prefix must remain unchanged"
    );
    let TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(terminal)) =
        &timeline.events()[before].kind
    else {
        panic!("expected the verified parent child terminal before Workflow settlement");
    };
    assert_eq!(terminal.outcome, chat_state::SubagentOutcome::Completed);
    assert_eq!(
        terminal.result_ref.as_ref().unwrap().first_seq,
        result.seq.get()
    );
    child
        .validate_subagent_result_link(&parent_id, spawned.seq, &spawn, terminal)
        .expect("the reconciler must retain the exact canonical child result link");
    assert!(matches!(
        &timeline.events()[before + 1].kind,
        TimelineEventKind::Workflow(WorkflowEvent::Ended {
            run_id,
            execution_epoch: 0,
            status: WorkflowExecutionStatus::Interrupted,
            handoff: WorkflowTurnHandoff::Completion,
            ..
        }) if run_id == "wf_historical"
    ));
    assert!(timeline.workflow_lifecycle("wf_live").unwrap().open);
    assert_notification_turn_consumable(&parent, turn, message).await;
    let after = serde_json::to_value(parent.timeline_events().await.unwrap()).unwrap();
    reconcile_orphaned_subagents_with_backend(
        &Default::default(),
        false,
        true,
        &historical_workflows,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    assert_eq!(
        serde_json::to_value(parent.timeline_events().await.unwrap()).unwrap(),
        after,
        "repeating the captured recovery targets must not append another terminal"
    );
}
