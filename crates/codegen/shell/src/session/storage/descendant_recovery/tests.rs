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
        execution_recipe: serde_json::json!({"private": "must not be copied"}),
        context_source: chat_state::SubagentContextSource::New,
        source_ref: None,
        context_normalized: false,
        resumed_from: None,
        parent_prompt_id: None,
        capability_mode: None,
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
            chat_state::SubagentEvent::Spawned(spawn.clone()),
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

fn grow(parent: &Info, event_id: &str, update: GrowUpdate) -> SessionUpdate {
    let mut meta = serde_json::Map::new();
    meta.insert("eventId".into(), serde_json::Value::String(event_id.into()));
    SessionUpdate::Grow(Box::new(GrowNotification {
        session_id: parent.id.clone(),
        update,
        meta: Some(serde_json::Value::Object(meta)),
    }))
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
    assert_eq!(compact.subagent_id, "agent-child");
    assert_eq!(compact.prompt, "Read the requested files");
    assert_eq!(compact.child_cwd, child.cwd);
    assert_eq!(compact.worktree_path.as_deref(), Some("/worktrees/child"));
    let encoded = serde_json::to_value(compact).unwrap();
    assert!(encoded.get("execution_recipe").is_none());
    assert!(encoded.get("origin_tool_call_id").is_none());

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
    assert!(!ownership.belongs_to("foreign-root"));

    let mut wire = serde_json::to_value(&ownership).unwrap();
    wire["version"] = serde_json::json!(2);
    let wrong_version: DescendantOwnership = serde_json::from_value(wire).unwrap();
    assert!(!wrong_version.belongs_to(parent.id.0.as_ref()));

    let mut wire = serde_json::to_value(&ownership).unwrap();
    wire["hash"] = serde_json::json!("not-a-prefix-hash");
    let wrong_hash: DescendantOwnership = serde_json::from_value(wire).unwrap();
    assert!(!wrong_hash.belongs_to(parent.id.0.as_ref()));

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
        .prompt = "\u{0000}".repeat(170_000);
    let near_limit = ownership.clone().into_load_meta().unwrap();
    assert!(serde_json::to_vec(&near_limit).unwrap().len() <= 1024 * 1024);
    ownership
        .spawns
        .get_mut(child.id.0.as_ref())
        .unwrap()
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
