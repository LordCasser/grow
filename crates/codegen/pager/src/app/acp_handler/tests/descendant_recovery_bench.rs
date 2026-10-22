//! Manual release benchmark for rebuilding nested subagent projections.
//!
//! Run the ignored test in the Pager test binary with a release profile. The
//! fixture creates valid unified journals and owner seeds, but does not start
//! child backends or execute cold recovery.

use super::*;
use shell::session::storage::{
    JsonlStorageAdapter, SessionUpdate as StorageSessionUpdate, StorageAdapter,
};
use shell::session::testkit::synth::{SessionSpec, replay};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct NodeFixture {
    info: shell::session::info::Info,
    grow_home: PathBuf,
    directory: PathBuf,
    _cwd: tempfile::TempDir,
}

fn turns_from_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| {
            let turns = value
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("{name} must be a positive integer"));
            assert!(turns > 0, "{name} must be positive");
            turns
        })
        .unwrap_or(default)
}

fn history_spec(turns: usize) -> SessionSpec {
    SessionSpec {
        turns,
        acu_per_turn: 0,
        catalog_commands: 0,
        catalog_desc_len: 0,
        agent_chunks_per_turn: 8,
        agent_chunk_len: 4096,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    }
}

async fn make_node(grow_home: &Path, turns: usize) -> NodeFixture {
    let cwd = tempfile::tempdir().expect("create session cwd");
    let cwd_path = std::fs::canonicalize(cwd.path()).expect("canonicalize session cwd");
    let spec = history_spec(turns);
    let (info, directory) = replay::prepare_legacy_session(grow_home, &cwd_path, &spec).await;
    shell::session::testkit::synth::write_admitted_history(
        &info,
        &directory,
        &spec,
        "test-model",
        0,
        None,
    );
    NodeFixture {
        info,
        grow_home: grow_home.to_owned(),
        directory,
        _cwd: cwd,
    }
}

fn id(node: &NodeFixture) -> String {
    node.info.id.0.to_string()
}

fn spawn_fact(
    parent: &NodeFixture,
    child: &NodeFixture,
    worktree: bool,
) -> chat_state::SubagentSpawnEvent {
    chat_state::SubagentSpawnEvent {
        subagent_id: id(child),
        child_session_id: id(child),
        origin_tool_call_id: None,
        security_parent_session_id: id(parent),
        parent_step_id: None,
        subagent_type: "general-purpose".into(),
        description: "descendant recovery benchmark".into(),
        prompt: "inspect the synthetic history".into(),
        execution_recipe: serde_json::json!({}),
        context_source: chat_state::SubagentContextSource::New,
        source_ref: None,
        context_normalized: false,
        resumed_from: None,
        parent_prompt_id: None,
        capability_mode: Some("read-only".into()),
        permission_mode: Some("auto".into()),
        effective_permission_mode: Some("auto".into()),
        workflow_run_id: None,
        goal_id: None,
        goal_definition_revision: None,
        surface_completion: true,
        child_cwd: child.info.cwd.clone(),
        worktree_path: worktree.then(|| child.info.cwd.clone()),
        effective_model_id: "test-model".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "test-model",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    }
}

fn set_child_summary(node: &NodeFixture, parent_id: &str) {
    let summary_path = node.directory.join("summary.json");
    let mut summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary_path).expect("read child summary"))
            .expect("parse child summary");
    summary["parent_session_id"] = serde_json::Value::String(parent_id.to_owned());
    summary["session_kind"] = serde_json::Value::String("subagent".into());
    std::fs::write(
        summary_path,
        serde_json::to_vec(&summary).expect("serialize child summary"),
    )
    .expect("write child ownership summary");
}

fn projected_spawn_update(
    parent_id: &str,
    spawn: &chat_state::SubagentSpawnEvent,
) -> shell::extensions::notification::SessionUpdate {
    let effective_context_source = match spawn.context_source {
        chat_state::SubagentContextSource::New => "new",
        chat_state::SubagentContextSource::Forked => "forked",
        chat_state::SubagentContextSource::Resumed => "resumed",
    };
    shell::extensions::notification::SessionUpdate::SubagentSpawned {
        subagent_id: spawn.subagent_id.clone(),
        parent_session_id: parent_id.to_owned(),
        parent_prompt_id: spawn.parent_prompt_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        subagent_type: spawn.subagent_type.clone(),
        description: spawn.description.clone(),
        effective_context_source: Some(effective_context_source.to_owned()),
        context_normalized: spawn.context_normalized,
        capability_mode: spawn.capability_mode.clone(),
        permission_mode: spawn.permission_mode.clone(),
        effective_permission_mode: spawn.effective_permission_mode.clone(),
        model: Some(spawn.effective_model_id.clone()),
        reasoning_effort: spawn.reasoning_effort.map(|effort| effort.to_string()),
        model_state: None,
        workflow_agent_names: None,
        resumed_from: spawn.resumed_from.clone(),
        workflow_run_id: spawn.workflow_run_id.clone(),
        goal_id: spawn.goal_id.clone(),
    }
}

async fn append_spawn(
    parent: &NodeFixture,
    child: &NodeFixture,
    worktree: bool,
) -> (chat_state::SubagentSpawnEvent, u64) {
    let path = parent.directory.join("timeline.jsonl");
    let mut timeline = shell::session::testkit::read_timeline_fixture(&path)
        .expect("decode parent unified journal");
    let spawn = spawn_fact(parent, child, worktree);
    let event = timeline
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(spawn.clone()),
        ))
        .expect("append valid parent Spawned event");
    let spawn_seq = event.seq.get();
    shell::session::testkit::append_timeline_domain_fixture(&path, &[event])
        .expect("append Spawned event to unified journal");
    let notification = shell::extensions::notification::SessionNotification {
        session_id: acp::SessionId::new(id(parent)),
        update: projected_spawn_update(&id(parent), &spawn),
        // Modern producers stamp stable source-local identities. Preserve it
        // so repeated descendant replay exercises the real overlap gate.
        meta: Some(serde_json::json!({
            "eventId": format!("{}-{spawn_seq}", id(parent)),
        })),
    };
    JsonlStorageAdapter::with_root(parent.grow_home.clone())
        .append_update(
            &parent.info,
            &StorageSessionUpdate::Grow(Box::new(notification)),
        )
        .await
        .expect("append matching typed Spawned presentation fact");
    (spawn, spawn_seq)
}

fn append_seed(
    child: &NodeFixture,
    parent_id: &str,
    spawn_seq: u64,
    spawn: &chat_state::SubagentSpawnEvent,
) {
    let path = child.directory.join("timeline.jsonl");
    let mut timeline = shell::session::testkit::read_timeline_fixture(&path)
        .expect("decode child unified journal");
    timeline
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: parent_id.to_owned(),
                parent_spawn_seq: spawn_seq,
                subagent_id: spawn.subagent_id.clone(),
                security_parent_session_id: spawn.security_parent_session_id.clone(),
                context_source: spawn.context_source,
                source_ref: spawn.source_ref.clone(),
                normalized: spawn.context_normalized,
            },
        ))
        .expect("append matching child owner seed");
    let event = timeline.events().last().expect("seed event exists").clone();
    shell::session::testkit::append_timeline_domain_fixture(&path, &[event])
        .expect("append owner seed to unified journal");
    let child_timeline = shell::session::testkit::read_timeline_fixture(&path)
        .expect("read child after owner seed append");
    child_timeline
        .validate_subagent_seed_link(parent_id, chat_state::EventSeq::new(spawn_seq), spawn)
        .expect("child seed must match its exact parent spawn");
    set_child_summary(child, parent_id);
}

async fn install_topology(
    nodes: &[NodeFixture],
    edges: &[(usize, usize)],
) -> Vec<(String, String)> {
    let mut seeded = Vec::with_capacity(edges.len());
    for (edge_index, &(parent_index, child_index)) in edges.iter().enumerate() {
        let parent = &nodes[parent_index];
        let child = &nodes[child_index];
        let (spawn, spawn_seq) = append_spawn(parent, child, edge_index % 2 == 0).await;
        append_seed(child, &id(parent), spawn_seq, &spawn);
        seeded.push((id(parent), id(child)));
    }
    seeded
}

fn child_rows(view: &AgentView) -> Vec<String> {
    view.scrollback
        .iter_entries()
        .filter_map(|(_, entry)| match &entry.block {
            crate::scrollback::block::RenderBlock::Subagent(block) => {
                Some(block.child_session_id.to_string())
            }
            _ => None,
        })
        .collect()
}

fn projection_shape(app: &AppView) -> BTreeMap<String, (usize, Vec<String>)> {
    let root = &app.agents[&AgentId(0)];
    let mut result = BTreeMap::new();
    result.insert("<root>".into(), (root.scrollback.len(), child_rows(root)));
    for (session_id, view) in &root.subagent_views {
        result.insert(
            session_id.clone(),
            (view.scrollback.len(), child_rows(view)),
        );
    }
    result
}

async fn run_shape(shape: &str, edges: &[(usize, usize)], child_turns: usize) {
    let grow_home = tempfile::tempdir().expect("create benchmark Grow home");
    crate::app::subagent::set_replay_grow_home_for_tests(Some(grow_home.path().to_owned()));
    let root_turns = turns_from_env("GROW_DESCENDANT_RECOVERY_ROOT_TURNS", 128);
    let child_turns = turns_from_env("GROW_DESCENDANT_RECOVERY_CHILD_TURNS", child_turns);
    let mut nodes = Vec::with_capacity(4);
    nodes.push(make_node(grow_home.path(), root_turns).await);
    for _ in 0..3 {
        nodes.push(make_node(grow_home.path(), child_turns).await);
    }
    let expected_edges = install_topology(&nodes, edges).await;
    let journal_bytes: u64 = nodes
        .iter()
        .map(|node| {
            std::fs::metadata(node.directory.join("timeline.jsonl"))
                .expect("stat node journal")
                .len()
        })
        .sum();

    // Build the already-projected root children outside the timed recovery.
    let root_id = id(&nodes[0]);
    let mut app = make_app_with_agent(&root_id);
    // Match a root replay: direct child rows are projected without eagerly
    // loading child transcripts or doing per-spawn disk enrichment.
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = true;
    let expected_root_rows = edges
        .iter()
        .map(|&(_, child_index)| id(&nodes[child_index]))
        .collect::<Vec<_>>();
    for &(parent_index, child_index) in edges {
        if parent_index != 0 {
            continue;
        }
        let child_id = id(&nodes[child_index]);
        handle(
            make_replayed_ext_session_notification(
                &root_id,
                &format!("{root_id}-{child_index}"),
                test_subagent_spawned(&root_id, &child_id),
            ),
            &mut app,
        );
    }

    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = false;
    let started = Instant::now();
    crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), None);
    let recovery_ms = started.elapsed().as_secs_f64() * 1000.0;

    let root = &app.agents[&AgentId(0)];
    assert_eq!(root.session.subagent_sessions.len(), 3);
    assert_eq!(root.subagent_views.len(), 3);
    assert_eq!(child_rows(root), expected_root_rows, "root child order");
    for owner_index in 1..nodes.len() {
        let owner_id = id(&nodes[owner_index]);
        let owner_view = &root.subagent_views[&owner_id];
        assert_eq!(
            child_rows(owner_view),
            Vec::<String>::new(),
            "child view eagerly contains lifecycle rows for {owner_id}"
        );
    }
    for (parent_id, child_id) in &expected_edges {
        let child = root
            .subagent_views
            .get(child_id)
            .unwrap_or_else(|| panic!("restored child view missing: {child_id}"));
        let info = &root.session.subagent_sessions[child_id];
        assert_eq!(info.subagent_id.as_ref(), child_id);
        assert_eq!(
            info.child_cwd.as_deref(),
            Some(child.session.cwd.to_string_lossy().as_ref()),
        );
        assert_eq!(
            child.session.cwd,
            PathBuf::from(info.child_cwd.as_deref().unwrap())
        );
        let spawn =
            shell::session::testkit::read_timeline_fixture(
                &nodes
                    .iter()
                    .find(|node| id(node) == *parent_id)
                    .expect("parent fixture exists")
                    .directory
                    .join("timeline.jsonl"),
            )
            .expect("read parent fixture for control assertion")
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.child_session_id == *child_id => Some(spawn.clone()),
                _ => None,
            })
            .expect("durable spawn exists");
        assert_eq!(spawn.security_parent_session_id, *parent_id);
        assert_eq!(info.prompt.as_deref(), Some(spawn.prompt.as_str()));
        assert_eq!(
            info.worktree_path.as_deref(),
            spawn.worktree_path.as_deref()
        );
        assert_eq!(child.session.is_worktree, spawn.worktree_path.is_some());
    }

    let first_shape = projection_shape(&app);
    crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), None);
    assert_eq!(
        projection_shape(&app),
        first_shape,
        "a second recovery pass must not add rows or change stable child order"
    );
    crate::app::subagent::set_replay_grow_home_for_tests(None);

    println!(
        "{{\"benchmark\":\"descendant_recovery\",\"shape\":{shape:?},\"node_count\":{},\"depth\":{},\"root_turns\":{root_turns},\"child_turns\":{child_turns},\"journal_bytes\":{journal_bytes},\"recovery_ms\":{recovery_ms:.3}}}",
        nodes.len(),
        if shape == "star" { 1 } else { 3 },
    );
}

#[tokio::test]
#[ignore = "manual release benchmark for nested descendant projection recovery"]
async fn descendant_recovery_star_and_chain_release_benchmark() {
    run_shape("star", &[(0, 1), (0, 2), (0, 3)], 256).await;
    run_shape("chain", &[(0, 1), (1, 2), (2, 3)], 256).await;
}
