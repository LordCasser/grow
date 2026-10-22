//! Non-ignored guard for the testkit's core path: synthesize a small session
//! with [`synth::prepare_session`], then confirm the production history reader
//! projects the durable user and assistant facts with the right per-kind counts.
//! AvailableCommandsUpdate is intentionally transient: the catalog is
//! re-advertised after load and is not a missing durable message.

use acp_transport::protocol as acp;
use tempfile::TempDir;

use shell::session::storage::{JsonlStorageAdapter, load_updates_for_replay_at};
use shell::session::testkit::synth::{self, SessionSpec};

#[tokio::test]
async fn synth_replay_roundtrip_preserves_durable_user_and_agent_counts() {
    let root = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    // Distinct per-turn counts so a miscount of any kind is unambiguous.
    let spec = SessionSpec {
        turns: 3,
        acu_per_turn: 2,
        catalog_commands: 2,
        catalog_desc_len: 8,
        agent_chunks_per_turn: 4,
        agent_chunk_len: 16,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    };
    assert!(spec.acu_per_turn > 0 && spec.catalog_commands > 0);

    let (info, dir) = synth::prepare_session(root.path(), cwd.path(), &spec).await;

    let replayed = load_updates_for_replay_at(info.id.0.as_ref(), root.path())
        .expect("load_updates_for_replay_at")
        .unwrap_or_default();

    let count = |pred: fn(&acp::SessionUpdate) -> bool| replayed.iter().filter(|u| pred(u)).count();
    let users = count(|u| matches!(u, acp::SessionUpdate::UserMessageChunk(_)));
    let acus = count(|u| matches!(u, acp::SessionUpdate::AvailableCommandsUpdate(_)));
    let agents = count(|u| matches!(u, acp::SessionUpdate::AgentMessageChunk(_)));

    assert_eq!(users, spec.turns, "one user chunk per turn");
    assert_eq!(
        acus, 0,
        "AvailableCommandsUpdate catalog rows are transient and re-advertised after load"
    );
    assert_eq!(
        agents,
        spec.turns * spec.agent_chunks_per_turn,
        "every agent chunk is preserved"
    );
    assert_eq!(
        replayed.len(),
        spec.turns * (1 + spec.agent_chunks_per_turn),
        "every durable user and assistant fact is preserved exactly once"
    );
    assert!(
        !dir.join("updates.jsonl").exists(),
        "history is stored in the unified journal, not an updates cache"
    );
}

/// The adapter-driven bench generator reaches its byte target in the unified
/// history journal. Synchronous because `synthesize_to_target_bytes` drives
/// the adapter on its own runtime.
#[test]
fn synthesize_to_target_bytes_reaches_target_and_parses() {
    let root = TempDir::new().unwrap();
    let target: u64 = 8 * 1024;

    let info = synth::synthesize_to_target_bytes(root.path(), target);

    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let len = adapter
        .history_snapshot_len(&info)
        .expect("read unified history journal metadata");
    assert!(
        len >= target,
        "unified history journal ({len} B) reached the target ({target} B)"
    );

    let replayed = load_updates_for_replay_at(info.id.0.as_ref(), root.path())
        .expect("load_updates_for_replay_at")
        .unwrap_or_default();
    assert!(
        !replayed.is_empty(),
        "the emitted history facts project back"
    );
}
