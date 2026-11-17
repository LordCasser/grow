use super::*;

use crate::extensions::notification::{
    SessionNotification as GrowNotification, SessionUpdate as GrowUpdate,
};
use crate::session::info::Info;
use crate::session::persistence::default_model_id;
use crate::session::storage::{JsonlStorageAdapter, SessionUpdate, StorageAdapter};
use acp_transport::protocol as acp;
use tempfile::TempDir;

fn info(id: &str, cwd: &str) -> Info {
    Info {
        id: acp::SessionId::new(id),
        cwd: cwd.into(),
    }
}

async fn init(adapter: &JsonlStorageAdapter, session: &Info) {
    adapter
        .init_session(session, default_model_id())
        .await
        .unwrap();
}

fn spawn(parent: &Info, child: &Info) -> chat_state::SubagentSpawnEvent {
    chat_state::SubagentSpawnEvent {
        subagent_id: "agent-child".into(),
        child_session_id: child.id.0.to_string(),
        origin_tool_call_id: Some("tool-child".into()),
        security_parent_session_id: parent.id.0.to_string(),
        parent_step_id: None,
        subagent_type: "explore".into(),
        description: "inspect the child workspace".into(),
        prompt: "Read the requested files".into(),
        execution_recipe: serde_json::json!({"mode": "test"}),
        context_source: chat_state::SubagentContextSource::New,
        source_ref: None,
        context_normalized: false,
        resumed_from: None,
        parent_prompt_id: None,
        parent_prompt_owner: None,
        capability_mode: None,
        birth_mcp_servers: Some(Default::default()),
        permission_mode: None,
        effective_permission_mode: None,
        workflow_run_id: None,
        goal_id: None,
        goal_definition_revision: None,
        surface_completion: true,
        child_cwd: child.cwd.clone(),
        worktree_path: Some("/worktrees/child".into()),
        effective_model_id: "test-model".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "test-model",
            "responses",
            "test",
        ),
        reasoning_effort: None,
    }
}

async fn add_canonical_spawn(
    adapter: &JsonlStorageAdapter,
    parent: &Info,
    child: &Info,
) -> chat_state::TimelineEvent {
    init(adapter, parent).await;
    init(adapter, child).await;
    let spawn = spawn(parent, child);
    let mut timeline = chat_state::Timeline::default();
    let event = timeline
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(spawn.clone())),
        ))
        .unwrap();
    adapter
        .append_timeline_event_durable(parent, &event)
        .await
        .unwrap();
    let seed = chat_state::SubagentSeedEvent {
        parent_timeline_id: parent.id.0.to_string(),
        parent_spawn_seq: event.seq.get(),
        subagent_id: spawn.subagent_id,
        security_parent_session_id: parent.id.0.to_string(),
        context_source: spawn.context_source,
        source_ref: spawn.source_ref,
        normalized: spawn.context_normalized,
    };
    let mut child_timeline = chat_state::Timeline::default();
    let child_seed = child_timeline
        .record(chat_state::TimelineEventKind::SubagentSeed(seed))
        .unwrap();
    adapter
        .append_timeline_event_durable(child, &child_seed)
        .await
        .unwrap();
    event
}

#[cfg(unix)]
#[tokio::test]
async fn settled_child_is_available_to_exact_queries_after_cold_load() {
    use chat_state::{ObservationEvent, SubagentEvent, SubagentOutcome, SubagentTerminalEvent};

    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("parent-root", "/workspace/root");
    let child = info("child-session", "/workspace/child");
    let spawn = add_canonical_spawn(&adapter, &parent, &child).await;
    let mut dense = chat_state::Timeline::from_events(vec![spawn.clone()]).unwrap();
    let terminal = dense
        .record(chat_state::TimelineEventKind::Subagent(
            SubagentEvent::Ended(SubagentTerminalEvent {
                subagent_id: "agent-child".into(),
                child_session_id: child.id.to_string(),
                outcome: SubagentOutcome::Cancelled,
                duration_ms: 10,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                result_ref: None,
                snapshot_ref: None,
                error: Some("cancelled".into()),
            }),
        ))
        .unwrap();
    adapter
        .append_update(
            &parent,
            &grow(
                &parent,
                "terminal-only-page",
                GrowUpdate::SubagentFinished {
                    subagent_id: "agent-child".into(),
                    child_session_id: child.id.0.to_string(),
                    status: "cancelled".into(),
                    error: Some("cancelled".into()),
                    tool_calls: 0,
                    turns: 0,
                    duration_ms: 10,
                    tokens_used: 0,
                    output: None,
                },
            ),
        )
        .await
        .unwrap();
    let publication = dense
        .record(chat_state::TimelineEventKind::Observation(
            ObservationEvent {
                scope: "shell".into(),
                name: "artifact".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!("x".repeat(1024 * 1024))),
            },
        ))
        .unwrap();
    for event in [&terminal, &publication] {
        adapter
            .append_timeline_event_durable(&parent, event)
            .await
            .unwrap();
    }
    drop(adapter);

    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let restored = adapter
        .load_session_for_write_with_indexed_history(&parent)
        .await
        .unwrap();
    assert!(restored.history.indexed.is_some());
    assert_eq!(restored.timeline.subagent_lifecycle_events().count(), 0);
    let (seq, operation, terminal) = restored
        .timeline
        .subagent_operation("agent-child")
        .unwrap()
        .unwrap();
    assert_eq!(seq, spawn.seq);
    assert_eq!(operation.child_session_id, child.id.to_string());
    assert_eq!(terminal.unwrap().outcome, SubagentOutcome::Cancelled);
    assert!(
        restored
            .timeline
            .subagent_operation("missing")
            .unwrap()
            .is_none()
    );

    // Indexed cold ownership omits settled spawn details, while the newest
    // replay page can still name this child. Resolve only that emitted id
    // against the exact indexed lifecycle before enriching Pager metadata.
    let mut ownership = DescendantOwnership::from_validated(
        parent.id.0.as_ref(),
        &restored.timeline,
        &restored.history,
    );
    assert!(!ownership.is_complete());
    assert!(!ownership.spawns.contains_key(child.id.0.as_ref()));
    let prepared = restored
        .history
        .prepare_indexed(&parent.id, &restored.timeline)
        .unwrap();
    let pages = restored.history.indexed_pages(prepared).unwrap().unwrap();
    let window = pages
        .read_before(&parent.id, crate::session::history::PagePosition::newest())
        .unwrap();
    let page_ownership = ownership
        .for_page(&window.rows, restored.history.indexed.as_ref())
        .unwrap()
        .unwrap();
    assert!(page_ownership.belongs_to(parent.id.0.as_ref()));
    let ended = page_ownership.spawns[child.id.0.as_ref()]
        .terminal
        .as_ref()
        .unwrap();
    assert_eq!(ended.outcome, SubagentOutcome::Cancelled);
    assert!(
        ended.result_ref.is_none(),
        "the parent terminal needs no child result for presentation"
    );
    let (_, page) = pages
        .page_before(&parent.id, crate::session::history::PagePosition::newest())
        .unwrap();
    let replayed_children = page
        .iter()
        .filter_map(|update| match update {
            SessionUpdate::Grow(notification) => match &notification.update {
                GrowUpdate::SubagentFinished {
                    subagent_id,
                    child_session_id,
                    ..
                } => Some((child_session_id.clone(), subagent_id.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        replayed_children.len(),
        1,
        "the newest page names the terminal child without its Spawned row"
    );
    ownership
        .include_replayed_children(&restored.timeline, &replayed_children)
        .unwrap();
    assert_eq!(
        ownership.replayed_child_ids().collect::<Vec<_>>(),
        vec![child.id.0.as_ref()]
    );
    let captured = &ownership.spawns[child.id.0.as_ref()];
    assert_eq!(captured.parent_spawn_seq, spawn.seq.get());
    assert_eq!(captured.spawn.subagent_id, "agent-child");
    assert_eq!(captured.spawn.child_session_id, child.id.0.as_ref());
}

#[tokio::test]
async fn page_ownership_survives_omitted_load_metadata_and_resolves_nested_ancestors() {
    use chat_state::{SubagentEvent, SubagentOutcome, SubagentTerminalEvent, TimelineEventKind};

    let root = info("parent-root", "/workspace/root");
    let child = info("child", "/workspace/child");
    let nested = info("nested", "/workspace/nested");
    let mut parent_spawn = spawn(&root, &child);
    parent_spawn.prompt = "x".repeat(1024 * 1024 + 1);
    let mut nested_spawn = spawn(&child, &nested);
    nested_spawn.subagent_id = "agent-nested".into();
    let mut timeline = chat_state::Timeline::default();
    for spawn in [parent_spawn, nested_spawn] {
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                Box::new(spawn),
            )))
            .unwrap();
    }
    let terminal = SubagentTerminalEvent {
        subagent_id: "agent-nested".into(),
        child_session_id: nested.id.0.to_string(),
        outcome: SubagentOutcome::Failed,
        duration_ms: 10,
        tool_calls: 0,
        turns: 0,
        tokens_used: 0,
        result_ref: None,
        snapshot_ref: None,
        error: Some("worktree could not be created".into()),
    };
    timeline
        .record(TimelineEventKind::Subagent(SubagentEvent::Ended(
            terminal.clone(),
        )))
        .unwrap();
    let history = HistorySnapshot {
        prefix_hash: "1".repeat(64),
        positions: vec![5],
        ..Default::default()
    };
    let ownership = DescendantOwnership::from_validated(root.id.0.as_ref(), &timeline, &history);
    assert!(ownership.clone().into_load_meta().is_none());
    let rows = [super::super::HistoryRow {
        source: super::super::HistorySourceKey { record: 4, part: 0 },
        entity: None,
        fragment: None,
        update: grow(
            &root,
            "nested-finished",
            crate::agent::subagent::finish_from_terminal(&terminal, None),
        ),
    }];
    let selected = ownership.for_page(&rows, None).unwrap().unwrap();
    assert!(selected.belongs_to(root.id.0.as_ref()));
    assert_eq!(
        selected.replayed_child_ids().collect::<Vec<_>>(),
        vec!["child", "nested"]
    );
    assert_eq!(
        selected.spawns["nested"].spawn.security_parent_session_id,
        "child"
    );
    assert_eq!(selected.spawns["nested"].terminal.as_ref(), Some(&terminal));
    assert!(selected.spawns["child"].spawn.prompt.is_empty());
}

#[tokio::test]
async fn delivered_child_is_settled_but_remains_exactly_queryable_after_cold_load() {
    use chat_state::{
        MessageCause, MessageEvent, NotificationEvent, NotificationOwner, NotificationPayloadRef,
        NotificationSource, NotificationSourceVersion, ObservationEvent, SubagentEvent,
        SubagentOutcome, SubagentResultEvent, SubagentTerminalEvent, SurfaceOp, TimelineEventKind,
        TurnEvent, TurnId, TurnIdentity, TurnInputKind,
    };

    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("delivered-parent", "/workspace/root");
    let child = info("delivered-child", "/workspace/child");
    let spawn = add_canonical_spawn(&adapter, &parent, &child).await;

    let mut child_timeline = chat_state::Timeline::default();
    child_timeline
        .record(TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: parent.id.0.to_string(),
                parent_spawn_seq: spawn.seq.get(),
                subagent_id: "agent-child".into(),
                security_parent_session_id: parent.id.0.to_string(),
                context_source: chat_state::SubagentContextSource::New,
                source_ref: None,
                normalized: false,
            },
        ))
        .unwrap();
    let child_publication = child_timeline
        .record(TimelineEventKind::Observation(ObservationEvent {
            scope: "shell".into(),
            name: "artifact".into(),
            turn: None,
            step: None,
            data: Some(serde_json::json!("publish child seed".repeat(65_536))),
        }))
        .unwrap();
    adapter
        .append_timeline_event_durable(&child, &child_publication)
        .await
        .unwrap();
    let child_result = child_timeline
        .record(TimelineEventKind::SubagentResult(SubagentResultEvent {
            subagent_id: "agent-child".into(),
            outcome: SubagentOutcome::Completed,
            duration_ms: 10,
            tool_calls: 0,
            turns: 1,
            tokens_used: 12,
            error: None,
            output_ref: None,
            planning_summary: None,
        }))
        .unwrap();
    adapter
        .append_timeline_event_durable(&child, &child_result)
        .await
        .unwrap();

    let mut dense = chat_state::Timeline::from_events(vec![spawn]).unwrap();
    let terminal = dense
        .record(TimelineEventKind::Subagent(SubagentEvent::Ended(
            SubagentTerminalEvent {
                subagent_id: "agent-child".into(),
                child_session_id: child.id.to_string(),
                outcome: SubagentOutcome::Completed,
                duration_ms: 10,
                tool_calls: 0,
                turns: 1,
                tokens_used: 12,
                result_ref: Some(chat_state::TimelineRangeRef {
                    timeline_id: child.id.to_string(),
                    first_seq: child_result.seq.get(),
                    last_seq: child_result.seq.get(),
                }),
                snapshot_ref: None,
                error: None,
            },
        )))
        .unwrap();
    let usage_settlement = dense
        .record(TimelineEventKind::Observation(ObservationEvent {
            scope: "session_usage".into(),
            name: "subagent_settled".into(),
            turn: None,
            step: None,
            data: Some(serde_json::json!({
                "subagent_id": "agent-child",
                "by_model": [["test-model", {
                    "input_tokens": 3,
                    "output_tokens": 2,
                    "model_calls": 1,
                    "cost_usd_ticks": null,
                    "cached_read_tokens": 0,
                    "cache_creation_tokens": 0,
                    "cache_read_known_input_tokens": 0,
                    "cache_read_unknown_calls": 0,
                    "reasoning_tokens": 0,
                    "api_duration_ms": 0,
                    "cost_missing_calls": 0,
                    "cache_write_unknown_calls": 0
                }]],
                "incomplete": false
            })),
        }))
        .unwrap();
    let source = NotificationSource::SubagentCompleted {
        subagent_id: "agent-child".into(),
        owner: NotificationOwner::Session,
    };
    let source_version = NotificationSourceVersion::Ordinal { value: 1 };
    let receipt_id =
        chat_state::notification_id(parent.id.0.as_ref(), &source, &source_version).unwrap();
    let receipt = dense
        .record(TimelineEventKind::Notification(
            NotificationEvent::Received {
                id: receipt_id,
                owner_session_id: parent.id.0.to_string(),
                source: source.clone(),
                source_version: source_version.clone(),
                payload_ref: NotificationPayloadRef {
                    blake3: blake3::hash(b"completion").to_hex().to_string(),
                    bytes: 10,
                },
            },
        ))
        .unwrap();
    let turn = TurnId(7);
    let turn_start = dense
        .record(TimelineEventKind::Turn(TurnEvent::Started {
            prompt_id: "delivery-prompt".into(),
            id: turn,
            input_ids: Vec::new(),
            identity: TurnIdentity {
                origin: "tool_result".into(),
                turn_kind: "internal".into(),
                goal_id: None,
                goal_definition_revision: None,
                stage_id: None,
            },
            model_id: "test-model".into(),
            input_message_count: dense.surface_len(),
            prompt_index: 0,
            prompt_text: "deliver child result".into(),
            input_kind: TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .unwrap();
    let delivery = dense
        .record(TimelineEventKind::Messages(MessageEvent {
            cause: MessageCause::ToolResult,
            items: vec![sampling_types::conversation::ConversationItem::ToolResult(
                sampling_types::conversation::ToolResultItem {
                    tool_call_id: "get-subagent-result".into(),
                    content: "child result".into(),
                    images: Vec::new(),
                    delivered_subagent_id: Some("agent-child".into()),
                },
            )],
            surface: SurfaceOp::Append,
            response_admission: None,
        }))
        .unwrap();
    assert_eq!(
        dense.consumed_notification_turn(&source, &source_version),
        Some(turn)
    );
    assert!(dense.pending_notifications().is_empty());

    let publication = dense
        .record(TimelineEventKind::Observation(ObservationEvent {
            scope: "shell".into(),
            name: "artifact".into(),
            turn: None,
            step: None,
            data: Some(serde_json::json!("x".repeat(1024 * 1024))),
        }))
        .unwrap();
    for event in [
        &terminal,
        &usage_settlement,
        &receipt,
        &turn_start,
        &delivery,
        &publication,
    ] {
        adapter
            .append_timeline_event_durable(&parent, event)
            .await
            .unwrap();
    }
    drop(adapter);

    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let restored = adapter
        .load_session_for_write_with_indexed_history(&parent)
        .await
        .unwrap()
        .timeline;
    assert!(restored.subagent_lifecycle_events().count() == 0);
    let (spawn_seq, operation, terminal) =
        restored.subagent_operation("agent-child").unwrap().unwrap();
    assert_eq!(
        spawn_seq,
        dense.subagent_operation("agent-child").unwrap().unwrap().0
    );
    assert_eq!(operation.child_session_id, child.id.to_string());
    let terminal = terminal.unwrap();
    assert_eq!(terminal.outcome, SubagentOutcome::Completed);
    let opened_child = adapter.open_session(&child).unwrap();
    assert_eq!(
        opened_child
            .validated_child_result(parent.id.0.as_ref(), spawn_seq, &operation, &terminal)
            .unwrap()
            .tokens_used,
        12
    );
    let mut conflicting = terminal.clone();
    conflicting.tokens_used = 13;
    assert!(
        opened_child
            .validated_child_result(parent.id.0.as_ref(), spawn_seq, &operation, &conflicting)
            .is_err()
    );
    let expected_receipt_id =
        chat_state::notification_id(parent.id.0.as_ref(), &source, &source_version).unwrap();
    assert!(
        matches!(&restored.exact_notification(&source, &source_version).unwrap().unwrap().kind,
            TimelineEventKind::Notification(NotificationEvent::Received { id, .. }) if id == &expected_receipt_id
        )
    );
    assert!(restored.pending_notifications().is_empty());
    assert_eq!(
        restored.consumed_notification_turn(&source, &source_version),
        Some(turn)
    );
}

fn grow(parent: &Info, event_id: &str, update: GrowUpdate) -> SessionUpdate {
    let mut meta = serde_json::Map::new();
    meta.insert("eventId".into(), serde_json::Value::String(event_id.into()));
    SessionUpdate::Grow(Box::new(GrowNotification {
        session_id: parent.id.clone(),
        update,
        meta: Some(serde_json::Value::Object(meta)),
    }))
}

#[cfg(unix)]
#[tokio::test]
async fn indexed_child_replay_keeps_exact_nested_ownership_and_lifecycle_ordered() {
    use chat_state::{ObservationEvent, SubagentEvent, TimelineEventKind};

    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("indexed-replay-parent", "/workspace/root");
    let child = info("indexed-replay-child", "/workspace/child");
    let grandchild = info("indexed-replay-grandchild", "/workspace/grandchild");
    let owner_spawn_event = add_canonical_spawn(&adapter, &parent, &child).await;
    let owner_spawn = ChildReplaySpawn {
        spawn: spawn(&parent, &child),
        parent_spawn_seq: owner_spawn_event.seq.get(),
        terminal: None,
    };

    let child_opened = adapter.open_session(&child).unwrap();
    let mut child_summary = child_opened.summary().clone();
    child_summary.parent_session_id = Some(parent.id.0.to_string());
    child_summary.session_kind = Some("subagent_explore".into());
    let summary_path = child_opened.directory().display_path().join("summary.json");
    drop(child_opened);
    std::fs::write(
        &summary_path,
        serde_json::to_vec_pretty(&child_summary).unwrap(),
    )
    .unwrap();

    init(&adapter, &grandchild).await;
    let mut child_timeline = chat_state::Timeline::default();
    child_timeline
        .record(TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: parent.id.0.to_string(),
                parent_spawn_seq: owner_spawn_event.seq.get(),
                subagent_id: "agent-child".into(),
                security_parent_session_id: parent.id.0.to_string(),
                context_source: chat_state::SubagentContextSource::New,
                source_ref: None,
                normalized: false,
            },
        ))
        .unwrap();
    let nested_spawn = spawn(&child, &grandchild);
    let nested_spawn_event = child_timeline
        .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            Box::new(nested_spawn.clone()),
        )))
        .unwrap();
    adapter
        .append_timeline_event_durable(&child, &nested_spawn_event)
        .await
        .unwrap();
    use tool_types::planning::{LocalPlan, PlanPriority, PlanStep, PlanStepStatus, PlanUpdateMode};
    let mut plan = LocalPlan::new(child.id.to_string(), "child planning");
    plan.update_steps(
        PlanUpdateMode::Merge,
        vec![PlanStep {
            id: "step".into(),
            content: "persisted child TODO".into(),
            priority: PlanPriority::Normal,
            status: PlanStepStatus::Pending,
            requirements: None,
            removed: false,
        }],
    )
    .unwrap();
    let plan_event = child_timeline
        .record(TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: None,
                plan: Box::new(plan),
            },
        ))
        .unwrap();
    adapter
        .append_timeline_event_durable(&child, &plan_event)
        .await
        .unwrap();
    let mut grandchild_timeline = chat_state::Timeline::default();
    let grandchild_seed = grandchild_timeline
        .record(TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: child.id.0.to_string(),
                parent_spawn_seq: nested_spawn_event.seq.get(),
                subagent_id: nested_spawn.subagent_id.clone(),
                security_parent_session_id: child.id.0.to_string(),
                context_source: nested_spawn.context_source.clone(),
                source_ref: nested_spawn.source_ref.clone(),
                normalized: nested_spawn.context_normalized,
            },
        ))
        .unwrap();
    adapter
        .append_timeline_event_durable(&grandchild, &grandchild_seed)
        .await
        .unwrap();

    let child_spawn_update = GrowUpdate::SubagentSpawned {
        subagent_id: nested_spawn.subagent_id.clone(),
        parent_session_id: child.id.0.to_string(),
        parent_prompt_id: None,
        child_session_id: grandchild.id.0.to_string(),
        subagent_type: "explore".into(),
        description: nested_spawn.description.clone(),
        effective_context_source: None,
        context_normalized: nested_spawn.context_normalized,
        capability_mode: None,
        permission_mode: None,
        effective_permission_mode: None,
        model: None,
        reasoning_effort: None,
        model_state: None,
        workflow_agent_names: None,
        resumed_from: None,
        workflow_run_id: None,
        goal_id: None,
    };
    adapter
        .append_update(&child, &grow(&child, "child-spawn", child_spawn_update))
        .await
        .unwrap();
    adapter
        .append_update(
            &child,
            &grow(
                &child,
                "child-progress",
                GrowUpdate::SubagentProgress {
                    subagent_id: nested_spawn.subagent_id.clone(),
                    parent_session_id: child.id.0.to_string(),
                    child_session_id: grandchild.id.0.to_string(),
                    duration_ms: 20,
                    turn_count: 1,
                    tool_call_count: 2,
                    tokens_used: 3,
                    context_window_tokens: 100,
                    context_usage_pct: 3,
                    tools_used: vec!["read".into()],
                    error_count: 0,
                },
            ),
        )
        .await
        .unwrap();
    adapter
        .append_update(
            &child,
            &grow(
                &child,
                "child-finish",
                GrowUpdate::SubagentFinished {
                    subagent_id: nested_spawn.subagent_id.clone(),
                    child_session_id: grandchild.id.0.to_string(),
                    status: "completed".into(),
                    error: None,
                    tool_calls: 2,
                    turns: 1,
                    duration_ms: 21,
                    tokens_used: 3,
                    output: Some("done".into()),
                },
            ),
        )
        .await
        .unwrap();
    let publication = child_timeline
        .record(TimelineEventKind::Observation(ObservationEvent {
            scope: "shell".into(),
            name: "artifact".into(),
            turn: None,
            step: None,
            data: Some(serde_json::json!("x".repeat(1024 * 1024))),
        }))
        .unwrap();
    adapter
        .append_timeline_event_durable(&child, &publication)
        .await
        .unwrap();
    drop(adapter);

    let publisher = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let loaded = publisher
        .load_session_for_write_with_indexed_history(&child)
        .await
        .unwrap();
    assert!(loaded.history.indexed.is_some());
    let indexed = loaded.history.indexed.as_ref().unwrap();
    assert!(indexed.has_child_presentation());
    let mut reader = indexed.page_reader().unwrap();
    let indexed_lifecycle = reader
        .read_subagent_lifecycle(&child.id, &loaded.history)
        .unwrap();
    assert_eq!(indexed_lifecycle.len(), 3);
    assert!(matches!(
        &indexed_lifecycle[0].update,
        GrowUpdate::SubagentSpawned { .. }
    ));
    assert!(matches!(
        &indexed_lifecycle[1].update,
        GrowUpdate::SubagentProgress { .. }
    ));
    assert!(matches!(
        &indexed_lifecycle[2].update,
        GrowUpdate::SubagentFinished { .. }
    ));
    drop(loaded);
    drop(publisher);

    let replay = load_descendant_child_replay_at(
        child.id.0.as_ref(),
        parent.id.0.as_ref(),
        &owner_spawn,
        root.path(),
    )
    .unwrap()
    .unwrap();
    assert!(replay.ownership.is_complete());
    assert_eq!(
        replay.plan.unwrap().entries[0].content,
        "persisted child TODO"
    );
    assert!(
        replay
            .ownership
            .into_spawns()
            .contains_key(grandchild.id.0.as_ref())
    );
    let lifecycle = replay
        .lifecycle
        .iter()
        .filter_map(|notification| match &notification.update {
            GrowUpdate::SubagentSpawned { .. } => Some("spawned"),
            GrowUpdate::SubagentProgress { .. } => Some("progress"),
            GrowUpdate::SubagentFinished { .. } => Some("finished"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lifecycle, vec!["spawned", "progress", "finished"]);
}

#[tokio::test]
async fn empty_session_is_distinct_from_missing_session() {
    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let empty = info("empty-root", "/workspace/empty");
    init(&adapter, &empty).await;

    assert!(
        load_descendant_ownership_at("missing-root", root.path())
            .unwrap()
            .is_none()
    );
    let ownership = load_descendant_ownership_at("empty-root", root.path())
        .unwrap()
        .expect("an initialized empty session has an empty ownership result");
    assert!(ownership.belongs_to("empty-root"));
    assert!(ownership.into_spawns().is_empty());
    let (ownership, lifecycle) = load_descendant_replay_at("empty-root", root.path())
        .unwrap()
        .unwrap();
    assert!(ownership.belongs_to("empty-root"));
    assert!(lifecycle.is_empty());
}

#[tokio::test]
async fn only_canonical_domain_spawn_grants_compact_child_ownership() {
    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("parent-root", "/workspace/root");
    let child = info("child-session", "/workspace/child");
    add_canonical_spawn(&adapter, &parent, &child).await;

    let ownership = load_descendant_ownership_at(parent.id.0.as_ref(), root.path())
        .unwrap()
        .unwrap();
    let spawns = ownership.into_spawns();
    let compact = spawns.get(child.id.0.as_ref()).unwrap();
    assert_eq!(compact.spawn.subagent_id, "agent-child");
    assert_eq!(compact.spawn.prompt, "Read the requested files");
    assert_eq!(compact.spawn.child_cwd, child.cwd);
    assert_eq!(
        compact.spawn.worktree_path.as_deref(),
        Some("/worktrees/child")
    );
    let encoded = serde_json::to_value(compact).unwrap();
    assert_eq!(
        encoded["spawn"]["execution_recipe"],
        serde_json::json!({
            "mode": "test"
        })
    );
    assert_eq!(encoded["spawn"]["origin_tool_call_id"], "tool-child");

    // A UI-only spawn remains visible in the ordered projection, but has no
    // authority to create an ownership edge.
    let ui_only_parent = info("ui-only-root", "/workspace/ui-only");
    init(&adapter, &ui_only_parent).await;
    adapter
        .append_update(
            &ui_only_parent,
            &grow(
                &ui_only_parent,
                "ui-spawn-1",
                GrowUpdate::SubagentSpawned {
                    subagent_id: "display-only-agent".into(),
                    parent_session_id: ui_only_parent.id.0.to_string(),
                    parent_prompt_id: None,
                    child_session_id: "display-only-child".into(),
                    subagent_type: "explore".into(),
                    description: "displayed only".into(),
                    effective_context_source: None,
                    context_normalized: false,
                    capability_mode: None,
                    permission_mode: None,
                    effective_permission_mode: None,
                    model: None,
                    reasoning_effort: None,
                    model_state: None,
                    workflow_agent_names: None,
                    resumed_from: None,
                    workflow_run_id: None,
                    goal_id: None,
                },
            ),
        )
        .await
        .unwrap();
    let (ownership, lifecycle) =
        load_descendant_replay_at(ui_only_parent.id.0.as_ref(), root.path())
            .unwrap()
            .unwrap();
    assert!(ownership.into_spawns().is_empty());
    assert_eq!(lifecycle.len(), 1);
    assert!(matches!(
        &lifecycle[0].update,
        GrowUpdate::SubagentSpawned { .. }
    ));
}

#[tokio::test]
async fn ownership_payload_is_bound_to_session_version_and_valid_prefix_hash() {
    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("payload-root", "/workspace/root");
    let child = info("payload-child", "/workspace/child");
    add_canonical_spawn(&adapter, &parent, &child).await;
    let ownership = load_descendant_ownership_at(parent.id.0.as_ref(), root.path())
        .unwrap()
        .unwrap();
    assert!(ownership.belongs_to(parent.id.0.as_ref()));
    assert!(ownership.is_complete());
    assert!(!ownership.belongs_to("foreign-root"));

    let mut wire = serde_json::to_value(&ownership).unwrap();
    wire["version"] = serde_json::json!(3);
    let wrong_version: DescendantOwnership = serde_json::from_value(wire).unwrap();
    assert!(!wrong_version.belongs_to(parent.id.0.as_ref()));

    let mut wire = serde_json::to_value(&ownership).unwrap();
    wire["hash"] = serde_json::json!("not-a-prefix-hash");
    let wrong_hash: DescendantOwnership = serde_json::from_value(wire).unwrap();
    assert!(!wrong_hash.belongs_to(parent.id.0.as_ref()));

    let mut wire = serde_json::to_value(&ownership).unwrap();
    wire["replayed_children"] = serde_json::json!(["unowned-child"]);
    let wrong_page_membership: DescendantOwnership = serde_json::from_value(wire).unwrap();
    assert!(!wrong_page_membership.belongs_to(parent.id.0.as_ref()));

    let mut wire = serde_json::to_value(&ownership).unwrap();
    wire["session_id"] = serde_json::json!("foreign-root");
    let wrong_session: DescendantOwnership = serde_json::from_value(wire).unwrap();
    assert!(!wrong_session.belongs_to(parent.id.0.as_ref()));
}

#[tokio::test]
async fn replay_preserves_lifecycle_order_and_is_a_captured_snapshot() {
    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("ordered-root", "/workspace/root");
    let child = info("ordered-child", "/workspace/child");
    add_canonical_spawn(&adapter, &parent, &child).await;
    let progress = GrowUpdate::SubagentProgress {
        subagent_id: "agent-child".into(),
        parent_session_id: parent.id.0.to_string(),
        child_session_id: child.id.0.to_string(),
        duration_ms: 20,
        turn_count: 1,
        tool_call_count: 2,
        tokens_used: 3,
        context_window_tokens: 100,
        context_usage_pct: 3,
        tools_used: vec!["read".into()],
        error_count: 0,
    };
    adapter
        .append_update(&parent, &grow(&parent, "progress-1", progress))
        .await
        .unwrap();
    let finish = GrowUpdate::SubagentFinished {
        subagent_id: "agent-child".into(),
        child_session_id: child.id.0.to_string(),
        status: "completed".into(),
        error: None,
        tool_calls: 2,
        turns: 1,
        duration_ms: 21,
        tokens_used: 3,
        output: Some("done".into()),
    };
    adapter
        .append_update(&parent, &grow(&parent, "finish-1", finish))
        .await
        .unwrap();

    let (old_ownership, old_lifecycle) =
        load_descendant_replay_at(parent.id.0.as_ref(), root.path())
            .unwrap()
            .unwrap();
    assert_eq!(old_lifecycle.len(), 2);
    assert!(matches!(
        &old_lifecycle[0].update,
        GrowUpdate::SubagentProgress { .. }
    ));
    assert!(matches!(
        &old_lifecycle[1].update,
        GrowUpdate::SubagentFinished { .. }
    ));

    adapter
        .append_update(
            &parent,
            &grow(
                &parent,
                "progress-2",
                GrowUpdate::SubagentProgress {
                    subagent_id: "agent-child".into(),
                    parent_session_id: parent.id.0.to_string(),
                    child_session_id: child.id.0.to_string(),
                    duration_ms: 22,
                    turn_count: 1,
                    tool_call_count: 2,
                    tokens_used: 3,
                    context_window_tokens: 100,
                    context_usage_pct: 3,
                    tools_used: vec!["read".into()],
                    error_count: 0,
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        old_lifecycle.len(),
        2,
        "captured replay cannot grow after append"
    );
    assert!(old_ownership.belongs_to(parent.id.0.as_ref()));
    let (_, new_lifecycle) = load_descendant_replay_at(parent.id.0.as_ref(), root.path())
        .unwrap()
        .unwrap();
    assert_eq!(new_lifecycle.len(), 3);
    assert!(matches!(
        &new_lifecycle[2].update,
        GrowUpdate::SubagentProgress { .. }
    ));
}

#[tokio::test]
async fn validated_reader_rejects_missing_mandatory_timeline_and_corruption() {
    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let missing_ledger = info("missing-ledger-root", "/workspace/missing-ledger");
    init(&adapter, &missing_ledger).await;
    let timeline_path = root
        .path()
        .join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname(
            &missing_ledger.cwd,
        ))
        .join(missing_ledger.id.to_string())
        .join("timeline.jsonl");
    std::fs::remove_file(&timeline_path).unwrap();
    let error =
        load_descendant_ownership_at(missing_ledger.id.0.as_ref(), root.path()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

    let corrupt = info("corrupt-root", "/workspace/corrupt");
    init(&adapter, &corrupt).await;
    let mut timeline = chat_state::Timeline::default();
    let event = timeline
        .record(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: "test".into(),
                name: "valid-before-corruption".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .unwrap();
    adapter
        .append_timeline_event_durable(&corrupt, &event)
        .await
        .unwrap();
    let path = root
        .path()
        .join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname(&corrupt.cwd))
        .join(corrupt.id.to_string())
        .join("timeline.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut record: serde_json::Value = serde_json::from_str(text.trim_end()).unwrap();
    let checksum = record["checksum"].as_str().unwrap();
    let mut damaged = checksum.to_owned();
    let last = damaged.pop().unwrap();
    damaged.push(if last == '0' { '1' } else { '0' });
    record["checksum"] = serde_json::Value::String(damaged);
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();
    assert!(load_descendant_replay_at(corrupt.id.0.as_ref(), root.path()).is_err());
}

#[tokio::test]
async fn foreign_receipt_owner_and_foreign_lookup_identity_fail_closed() {
    use crate::session::notification_inbox;

    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let owner = info("receipt-owner", "/workspace/owner");
    init(&adapter, &owner).await;
    let opened = adapter.open_session(&owner).unwrap();
    let body = "valid receipt body";
    let payload_ref = notification_inbox::write_payload(opened.directory(), body).unwrap();
    let source = chat_state::NotificationSource::ParentMessage {
        parent_session_id: "source-parent".into(),
        message_id: "message-1".into(),
        interrupt: false,
    };
    let source_version = chat_state::NotificationSourceVersion::Ordinal {
        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let id = chat_state::notification_id("foreign-session", &source, &source_version).unwrap();
    let mut timeline = chat_state::Timeline::default();
    let receipt = timeline
        .record(chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received {
                id,
                owner_session_id: "foreign-session".into(),
                source,
                source_version,
                payload_ref,
            },
        ))
        .unwrap();
    adapter
        .append_timeline_event_durable(&owner, &receipt)
        .await
        .unwrap();
    assert!(load_descendant_ownership_at(owner.id.0.as_ref(), root.path()).is_err());

    let other_root = TempDir::new().unwrap();
    assert!(
        load_descendant_replay_at(owner.id.0.as_ref(), other_root.path())
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn oversized_optional_metadata_is_omitted_without_losing_local_authority() {
    let root = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let parent = info("large-meta-root", "/workspace/root");
    let child = info("large-meta-child", "/workspace/child");
    add_canonical_spawn(&adapter, &parent, &child).await;
    let mut ownership = load_descendant_ownership_at(parent.id.0.as_ref(), root.path())
        .unwrap()
        .unwrap();
    assert!(ownership.clone().into_load_meta().is_some());
    ownership
        .spawns
        .get_mut(child.id.0.as_ref())
        .unwrap()
        .spawn
        .prompt = "\u{0000}".repeat(170_000);
    let near_limit = ownership.clone().into_load_meta().unwrap();
    assert!(serde_json::to_vec(&near_limit).unwrap().len() <= 1024 * 1024);
    ownership
        .spawns
        .get_mut(child.id.0.as_ref())
        .unwrap()
        .spawn
        .prompt = "\u{0000}".repeat(180_000);
    assert!(ownership.clone().into_load_meta().is_none());
    assert_eq!(
        ownership.into_spawns().len(),
        1,
        "omission is not empty authority"
    );
    assert_eq!(
        load_descendant_ownership_at(parent.id.0.as_ref(), root.path())
            .unwrap()
            .unwrap()
            .into_spawns()
            .len(),
        1
    );
}
