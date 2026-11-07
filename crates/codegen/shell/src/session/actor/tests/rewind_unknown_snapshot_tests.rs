//! Regression tests: rewind must judge every path by the LATEST checkpoint
//! that touches it (before-snapshot membership) and only that checkpoint's
//! own after entry. A touched path without an after entry is
//! `snapshot_unavailable` — never clean, never substituted with an older
//! checkpoint's entry, regardless of the current file's existence. A
//! confirmed deletion (after content None) with the file currently absent
//! stays clean. Forcing the rewind keeps the existing conflict semantics.
//! Folding an unknown touching round into its predecessor must drop the
//! predecessor's stale same-path after entry, and a later preview must
//! report unknown instead of the stale after.

use super::support::{create_test_actor, seed_test_timeline};
use crate::sampling::ConversationItem;
use crate::session::{RewindConflictInfo, RewindMode, RewindRequest, SessionActor};
use paths::RelPathBuf;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::LocalSet;
use workspace::file_system::{AsyncFsWrapper, MockFs};
use workspace::session::file_state::FileStateTracker;

/// Two real prompts so a ConversationOnly rewind has a valid branch target.
fn seed_two_prompts() -> Vec<ConversationItem> {
    let turn_user = |text: &str, idx: usize| {
        let mut item = ConversationItem::user(text);
        item.set_prompt_index(idx);
        item
    };
    vec![
        ConversationItem::system("test system prompt"),
        ConversationItem::user("<user_info>OS: test</user_info>"),
        turn_user("P0", 0),
        ConversationItem::assistant("A0"),
        turn_user("P1", 1),
        ConversationItem::assistant("A1"),
    ]
}

/// Rewind-point shape produced by the bounded capture contract:
///
/// - prompt 0 completes through the real `end_prompt` capture: `a.rs` ends
///   KNOWN (`v0`), `d.rs` ends confirmed-absent (`content = None`), `c.rs`
///   is confirmed absent at that time;
/// - prompt 1 re-touches `a.rs` and `c.rs` but never confirms after states
///   (no after entries): the LATEST touching checkpoint for both is unknown,
///   while `d.rs`'s latest touching checkpoint stays prompt 0.
async fn seed_unknown_state_tracker(root: &Path) -> Arc<FileStateTracker> {
    let fixture_fs = AsyncFsWrapper::new(Arc::new(MockFs::new(root.to_path_buf())));
    fixture_fs
        .write_file(Path::new("a.rs"), b"v0")
        .await
        .unwrap();

    let tracker = FileStateTracker::new();
    tracker.begin_prompt(0).await;
    tracker
        .add_before_snapshot_for_prompt(0, &root.join("a.rs"), root, Some("v0".into()))
        .await;
    tracker
        .add_before_snapshot_for_prompt(0, &root.join("d.rs"), root, Some("v0d".into()))
        .await;
    tracker
        .add_before_snapshot_for_prompt(0, &root.join("c.rs"), root, Some("v0c".into()))
        .await;
    tracker.end_prompt(&fixture_fs, 0).await;

    // Prompt 1 re-touches a.rs and c.rs; no end_prompt call — its touched
    // paths carry no after entries, i.e. unknown rounds.
    tracker.begin_prompt(1).await;
    tracker
        .add_before_snapshot_for_prompt(1, &root.join("a.rs"), root, Some("v1".into()))
        .await;
    tracker
        .add_before_snapshot_for_prompt(1, &root.join("c.rs"), root, Some("v1c".into()))
        .await;

    Arc::new(tracker)
}

/// Actor whose current file state matches prompt 0's old after for `a.rs`:
/// the pre-fix preview would find that stale entry via `find_map` and call
/// the path clean. The fixture tracker replaces the actor's own.
async fn build_unknown_state_actor() -> (SessionActor, PathBuf) {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 200_000, 80, gateway_tx, persistence_tx).await;
    let root = actor.tool_context.cwd.to_path_buf();
    actor.file_state_tracker = seed_unknown_state_tracker(&root).await;
    actor
        .tool_context
        .fs
        .write_file(Path::new("a.rs"), b"v0")
        .await
        .unwrap();
    (actor, root)
}

/// Two-round same-path regression: the latest touching checkpoint (prompt 1)
/// is unknown for `a.rs` and `c.rs`, so neither may be clean — not the file
/// whose content still matches the older checkpoint's after entry, and not
/// the missing file whose missing entry would equal the current `None`. The
/// confirmed-deleted `d.rs` stays clean, and the preview names unknown
/// instead of external modification.
async fn run_preview_reports_snapshot_unavailable() {
    let (mut actor, _root) = build_unknown_state_actor().await;

    let response = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 0,
            force: false,
            mode: RewindMode::FilesOnly,
        })
        .await
        .expect("handle_rewind preview ok");

    assert!(
        !response.success,
        "preview with unknown conflicts must refuse: {response:?}"
    );
    let unavailable: Vec<&RewindConflictInfo> = response
        .conflicts
        .iter()
        .filter(|conflict| conflict.conflict_type == "snapshot_unavailable")
        .collect();
    let unavailable_paths: Vec<&str> = unavailable
        .iter()
        .map(|conflict| conflict.path.as_str())
        .collect();
    assert_eq!(
        unavailable_paths,
        vec!["a.rs", "c.rs"],
        "both unknown-touching paths must report snapshot_unavailable \
         regardless of current existence: {response:?}"
    );
    assert_eq!(
        response.clean_files,
        vec!["d.rs".to_string()],
        "confirmed deletion stays clean; unknown paths must never be clean: {response:?}"
    );
    let error = response.error.as_deref().unwrap_or_default();
    assert!(
        error.contains("cannot be confirmed"),
        "preview must name the unknown final states: {error}"
    );
    assert!(
        !error.contains("External modifications detected"),
        "unknown must not be presented as external modification: {error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn preview_reports_snapshot_unavailable_instead_of_stale_after() {
    LocalSet::new()
        .run_until(run_preview_reports_snapshot_unavailable())
        .await;
}

/// Force keeps the existing user-confirmed semantics: the rewind executes
/// with the conflicts still reported, restoring every before-snapshot —
/// including the unknown-after paths — with no new approval step.
async fn run_force_executes_with_unknown_conflicts() {
    let (mut actor, _root) = build_unknown_state_actor().await;

    let response = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 0,
            force: true,
            mode: RewindMode::FilesOnly,
        })
        .await
        .expect("handle_rewind commit ok");

    assert!(
        response.success,
        "force must execute despite conflicts: {response:?}"
    );
    assert_eq!(response.reverted_files.len(), 3, "{response:?}");
    assert!(
        response
            .conflicts
            .iter()
            .any(|conflict| conflict.path == "a.rs"
                && conflict.conflict_type == "snapshot_unavailable"),
        "commit response keeps reporting the unknown conflicts: {response:?}"
    );
    let restored_d = actor
        .tool_context
        .fs
        .try_read_to_string(Path::new("d.rs"))
        .await
        .unwrap();
    assert_eq!(
        restored_d.as_deref(),
        Some("v0d"),
        "confirmed-deleted file restored from its before snapshot"
    );
    let restored_c = actor
        .tool_context
        .fs
        .try_read_to_string(Path::new("c.rs"))
        .await
        .unwrap();
    assert_eq!(
        restored_c.as_deref(),
        Some("v0c"),
        "unknown-after file restored like any other conflict"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn force_true_keeps_existing_conflict_semantics() {
    LocalSet::new()
        .run_until(run_force_executes_with_unknown_conflicts())
        .await;
}

/// ConversationOnly rewind folds the unknown touching round into its
/// predecessor: the predecessor's stale known-after for the same paths must
/// be dropped, and a later preview must report unknown — never the old after.
async fn run_merge_drops_stale_after_of_unknown_round() {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 200_000, 80, gateway_tx, persistence_tx).await;
    seed_test_timeline(&actor, seed_two_prompts(), &["P0", "P1"]).await;
    let root = actor.tool_context.cwd.to_path_buf();
    actor.file_state_tracker = seed_unknown_state_tracker(&root).await;
    actor
        .tool_context
        .fs
        .write_file(Path::new("a.rs"), b"v0")
        .await
        .unwrap();

    let response = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 1,
            force: true,
            mode: RewindMode::ConversationOnly,
        })
        .await
        .expect("conversation-only rewind ok");
    assert!(response.success, "{response:?}");

    let points = actor.file_state_tracker.get_rewind_points().await.unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].prompt_index, 0);
    let unknown_rel = |name: &str| RelPathBuf::new(name).unwrap();
    assert!(
        !points[0].after_snapshots.contains_key(&unknown_rel("a.rs")),
        "merged point must drop the stale after of the unknown touching round"
    );
    assert!(
        !points[0].after_snapshots.contains_key(&unknown_rel("c.rs")),
        "the unknown round also invalidates the predecessor's confirmed-absence entry"
    );
    assert!(
        points[0].after_snapshots.contains_key(&unknown_rel("d.rs")),
        "paths untouched by the unknown round keep their known after"
    );

    // The merged point must now preview as unknown, not clean via old after.
    let preview = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 0,
            force: false,
            mode: RewindMode::FilesOnly,
        })
        .await
        .expect("preview after merge ok");
    assert!(!preview.success);
    assert!(
        preview
            .conflicts
            .iter()
            .any(|conflict| conflict.path == "a.rs"
                && conflict.conflict_type == "snapshot_unavailable"),
        "preview after merge must be unknown, not the stale after: {preview:?}"
    );
    assert!(
        preview.clean_files.iter().any(|path| path == "d.rs"),
        "confirmed deletion stays clean after the merge: {preview:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn conversation_only_merge_drops_stale_after_of_unknown_round() {
    LocalSet::new()
        .run_until(run_merge_drops_stale_after_of_unknown_round())
        .await;
}
