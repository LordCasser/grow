use super::*;
use crate::session::storage::JsonlStorageAdapter;
use crate::session::storage::StorageAdapter as _;

fn info(id: &str) -> crate::session::info::Info {
    crate::session::info::Info {
        id: acp::SessionId::new(id),
        cwd: "/workspace".into(),
    }
}

async fn init(
    storage: &JsonlStorageAdapter,
    info: &crate::session::info::Info,
    parent: Option<&str>,
    facts: Vec<chat_state::TimelineEventKind>,
) -> Vec<chat_state::TimelineEvent> {
    let mut summary = crate::session::persistence::Summary::new(
        info,
        crate::session::persistence::default_model_id(),
    )
    .unwrap();
    if let Some(parent) = parent {
        summary.parent_session_id = Some(parent.into());
        summary.session_kind = Some("subagent".into());
    }
    storage
        .init_session_with_summary(
            info,
            summary,
            Vec::new(),
            Default::default(),
            Default::default(),
            facts,
        )
        .await
        .unwrap()
        .1
}

fn seed(
    root: &str,
    spawn_seq: u64,
    spawn: &chat_state::SubagentSpawnEvent,
) -> chat_state::TimelineEventKind {
    chat_state::TimelineEventKind::SubagentSeed(chat_state::SubagentSeedEvent {
        parent_timeline_id: root.into(),
        parent_spawn_seq: spawn_seq,
        subagent_id: spawn.subagent_id.clone(),
        security_parent_session_id: spawn.security_parent_session_id.clone(),
        context_source: spawn.context_source,
        source_ref: spawn.source_ref.clone(),
        normalized: spawn.context_normalized,
    })
}

fn result(outcome: chat_state::SubagentOutcome) -> chat_state::TimelineEventKind {
    chat_state::TimelineEventKind::SubagentResult(chat_state::SubagentResultEvent {
        subagent_id: "cancelled".into(),
        outcome,
        duration_ms: 17,
        tool_calls: 2,
        turns: 1,
        tokens_used: 31,
        error: (outcome != chat_state::SubagentOutcome::Completed).then(|| "stopped".into()),
        output_ref: None,
        planning_summary: None,
    })
}

fn terminal(
    spawn: &chat_state::SubagentSpawnEvent,
    outcome: chat_state::SubagentOutcome,
    result_seq: Option<u64>,
) -> chat_state::TimelineEventKind {
    chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
        chat_state::SubagentTerminalEvent {
            subagent_id: spawn.subagent_id.clone(),
            child_session_id: spawn.child_session_id.clone(),
            outcome,
            duration_ms: 17,
            tool_calls: 2,
            turns: 1,
            tokens_used: 31,
            error: (outcome != chat_state::SubagentOutcome::Completed).then(|| "stopped".into()),
            result_ref: result_seq.map(|seq| chat_state::TimelineRangeRef {
                timeline_id: spawn.child_session_id.clone(),
                first_seq: seq,
                last_seq: seq,
            }),
            snapshot_ref: None,
        },
    ))
}

#[tokio::test]
async fn durable_history_observation_uses_authorized_parent_facts_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let storage = JsonlStorageAdapter::with_root(temp.path().to_path_buf());
    let root_info = info("history-root");
    let mut nested_spawn = recovery_spawn("nested", "history-nested");
    nested_spawn.security_parent_session_id = root_info.id.0.to_string();
    let mut cancelled_spawn = recovery_spawn("cancelled", "history-cancelled");
    cancelled_spawn.security_parent_session_id = "history-nested".into();
    cancelled_spawn.prompt = "p".repeat(17 * 1024 * 1024);
    let mut open_spawn = recovery_spawn("open", "history-open");
    open_spawn.security_parent_session_id = root_info.id.0.to_string();
    let mut workflow_spawn = recovery_spawn("workflow", "history-workflow");
    workflow_spawn.workflow_run_id = Some("run-1".into());
    let root_events = init(
        &storage,
        &root_info,
        None,
        std::iter::once(chat_state::TimelineEventKind::Workflow(
            chat_state::WorkflowEvent::Spawned {
                run_id: "run-1".into(),
                execution_epoch: 0,
                name: "owner".into(),
                objective: "hidden child".into(),
                script_hash: "0".repeat(64),
                args_hash: "0".repeat(64),
                initial_manifest: serde_json::json!({}),
            },
        ))
        .chain(
            [
                nested_spawn.clone(),
                cancelled_spawn.clone(),
                open_spawn.clone(),
                workflow_spawn.clone(),
            ]
            .into_iter()
            .map(|spawn| {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    Box::new(spawn),
                ))
            }),
        )
        .collect(),
    )
    .await;
    let seq_for = |id: &str| {
        root_events
            .iter()
            .find(|event| {
                matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn)
                    ) if spawn.subagent_id == id
                )
            })
            .unwrap()
            .seq
            .get()
    };
    let nested_info = info("history-nested");
    init(
        &storage,
        &nested_info,
        Some(root_info.id.0.as_ref()),
        vec![seed(
            root_info.id.0.as_ref(),
            seq_for("nested"),
            &nested_spawn,
        )],
    )
    .await;
    let child_info = info("history-cancelled");
    let child_events = init(
        &storage,
        &child_info,
        Some(root_info.id.0.as_ref()),
        vec![
            seed(
                root_info.id.0.as_ref(),
                seq_for("cancelled"),
                &cancelled_spawn,
            ),
            result(chat_state::SubagentOutcome::Cancelled),
        ],
    )
    .await;
    let result_seq = child_events.last().unwrap().seq.get();
    let mut parent = chat_state::Timeline::from_events(root_events.clone()).unwrap();
    let ended = parent
        .record(terminal(
            &cancelled_spawn,
            chat_state::SubagentOutcome::Cancelled,
            Some(result_seq),
        ))
        .unwrap();
    storage
        .append_timeline_event_durable(&root_info, &ended)
        .await
        .unwrap();
    // The Pager's production lifecycle reader must use required references,
    // even when unrelated parent prompt evidence exceeds diagnostic budgets.
    assert!(matches!(
        crate::extensions::session_admin::read_durable_subagent_lifecycle(
            &storage, child_info.id.0.to_string(), true,
        ).unwrap(),
        crate::extensions::session_admin::DurableSubagentLifecycle::Terminal { stop_reason, .. }
            if stop_reason == "cancelled"
    ));
    let foreign_info = info("history-foreign");
    init(&storage, &foreign_info, None, Vec::new()).await;

    let root_path = storage
        .open_session(&root_info)
        .unwrap()
        .directory()
        .display_path()
        .join(crate::session::storage::TIMELINE_FILE);
    let child_path = storage
        .open_session(&child_info)
        .unwrap()
        .directory()
        .display_path()
        .join(crate::session::storage::TIMELINE_FILE);
    let before = (
        std::fs::read(&root_path).unwrap(),
        std::fs::read(&child_path).unwrap(),
    );
    let read =
        |source: &str, id: &str| super::super::observation::read_history(&storage, source, id);
    let list = super::super::observation::read_history_list(&storage, "history-root", tool_types::SubagentListScope::All).unwrap();
    let ids = list.iter().map(|row| row.subagent_id.as_str()).collect::<std::collections::HashSet<_>>();
    assert_eq!(ids, std::collections::HashSet::from(["nested", "cancelled", "open"]));
    assert!(list.iter().any(|row| row.subagent_id == "cancelled" && row.status == tool_types::SubagentStatus::Cancelled));
    assert!(list.iter().any(|row| row.subagent_id == "open" && row.status == tool_types::SubagentStatus::ObservationUnavailable));
    let direct = super::super::observation::read_history_list(&storage, "history-nested", tool_types::SubagentListScope::All).unwrap();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].subagent_id, "cancelled");
    let value = serde_json::to_value(&list).unwrap();
    for row in value.as_array().unwrap() {
        for key in ["output", "tool_call_count", "tool_calls", "tools_used"] {
            assert!(row.get(key).is_none(), "inventory does not contain {key}");
        }
    }
    let details = super::super::observation::read_statuses(&storage, "history-nested", &["cancelled".into(), "open".into(), "workflow".into()]).unwrap();
    assert_eq!(details[0].status, tool_types::SubagentStatus::Cancelled);
    assert_eq!(details[0].statistics.tool_call_count, Some(2));
    assert_eq!(details[0].statistics.turn_count, Some(1));
    assert_eq!(details[0].statistics.total_tokens_used, Some(31));
    assert_eq!(details[1].status, tool_types::SubagentStatus::Unauthorized);
    assert_eq!(details[1].statistics.tool_call_count, None);
    assert_eq!(details[2].status, tool_types::SubagentStatus::Unauthorized);
    let cancelled = read("history-nested", "cancelled")
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        cancelled.status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    assert!(matches!(
        read("history-root", "cancelled")
            .unwrap()
            .unwrap()
            .unwrap()
            .status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    assert_eq!(
        read("history-nested", "open").unwrap().unwrap_err(),
        SubagentObservationError::Unauthorized
    );
    assert_eq!(
        read("history-root", "open").unwrap().unwrap_err(),
        SubagentObservationError::Unavailable
    );
    assert_eq!(
        read("history-root", "workflow").unwrap().unwrap_err(),
        SubagentObservationError::Unauthorized
    );
    assert_eq!(
        read("history-foreign", "cancelled").unwrap().unwrap_err(),
        SubagentObservationError::Unauthorized
    );
    assert_eq!(
        read("history-root", "missing").unwrap().unwrap_err(),
        SubagentObservationError::Unauthorized
    );
    assert_eq!(
        (
            std::fs::read(root_path).unwrap(),
            std::fs::read(child_path).unwrap()
        ),
        before
    );
}

#[tokio::test]
async fn durable_history_observation_validates_result_links_and_output() {
    for case in ["completed", "failed", "invalid-link"] {
        let temp = tempfile::tempdir().unwrap();
        let storage = JsonlStorageAdapter::with_root(temp.path().to_path_buf());
        let root = info("result-root");
        let mut spawn = recovery_spawn("cancelled", "result-child");
        spawn.security_parent_session_id = root.id.0.to_string();
        let root_events = init(
            &storage,
            &root,
            None,
            vec![chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(Box::new(spawn.clone())),
            )],
        )
        .await;
        let spawn_seq = root_events.last().unwrap().seq.get();
        let child = info("result-child");
        let child_events = init(
            &storage,
            &child,
            Some(root.id.0.as_ref()),
            vec![seed(root.id.0.as_ref(), spawn_seq, &spawn)],
        )
        .await;
        let opened = storage.open_session(&child).unwrap();
        let artifact = write_subagent_output(opened.directory(), "durable answer").unwrap();
        let outcome = if case == "failed" {
            chat_state::SubagentOutcome::Failed
        } else {
            chat_state::SubagentOutcome::Completed
        };
        let mut child_timeline = chat_state::Timeline::from_events(child_events).unwrap();
        let mut result_kind = result(outcome);
        if let chat_state::TimelineEventKind::SubagentResult(result) = &mut result_kind {
            result.output_ref = Some(artifact.timeline_ref.clone());
        }
        let result = child_timeline.record(result_kind).unwrap();
        storage
            .append_timeline_event_durable(&child, &result)
            .await
            .unwrap();
        let mut root_timeline = chat_state::Timeline::from_events(root_events).unwrap();
        let result_seq = if case == "invalid-link" {
            result.seq.get() - 1
        } else {
            result.seq.get()
        };
        let ended = root_timeline
            .record(terminal(&spawn, outcome, Some(result_seq)))
            .unwrap();
        storage
            .append_timeline_event_durable(&root, &ended)
            .await
            .unwrap();
        let query = || {
            super::super::observation::read_history(
                &storage,
                root.id.0.as_ref(),
                &spawn.subagent_id,
            )
        };
        if case == "invalid-link" {
            assert_eq!(query().unwrap_err().kind(), std::io::ErrorKind::InvalidData);
            let rows = super::super::observation::read_history_list(&storage, root.id.0.as_ref(), tool_types::SubagentListScope::All).unwrap();
            assert_eq!(rows[0].status, tool_types::SubagentStatus::ObservationInvalidEvidence);
            let active_merge_rows = super::super::observation::read_history_list(
                &storage,
                root.id.0.as_ref(),
                tool_types::SubagentListScope::Active,
            ).unwrap();
            assert!(active_merge_rows[0].status.is_terminal(), "active inventory uses the parent Ended fact to suppress the historical row without following its invalid child link");

            let error = crate::extensions::session_admin::read_durable_subagent_lifecycle(
                &storage, child.id.0.to_string(), true,
            ).err().expect("invalid result link must not be accepted as terminal");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

            continue;
        }
        let snapshot = query().unwrap().unwrap().unwrap();
        if case == "failed" {
            assert!(
                matches!(snapshot.status, SubagentSnapshotStatus::Failed { error } if error == "stopped")
            );
        } else {
            assert!(
                matches!(snapshot.status, SubagentSnapshotStatus::Completed { output, tool_calls: 2, turns: 1, tokens_used: 31, .. } if output == "durable answer")
            );
            let hash = artifact.timeline_ref.rsplit(':').next().unwrap();
            let path = opened
                .directory()
                .display_path()
                .join("artifacts/subagent-output")
                .join(format!("{hash}.json"));
            std::fs::remove_file(&path).unwrap();
            assert!(matches!(
                query().unwrap().unwrap().unwrap().status,
                SubagentSnapshotStatus::CompletedOutputUnavailable { .. }
            ));
            let rows = super::super::observation::read_history_list(&storage, root.id.0.as_ref(), tool_types::SubagentListScope::All).unwrap();
            assert_eq!(rows[0].status, tool_types::SubagentStatus::Completed, "listing must not depend on answer artifact availability");
            let detail = super::super::observation::read_statuses(&storage, root.id.0.as_ref(), &["cancelled".into()]).unwrap();
            assert_eq!(detail[0].status, tool_types::SubagentStatus::Completed);
            assert_eq!(detail[0].statistics.tool_call_count, Some(2), "details do not read missing answer artifacts");
            std::fs::write(path, b"corrupt").unwrap();
            assert!(matches!(
                query().unwrap().unwrap().unwrap().status,
                SubagentSnapshotStatus::CompletedOutputUnavailable { .. }
            ));
        }
    }
}
