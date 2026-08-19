use super::support::{create_test_actor, record_test_prompt, seed_test_timeline};

use crate::sampling::ConversationItem;
use crate::session::{RewindMode, RewindRequest};
use agent_client_protocol as acp;

fn prompt(text: &str, index: usize) -> ConversationItem {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(index);
    item
}

async fn seed_compacted_timeline(actor: &super::SessionActor) {
    let mut conversation = vec![
        ConversationItem::system("SYS"),
        ConversationItem::user("UI0"),
    ];
    for index in 0..5 {
        conversation.push(prompt(&format!("P{index}"), index));
    }
    seed_test_timeline(actor, conversation, &["P0", "P1", "P2", "P3", "P4"]).await;
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Started {
                id: "compact-5".into(),
                source_items: 7,
                prompt_index: 5,
            },
        ))
        .await
        .unwrap();
    let (_, source_surface_revision) = actor
        .chat_state_handle
        .get_conversation_with_revision()
        .await
        .expect("chat-state actor must be live");
    let materialized = actor
        .chat_state_handle
        .materialize_timeline(actor.session_id_string())
        .await
        .expect("compaction input must materialize");
    let input_ref = materialized.input_ref;
    let target = chat_state::SurfaceRange {
        start: *materialized.surface_ids.first().unwrap(),
        end: *materialized.surface_ids.last().unwrap(),
        shadowed: materialized.surface_ids,
    };
    let sideband_id = uuid::Uuid::now_v7().to_string();
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Sideband(
            chat_state::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: chat_state::SidebandPurpose::CompactionSummary,
                input_refs: vec![input_ref.clone()],
            },
        ))
        .await
        .unwrap();
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Summary {
                id: "compact-5".into(),
                input_ref,
                result_ref: chat_state::TimelineRangeRef {
                    timeline_id: sideband_id,
                    first_seq: 2,
                    last_seq: 2,
                },
                target: target.clone(),
                source_tokens: 100,
                summary_chars: 7,
            },
        ))
        .await
        .unwrap();
    actor
        .chat_state_handle
        .replace_compaction_range(
            target,
            vec![
                ConversationItem::system("SYS"),
                ConversationItem::user("UI1"),
                ConversationItem::user("SUMMARY"),
            ],
            source_surface_revision,
        )
        .await
        .unwrap();
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Completed {
                id: "compact-5".into(),
                source_items: 7,
                result_items: 3,
                duration_ms: 1,
            },
        ))
        .await
        .unwrap();
    actor.chat_state_handle.push_user_message(prompt("P5", 5));
    record_test_prompt(actor, "P5").await;
    actor
        .chat_state_handle
        .push_assistant_response(ConversationItem::assistant("R5"));
    actor.chat_state_handle.push_user_message(prompt("P6", 6));
    record_test_prompt(actor, "P6").await;
}

#[tokio::test(flavor = "current_thread")]
async fn rewind_pre_compaction_with_cancelled_turns_truncates_context_gb2961() {
    let local = tokio::task::LocalSet::new();
    local.run_until(run_rewind_scenario()).await;
}

async fn run_rewind_scenario() {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 200_000, 80, gateway_tx, persistence_tx).await;

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    actor.session_info.id = acp::SessionId::new(format!("rw-e2e-{unique}"));

    seed_compacted_timeline(&actor).await;

    let resp = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 3,
            force: true,
            mode: RewindMode::ConversationOnly,
        })
        .await
        .expect("handle_rewind ok");
    assert!(resp.success, "rewind should succeed: {resp:?}");

    let conv = actor.chat_state_handle.get_conversation().await;
    let texts: Vec<String> = conv.iter().map(|c| c.text_content()).collect();

    assert_eq!(
        texts,
        vec!["SYS", "UI0", "P0", "P1", "P2"],
        "conversation must truncate to prompts 0..2 (got {texts:?})"
    );
    assert!(
        !texts
            .iter()
            .any(|t| ["P3", "P4", "P5", "P6", "SUMMARY"].contains(&t.as_str())),
        "post-target prompts / compacted summary must not leak into context: {texts:?}"
    );
    assert_eq!(
        actor.chat_state_handle.get_prompt_index().await,
        3,
        "prompt_index must be reset to the rewind target"
    );
}

/// `FilesOnly` is exempt from the chat-state prompt-index bound (its real bound
/// is the on-disk snapshot index), so it no-ops to success when out of range —
/// the property the bridge relies on when the chat-state index is empty.
/// `ConversationOnly` is NOT exempt and still rejects an out-of-range target.
#[tokio::test(flavor = "current_thread")]
async fn files_only_rewind_is_exempt_from_chat_state_bound() {
    let local = tokio::task::LocalSet::new();
    local.run_until(run_files_only_bound_scenario()).await;
}

async fn run_files_only_bound_scenario() {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let actor = create_test_actor(0, 200_000, 80, gateway_tx, persistence_tx).await;

    record_test_prompt(&actor, "P0").await;
    record_test_prompt(&actor, "P1").await;

    // Out-of-range FilesOnly: exempt → reverts nothing (no snapshots) but
    // succeeds.
    let oor = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 5,
            force: true,
            mode: RewindMode::FilesOnly,
        })
        .await
        .expect("files-only rewind ok");
    assert!(
        oor.success,
        "out-of-range FilesOnly must no-op succeed: {oor:?}"
    );
    assert!(oor.reverted_files.is_empty());

    // In-range FilesOnly also succeeds.
    let in_range = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 1,
            force: true,
            mode: RewindMode::FilesOnly,
        })
        .await
        .expect("files-only rewind ok");
    assert!(
        in_range.success,
        "in-range FilesOnly must succeed: {in_range:?}"
    );

    // ConversationOnly is still bounded by the chat-state index.
    let convo = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 5,
            force: true,
            mode: RewindMode::ConversationOnly,
        })
        .await
        .expect("handle_rewind returns Ok(success=false)");
    assert!(
        !convo.success,
        "out-of-range ConversationOnly must be rejected"
    );
    assert!(convo.error.is_some());
}

/// `rewind_file_counts` (the `GetRewindFileCounts` actor arm) maps the
/// file-state tracker's per-prompt snapshot metadata to `prompt_index → count`.
#[tokio::test(flavor = "current_thread")]
async fn rewind_file_counts_maps_snapshot_metadata() {
    let local = tokio::task::LocalSet::new();
    local.run_until(run_file_counts_scenario()).await;
}

async fn run_file_counts_scenario() {
    use std::path::Path;

    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let actor = create_test_actor(0, 200_000, 80, gateway_tx, persistence_tx).await;

    let cwd = Path::new("/tmp");
    // Prompt 0 has two distinct file snapshots; prompt 1 has one.
    actor
        .file_state_tracker
        .add_before_snapshot_for_prompt(0, Path::new("/tmp/a.rs"), cwd, Some("a".into()))
        .await;
    actor
        .file_state_tracker
        .add_before_snapshot_for_prompt(0, Path::new("/tmp/b.rs"), cwd, Some("b".into()))
        .await;
    actor
        .file_state_tracker
        .add_before_snapshot_for_prompt(1, Path::new("/tmp/c.rs"), cwd, Some("c".into()))
        .await;

    let counts = actor.rewind_file_counts().await;
    assert_eq!(counts.get(&0).copied(), Some(2));
    assert_eq!(counts.get(&1).copied(), Some(1));
    assert_eq!(counts.get(&2).copied(), None);
}

/// A cross-compaction rewind to BEFORE the compaction point rebuilds the
/// conversation without a summary, so the stale `last_compaction_prompt_index`
/// must be cleared — otherwise the per-model `x-compactions-remaining` header
/// would wrongly report `0` for a session that no longer holds a summary.
#[tokio::test(flavor = "current_thread")]
async fn rewind_before_compaction_clears_stale_compaction_marker() {
    let local = tokio::task::LocalSet::new();
    local.run_until(run_clears_marker_scenario()).await;
}

async fn run_clears_marker_scenario() {
    use sampling_types::CompactionsRemaining;
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 200_000, 80, gateway_tx, persistence_tx).await;

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    actor.session_info.id = acp::SessionId::new(format!("rw-marker-{unique}"));

    seed_compacted_timeline(&actor).await;

    // Rewind to prompt 3 — before the compaction point (5), so the summary is
    // dropped from the rebuilt conversation and the marker must be cleared.
    let resp = actor
        .handle_rewind(RewindRequest {
            target_prompt_index: 3,
            force: true,
            mode: RewindMode::ConversationOnly,
        })
        .await
        .expect("handle_rewind ok");
    assert!(resp.success, "rewind should succeed: {resp:?}");

    let marker = actor
        .chat_state_handle
        .get_last_compaction_prompt_index()
        .await;

    // End-to-end: advertise support so the gate runs, then read the header
    // off the reconstructed config — it must report a fresh "1", not stale "0".
    actor
        .compactions_remaining
        .set(Some(CompactionsRemaining::Dynamic(true)));
    let header = actor
        .reconstruct_full_config()
        .await
        .extra_headers
        .get("x-compactions-remaining")
        .cloned();

    assert_eq!(
        marker, None,
        "pre-compaction rewind must clear the stale compaction marker"
    );
    assert_eq!(
        header.as_deref(),
        Some("1"),
        "header must report 1 after the summary is dropped (got {header:?})"
    );
}
