//! Regression coverage for settling historical Workflows under a live actor.

use super::*;
use crate::{
    NotificationEvent, NotificationPayloadRef, NotificationSource, NotificationSourceVersion,
    SubagentEvent, SubagentOutcome, SubagentSpawnEvent, SubagentTerminalEvent, Timeline,
    TimelineEventKind, TimelineWriteError, WorkflowEvent, WorkflowExecutionStatus,
    WorkflowTurnHandoff,
};

fn workflow_spawn(run_id: &str) -> TimelineEventKind {
    TimelineEventKind::Workflow(WorkflowEvent::Spawned {
        run_id: run_id.into(),
        execution_epoch: 0,
        name: "research".into(),
        objective: "settle only historical work".into(),
        script_hash: "0".repeat(64),
        args_hash: "0".repeat(64),
        initial_manifest: serde_json::json!({}),
    })
}

fn child_spawn() -> SubagentSpawnEvent {
    SubagentSpawnEvent {
        subagent_id: "historical-child".into(),
        child_session_id: "child-session".into(),
        origin_tool_call_id: None,
        security_parent_session_id: "test-session".into(),
        parent_step_id: None,
        subagent_type: "explore".into(),
        description: "inspect architecture".into(),
        prompt: "trace the canonical state".into(),
        execution_recipe: serde_json::json!({}),
        context_source: crate::SubagentContextSource::Forked,
        source_ref: None,
        context_normalized: false,
        resumed_from: None,
        parent_prompt_id: None,
        capability_mode: None,
        birth_mcp_servers: Some(Default::default()),
        permission_mode: None,
        effective_permission_mode: None,
        workflow_run_id: Some("wf_historical".into()),
        goal_id: None,
        goal_definition_revision: None,
        surface_completion: true,
        child_cwd: "/tmp".into(),
        worktree_path: None,
        effective_model_id: "test-model".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "test-model",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    }
}

fn reply_source() -> NotificationSource {
    NotificationSource::AgentReply {
        source_session_id: "other-session".into(),
        message_id: "reply-1".into(),
        reply_to: sampling_types::AgentMessageRef {
            source_session_id: "test-session".into(),
            message_id: "original-message".into(),
        },
    }
}

fn reply_input(receipt_id: String) -> ConversationItem {
    ConversationItem::received_agent_message(sampling_types::AgentMessage {
        receipt_id,
        source_session_id: "other-session".into(),
        target_session_id: "test-session".into(),
        message_id: "reply-1".into(),
        reply_to: match reply_source() {
            NotificationSource::AgentReply { reply_to, .. } => Some(reply_to),
            _ => unreachable!(),
        },
        message: "the requested evidence".into(),
    })
}

async fn start_notification_turn(handle: &crate::ChatStateHandle, turn: crate::TurnId) {
    handle
        .record_timeline_event_durably(TimelineEventKind::Turn(crate::TurnEvent::Started {
            id: turn,
            prompt_id: "notification-prompt".into(),
            input_ids: Vec::new(),
            identity: crate::TurnIdentity {
                origin: "notification_drain".into(),
                ..test_turn_identity()
            },
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index: 0,
            prompt_text: "notification available".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
}

fn assert_only_settlement(events: &[crate::TimelineEvent], run_id: &str) {
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        TimelineEventKind::Workflow(WorkflowEvent::Ended {
            run_id: actual,
            execution_epoch: 0,
            status: WorkflowExecutionStatus::Interrupted,
            execution_suspended: false,
            handoff: WorkflowTurnHandoff::Completion,
            message: Some(message),
            ..
        }) if actual == run_id && message == "process_interrupted"
    ));
}

#[tokio::test]
async fn durable_settle_interrupted_workflows_preserves_live_receipt_after_child_terminal() {
    let mut historical = Timeline::default();
    historical.record(workflow_spawn("wf_historical")).unwrap();
    let spawn = child_spawn();
    let spawn_event = historical
        .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            Box::new(spawn.clone()),
        )))
        .unwrap();
    let version = NotificationSourceVersion::Ordinal {
        value: crate::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let body = "the requested evidence";
    let receipt_id = crate::notification_id("test-session", &reply_source(), &version).unwrap();
    historical
        .record(TimelineEventKind::Notification(
            NotificationEvent::Received {
                id: receipt_id.clone(),
                owner_session_id: "test-session".into(),
                source: reply_source(),
                source_version: version.clone(),
                payload_ref: NotificationPayloadRef {
                    blake3: blake3::hash(body.as_bytes()).to_hex().to_string(),
                    bytes: body.len() as u64,
                },
            },
        ))
        .unwrap();
    let (handle, mut persistence_rx) = cold_actor_from_events(historical.events().to_vec()).await;
    let turn = crate::TurnId(1901);
    start_notification_turn(&handle, turn).await;
    handle
        .record_timeline_event_durably(workflow_spawn("wf_new"))
        .await
        .unwrap();
    let expected = vec![("wf_historical".into(), 0)];
    let before = handle.timeline_events().await.unwrap();
    persistence_rx.drain();
    assert!(
        handle
            .settle_interrupted_workflows_durably(expected.clone())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(handle.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(persistence_rx.drain().is_empty());

    // Verify the exact child result link before committing the parent terminal,
    // as the backend-aware reconciler does before granting settlement authority.
    let mut child = Timeline::default();
    child
        .record(TimelineEventKind::SubagentSeed(crate::SubagentSeedEvent {
            parent_timeline_id: "test-session".into(),
            parent_spawn_seq: spawn_event.seq.get(),
            subagent_id: spawn.subagent_id.clone(),
            security_parent_session_id: spawn.security_parent_session_id.clone(),
            context_source: spawn.context_source,
            source_ref: None,
            normalized: false,
        }))
        .unwrap();
    let result = child
        .record(TimelineEventKind::SubagentResult(
            crate::SubagentResultEvent {
                subagent_id: spawn.subagent_id.clone(),
                outcome: SubagentOutcome::Cancelled,
                duration_ms: 1,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                error: Some("backend reconciliation".into()),
                output_ref: None,
                planning_summary: None,
            },
        ))
        .unwrap();
    let terminal = SubagentTerminalEvent {
        subagent_id: spawn.subagent_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        outcome: SubagentOutcome::Cancelled,
        duration_ms: 1,
        tool_calls: 0,
        turns: 0,
        tokens_used: 0,
        error: Some("backend reconciliation".into()),
        result_ref: Some(crate::TimelineRangeRef {
            timeline_id: spawn.child_session_id.clone(),
            first_seq: result.seq.get(),
            last_seq: result.seq.get(),
        }),
        snapshot_ref: None,
    };
    child
        .validate_subagent_result_link("test-session", spawn_event.seq, &spawn, &terminal)
        .unwrap();
    handle
        .record_timeline_event_durably(TimelineEventKind::Subagent(SubagentEvent::Ended(terminal)))
        .await
        .unwrap();
    let before_settlement = handle.timeline_events().await.unwrap();
    persistence_rx.drain();
    let settled = handle
        .settle_interrupted_workflows_durably(expected.clone())
        .await
        .unwrap();
    assert_only_settlement(&settled, "wf_historical");
    let after_settlement = handle.timeline_events().await.unwrap();
    assert_eq!(after_settlement.len(), before_settlement.len() + 1);
    assert_eq!(
        serde_json::to_value(&after_settlement[..before_settlement.len()]).unwrap(),
        serde_json::to_value(before_settlement).unwrap()
    );
    let persisted: Vec<_> = persistence_rx
        .drain()
        .into_iter()
        .map(|record| match record {
            PersistenceRecord::Timeline(event) => *event,
            PersistenceRecord::Flush => panic!("unexpected flush"),
        })
        .collect();
    assert_eq!(
        serde_json::to_value(persisted).unwrap(),
        serde_json::to_value(&settled).unwrap()
    );
    let projected = Timeline::from_events(after_settlement).unwrap();
    assert_eq!(projected.active_turn(), Some(turn));
    assert!(projected.workflow_lifecycle("wf_new").unwrap().open);
    assert_eq!(projected.pending_notifications()[0].id, receipt_id);

    handle
        .record_timeline_event_durably(TimelineEventKind::Notification(
            NotificationEvent::Consumed {
                notification_ids: vec![receipt_id.clone()],
                turn,
                input: Some(reply_input(receipt_id)),
            },
        ))
        .await
        .expect("the exact pending reply remains consumable by its live NotificationDrain Turn");
    assert!(handle.pending_notifications().await.unwrap().is_empty());
    assert!(
        handle
            .settle_interrupted_workflows_durably(expected)
            .await
            .unwrap()
            .is_empty()
    );
    let replayed = Timeline::from_events(handle.timeline_events().await.unwrap()).unwrap();
    assert_eq!(
        replayed.consumed_notification_turn(&reply_source(), &version),
        Some(turn)
    );
    assert_eq!(replayed.active_turn(), Some(turn));
    handle
        .record_timeline_event_durably(TimelineEventKind::Turn(crate::TurnEvent::Ended {
            id: turn,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 0,
            terminal: crate::TurnTerminal {
                source: crate::TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }))
        .await
        .unwrap();
    assert!(!handle.is_closed());
}

#[tokio::test]
async fn durable_settle_interrupted_workflows_matches_epoch_and_is_idempotent() {
    let mut h = TestHarness::new();
    for id in [
        "wf_old",
        "wf_new_epoch",
        "wf_terminal",
        "wf_closed",
        "wf_unrelated",
    ] {
        h.handle
            .record_timeline_event_durably(workflow_spawn(id))
            .await
            .unwrap();
    }
    for id in ["wf_new_epoch", "wf_terminal", "wf_closed"] {
        h.handle
            .record_timeline_event_durably(TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id: id.into(),
                execution_epoch: 0,
                status: WorkflowExecutionStatus::Failed,
                execution_suspended: false,
                handoff: WorkflowTurnHandoff::Completion,
                duration_ms: 1,
                message: None,
            }))
            .await
            .unwrap();
    }
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Workflow(WorkflowEvent::Resumed {
            run_id: "wf_new_epoch".into(),
            execution_epoch: 1,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Workflow(WorkflowEvent::Closed {
            run_id: "wf_closed".into(),
            execution_epoch: 0,
            status: WorkflowExecutionStatus::Cancelled,
            handoff: WorkflowTurnHandoff::None,
            duration_ms: 1,
            message: None,
        }))
        .await
        .unwrap();
    let expected = vec![
        ("wf_old".into(), 0),
        ("wf_old".into(), 0),
        ("wf_absent".into(), 0),
        ("wf_new_epoch".into(), 0),
        ("wf_terminal".into(), 0),
        ("wf_closed".into(), 0),
    ];
    let before = h.handle.timeline_events().await.unwrap();
    h.drain_persistence();
    let settled = h
        .handle
        .settle_interrupted_workflows_durably(expected.clone())
        .await
        .unwrap();
    assert_only_settlement(&settled, "wf_old");
    let after = h.handle.timeline_events().await.unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        serde_json::to_value(&after[..before.len()]).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let timeline = Timeline::from_events(after.clone()).unwrap();
    assert!(timeline.workflow_lifecycle("wf_new_epoch").unwrap().open);
    assert!(timeline.workflow_lifecycle("wf_unrelated").unwrap().open);
    assert_eq!(
        timeline.workflow_lifecycle("wf_terminal").unwrap().status,
        Some(WorkflowExecutionStatus::Failed)
    );
    assert_eq!(
        timeline.workflow_lifecycle("wf_closed").unwrap().status,
        Some(WorkflowExecutionStatus::Cancelled)
    );
    assert_eq!(h.drain_persistence().len(), 1);
    assert!(
        h.handle
            .settle_interrupted_workflows_durably(expected)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        h.handle
            .settle_interrupted_workflows_durably(Vec::new())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(h.handle.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    assert!(h.drain_persistence().is_empty());
}

#[tokio::test]
async fn durable_settle_interrupted_workflows_preserves_runtime_checkpoint() {
    let (mut events, turn) = suspended_open_turn_events().await;
    let mut timeline = Timeline::from_events(events).unwrap();
    timeline.record(workflow_spawn("wf_checkpoint")).unwrap();
    events = timeline.events().to_vec();
    let (handle, mut persistence_rx) = cold_actor_from_events(events).await;
    let before = handle.timeline_events().await.unwrap();
    let runtime = handle.runtime_state().await.unwrap();
    persistence_rx.drain();
    assert!(
        handle
            .settle_interrupted_workflows_durably(vec![("wf_checkpoint".into(), 0)])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(handle.runtime_state().await.unwrap(), runtime);
    assert_eq!(
        serde_json::to_value(handle.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(
        Timeline::from_events(before.clone())
            .unwrap()
            .workflow_lifecycle("wf_checkpoint")
            .unwrap()
            .open
    );
    assert_original_turn_is_preserved(&before, turn);
    assert!(persistence_rx.drain().is_empty());
}

#[tokio::test]
async fn durable_settle_interrupted_workflows_waits_for_ack_and_retries_exact_event() {
    let mut h = TestHarness::with_manual_timeline_ack_after(Vec::new(), 1);
    h.handle
        .record_timeline_event_durably(workflow_spawn("wf_ack"))
        .await
        .unwrap();
    let before = h.handle.timeline_events().await.unwrap();
    h.drain_persistence();
    let write = {
        let handle = h.handle.clone();
        tokio::spawn(async move {
            handle
                .settle_interrupted_workflows_durably(vec![
                    ("wf_ack".into(), 0),
                    ("wf_ack".into(), 0),
                ])
                .await
        })
    };
    let first = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!write.is_finished());
    let pending = h.handle.pending_timeline_commit().unwrap();
    assert_only_settlement(std::slice::from_ref(&pending.event), "wf_ack");
    let first_records = h.drain_persistence();
    assert_eq!(first_records.len(), 1);
    first
        .send(Err(std::io::Error::other("transient settlement failure")))
        .unwrap();
    let retry = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!write.is_finished());
    assert_eq!(
        serde_json::to_value(h.handle.pending_timeline_commit().unwrap().event).unwrap(),
        serde_json::to_value(&pending.event).unwrap()
    );
    let retry_records = h.drain_persistence();
    assert_eq!(retry_records.len(), 1);
    assert_eq!(
        serde_json::to_value(match &retry_records[0] {
            PersistenceRecord::Timeline(event) => event,
            _ => panic!("expected retry"),
        })
        .unwrap(),
        serde_json::to_value(&pending.event).unwrap()
    );
    retry.send(Ok(())).unwrap();
    let settled = write.await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&settled).unwrap(),
        serde_json::to_value(vec![pending.event]).unwrap()
    );
    let after = h.handle.timeline_events().await.unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert!(h.handle.pending_timeline_commit().is_none());
    assert!(
        h.handle
            .settle_interrupted_workflows_durably(vec![("wf_ack".into(), 0)])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(h.drain_persistence().is_empty());
}

#[tokio::test]
async fn durable_settle_interrupted_workflows_replays_partial_success_from_durable_prefix() {
    let mut h = TestHarness::with_manual_timeline_ack_after(Vec::new(), 2);
    let expected: Vec<(String, u64)> =
        vec![("wf_batch_first".into(), 0), ("wf_batch_second".into(), 0)];
    for (run_id, _) in &expected {
        h.handle
            .record_timeline_event_durably(workflow_spawn(run_id))
            .await
            .unwrap();
    }
    let mut durable_prefix = h
        .drain_persistence()
        .into_iter()
        .map(|record| match record {
            PersistenceRecord::Timeline(event) => *event,
            PersistenceRecord::Flush => panic!("unexpected flush"),
        })
        .collect::<Vec<_>>();
    assert_eq!(durable_prefix.len(), 2);
    assert_eq!(
        serde_json::to_value(&durable_prefix).unwrap(),
        serde_json::to_value(h.handle.timeline_events().await.unwrap()).unwrap()
    );
    let write = {
        let handle = h.handle.clone();
        let expected = expected.clone();
        tokio::spawn(async move { handle.settle_interrupted_workflows_durably(expected).await })
    };
    let first_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!write.is_finished());
    let first_pending = h.handle.pending_timeline_commit().unwrap();
    assert_only_settlement(std::slice::from_ref(&first_pending.event), "wf_batch_first");
    let mut first_records = h.drain_persistence();
    assert_eq!(first_records.len(), 1);
    let Some(PersistenceRecord::Timeline(first_terminal)) = first_records.pop() else {
        panic!("expected the first Workflow terminal persistence attempt");
    };
    assert_eq!(
        serde_json::to_value(&first_terminal).unwrap(),
        serde_json::to_value(&first_pending.event).unwrap()
    );
    first_ack.send(Ok(())).unwrap();
    durable_prefix.push(*first_terminal);

    let second_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(
        !write.is_finished(),
        "the batch still awaits its second commit"
    );
    let second_pending = h.handle.pending_timeline_commit().unwrap();
    assert_only_settlement(
        std::slice::from_ref(&second_pending.event),
        "wf_batch_second",
    );
    assert_eq!(
        second_pending.event.seq.get() as usize,
        durable_prefix.len()
    );
    let mut second_records = h.drain_persistence();
    assert_eq!(second_records.len(), 1);
    let Some(PersistenceRecord::Timeline(second_terminal)) = second_records.pop() else {
        panic!("expected the second Workflow terminal persistence attempt");
    };
    assert_eq!(
        serde_json::to_value(&second_terminal).unwrap(),
        serde_json::to_value(&second_pending.event).unwrap()
    );
    second_ack
        .send(Err(std::io::Error::new(
            std::io::ErrorKind::StorageFull,
            "second Workflow terminal was not persisted",
        )))
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), write)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        result,
        Err(TimelineWriteError::Persistence(error))
            if error.kind() == std::io::ErrorKind::StorageFull
    ));
    let failed = h.handle.pending_timeline_commit().unwrap();
    assert_eq!(
        serde_json::to_value(&failed.event).unwrap(),
        serde_json::to_value(&second_pending.event).unwrap()
    );
    assert_eq!(failed.error_kind, Some(std::io::ErrorKind::StorageFull));
    assert!(!failed.acknowledgement_lost);
    assert!(h.handle.is_closed());
    assert!(h.drain_persistence().is_empty());

    // Retain only the persistence records whose ACKs succeeded. In particular,
    // the rejected second attempt must not become part of this durable prefix.
    let restored = Timeline::from_events(durable_prefix.clone()).unwrap();
    let first = restored.workflow_lifecycle("wf_batch_first").unwrap();
    assert_eq!(first.status, Some(WorkflowExecutionStatus::Interrupted));
    assert!(first.closed);
    assert!(restored.workflow_lifecycle("wf_batch_second").unwrap().open);
    assert!(
        !durable_prefix
            .iter()
            .any(|event| event.seq == failed.event.seq)
    );

    // Normal cold-load startup runs whole-session recovery first. Rebuild the
    // owner from the verified durable fold here so the same public settlement
    // command, rather than bootstrap recovery, must finish the remaining target.
    let state =
        crate::actor::state::ChatState::from_timeline(restored, test_config(), None).unwrap();
    let (persistence, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let replacement = ChatStateActor::launch(
        state,
        Box::new(persistence),
        Vec::new(),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    assert_eq!(
        serde_json::to_value(replacement.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(&durable_prefix).unwrap()
    );
    assert!(persistence_rx.drain().is_empty());
    let replay = {
        let handle = replacement.clone();
        let expected = expected.clone();
        tokio::spawn(async move { handle.settle_interrupted_workflows_durably(expected).await })
    };
    let replay_ack = persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!replay.is_finished());
    let replay_pending = replacement.pending_timeline_commit().unwrap();
    assert_only_settlement(
        std::slice::from_ref(&replay_pending.event),
        "wf_batch_second",
    );
    assert_eq!(replay_pending.event.seq, failed.event.seq);
    let mut replay_records = persistence_rx.drain();
    assert_eq!(
        replay_records.len(),
        1,
        "the first target must not be appended again"
    );
    let Some(PersistenceRecord::Timeline(replayed_terminal)) = replay_records.pop() else {
        panic!("expected only the remaining Workflow terminal persistence attempt");
    };
    assert_eq!(
        serde_json::to_value(&replayed_terminal).unwrap(),
        serde_json::to_value(&replay_pending.event).unwrap()
    );
    replay_ack.send(Ok(())).unwrap();
    let settled = replay.await.unwrap().unwrap();
    assert_only_settlement(&settled, "wf_batch_second");
    assert_eq!(
        serde_json::to_value(&settled[0]).unwrap(),
        serde_json::to_value(&replayed_terminal).unwrap()
    );
    durable_prefix.push(*replayed_terminal);
    assert_eq!(
        serde_json::to_value(replacement.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(&durable_prefix).unwrap()
    );
    let final_fold = Timeline::from_events(durable_prefix.clone()).unwrap();
    for (run_id, _) in &expected {
        assert_eq!(
            final_fold.workflow_lifecycle(run_id).unwrap().status,
            Some(WorkflowExecutionStatus::Interrupted)
        );
        assert_eq!(
            durable_prefix
                .iter()
                .filter(|event| matches!(
                    &event.kind,
                    TimelineEventKind::Workflow(WorkflowEvent::Ended { run_id: actual, .. })
                        if actual == run_id
                ))
                .count(),
            1,
            "each target must have exactly one canonical terminal"
        );
    }
    assert!(replacement.pending_timeline_commit().is_none());
    assert!(
        replacement
            .settle_interrupted_workflows_durably(expected)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(persistence_rx.drain().is_empty());
    assert_eq!(
        serde_json::to_value(replacement.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(durable_prefix).unwrap()
    );
}

#[tokio::test]
async fn durable_settle_interrupted_workflows_failure_keeps_uncommitted_identity() {
    let mut h = TestHarness::with_manual_timeline_ack_after(Vec::new(), 1);
    h.handle
        .record_timeline_event_durably(workflow_spawn("wf_failure"))
        .await
        .unwrap();
    let before = h.handle.timeline_events().await.unwrap();
    h.drain_persistence();
    let write = {
        let handle = h.handle.clone();
        tokio::spawn(async move {
            handle
                .settle_interrupted_workflows_durably(vec![
                    ("wf_failure".into(), 0),
                    ("wf_absent".into(), 0),
                ])
                .await
        })
    };
    let ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!write.is_finished());
    let pending = h.handle.pending_timeline_commit().unwrap();
    ack.send(Err(std::io::Error::new(
        std::io::ErrorKind::StorageFull,
        "settlement storage full",
    )))
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), write)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(result, Err(TimelineWriteError::Persistence(error)) if error.kind() == std::io::ErrorKind::StorageFull)
    );
    let failed = h.handle.pending_timeline_commit().unwrap();
    assert_eq!(
        serde_json::to_value(&failed.event).unwrap(),
        serde_json::to_value(&pending.event).unwrap()
    );
    assert_eq!(failed.error_kind, Some(std::io::ErrorKind::StorageFull));
    assert_eq!(failed.event.seq.get() as usize, before.len());
    assert_only_settlement(std::slice::from_ref(&failed.event), "wf_failure");
    assert!(h.handle.is_closed());
    assert!(matches!(
        h.handle
            .settle_interrupted_workflows_durably(vec![("wf_failure".into(), 0)])
            .await,
        Err(TimelineWriteError::AcknowledgementLost)
    ));
    let records = h.drain_persistence();
    assert_eq!(
        records.len(),
        1,
        "failed writes must not invent later commits"
    );
    assert!(
        Timeline::from_events(before)
            .unwrap()
            .workflow_lifecycle("wf_failure")
            .unwrap()
            .open
    );
}
