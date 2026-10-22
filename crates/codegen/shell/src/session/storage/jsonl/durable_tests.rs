use super::*;
use crate::session::info::Info;
use crate::session::persistence::default_model_id;
use crate::session::storage::{SessionUpdate, StorageAdapter, TIMELINE_FILE};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn info() -> Info {
    Info {
        id: acp::SessionId::new("durable-jsonl"),
        cwd: "/test".into(),
    }
}

fn update(info: &Info, text: String) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(text),
        ))),
    )))
}

fn timeline_event(name: &str, timeline: &mut chat_state::Timeline) -> chat_state::TimelineEvent {
    timeline
        .record(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: "test".into(),
                name: name.into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .unwrap()
}

fn encode_journal(events: &[chat_state::TimelineEvent]) -> Vec<u8> {
    JsonlStorageAdapter::encode_journal(events, &[]).unwrap()
}

fn decode_journal(path: &Path) -> io::Result<super::ledger::JournalData> {
    let text = std::fs::read_to_string(path)?;
    let lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    JsonlStorageAdapter::decode_journal_lines(&lines)
}

fn projection(
    info: &Info,
    text: &str,
) -> crate::session::response_projection::ResponseReplayProjection {
    crate::session::response_projection::project_admitted_response(
        &info.id,
        &chat_state::AdmittedResponse {
            event_seq: chat_state::EventSeq::new(7),
            identity: chat_state::ResponseAdmissionIdentity {
                request_id: "request-1".into(),
                attempt: 1,
            },
            items: vec![sampling_types::ConversationItem::assistant(text)],
            quarantined_tool_exchanges: 0,
        },
    )
    .unwrap()
}

fn append_timeline_with_prefix_state(
    directory: &crate::session::storage::ContainedDirectory,
    path: &Path,
    prefix_state: &std::sync::Mutex<Option<LedgerPrefix>>,
    event: &chat_state::TimelineEvent,
) -> io::Result<()> {
    let mut line = serde_json::to_vec(event).unwrap();
    line.push(b'\n');
    JsonlStorageAdapter::append_timeline_line_in_directory_sync(
        directory,
        prefix_state,
        path,
        line,
        event.seq.get(),
        AppendDurability::Buffered,
    )
}

#[test]
fn prefix_validation_hashes_exact_bounded_bytes_and_validates_each_event() {
    let mut file = tempfile::tempfile().unwrap();
    let mut timeline = chat_state::Timeline::default();
    let mut bytes = Vec::new();
    for index in 0..300 {
        timeline_event(&format!("event-{index}"), &mut timeline);
    }
    for line in encode_journal(timeline.events()).split_inclusive(|byte| *byte == b'\n') {
        // Whitespace is semantically insignificant, but must be included in
        // the immutable prefix digest just like the old whole-file reader.
        bytes.extend_from_slice(line.strip_suffix(b"\n").unwrap_or(line));
        bytes.extend_from_slice(b" \r\n");
    }
    file.write_all(&bytes).unwrap();
    file.write_all(b"uncommitted tail").unwrap();
    let prefix = JsonlStorageAdapter::load_timeline_prefix(&mut file, bytes.len() as u64).unwrap();
    assert_eq!(prefix.hash, blake3::hash(&bytes));
    let mut extended = prefix.hasher;
    extended.update(b"next");
    bytes.extend_from_slice(b"next");
    assert_eq!(extended.finalize(), blake3::hash(&bytes));
}

#[test]
fn prefix_validation_rejects_complete_json_with_invalid_lifecycle() {
    let mut timeline = chat_state::Timeline::default();
    let valid = timeline_event("valid", &mut timeline);
    let mut invalid = timeline_event("invalid", &mut timeline);
    invalid.kind = chat_state::TimelineEventKind::Step(chat_state::StepEvent::Started {
        id: chat_state::StepId {
            turn: chat_state::TurnId(1),
            index: 0,
        },
    });
    let bytes = encode_journal(&[valid, invalid]);
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    assert_eq!(
        JsonlStorageAdapter::load_timeline_prefix(&mut file, bytes.len() as u64)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
    file.seek(io::SeekFrom::Start(0)).unwrap();
    let mut after = Vec::new();
    file.read_to_end(&mut after).unwrap();
    assert_eq!(after, bytes, "a rejected prefix must remain untouched");
}

#[test]
fn prefix_validation_rejects_short_blank_and_oversized_records() {
    let mut file = tempfile::tempfile().unwrap();
    let error = JsonlStorageAdapter::load_timeline_prefix(&mut file, 100)
        .err()
        .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    file.write_all(b"\n").unwrap();
    assert_eq!(
        JsonlStorageAdapter::load_timeline_prefix(&mut file, 1)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
    // A sparse oversized record exercises the read bound without constructing
    // a huge test string or allocating storage for a whole history file.
    let size = super::super::MAX_JSONL_ENTRY_BYTES + 2;
    file.set_len(size).unwrap();
    file.seek(io::SeekFrom::Start(0)).unwrap();
    file.write_all(b" ").unwrap();
    file.seek(io::SeekFrom::End(-1)).unwrap();
    file.write_all(b"\n").unwrap();
    let error = JsonlStorageAdapter::load_timeline_prefix(&mut file, size)
        .err()
        .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("exceeds"));
}

async fn append_after_injected_sync_failure(
    root: &Path,
    info: &Info,
    event: &chat_state::TimelineEvent,
    failure_point: TimelineSyncPoint,
    leave_partial_tail: bool,
) {
    let baseline = JsonlStorageAdapter::with_root(root.to_path_buf());
    if leave_partial_tail {
        let path = baseline.timeline_file(info);
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b"{\"partial\":")
            .unwrap();
    }

    let fail_once = Arc::new(AtomicBool::new(true));
    let should_fail = Arc::clone(&fail_once);
    let writer = JsonlStorageAdapter::with_timeline_sync_probe(root.to_path_buf(), move |point| {
        if point == failure_point && should_fail.swap(false, Ordering::AcqRel) {
            Err(io::Error::other(format!("injected {point:?} sync failure")))
        } else {
            Ok(())
        }
    });
    let error = writer
        .append_timeline_event_durable(info, event)
        .await
        .expect_err("the selected sync boundary must fail after the write");
    assert!(error.to_string().contains("injected"));
    drop(writer);

    // A new adapter models restart after a lost ACK. Replaying the exact event
    // must finish durability without adding another physical canonical fact.
    let restarted = JsonlStorageAdapter::with_root(root.to_path_buf());
    restarted
        .append_timeline_event_durable(info, event)
        .await
        .expect("exact retry after restart");
    let stored = restarted.read_timeline_events_sync(info).unwrap();
    assert_eq!(
        stored
            .iter()
            .filter(|candidate| candidate.seq == event.seq)
            .count(),
        1,
        "the failed attempt and exact retry leave one event at the canonical sequence"
    );
    assert_eq!(
        serde_json::to_value(stored.last().unwrap()).unwrap(),
        serde_json::to_value(event).unwrap(),
        "the canonical payload remains byte-equivalent at the event boundary"
    );
    chat_state::Timeline::from_events(stored.clone()).expect("committed prefix validates");
    let physical = std::fs::read_to_string(restarted.timeline_file(info)).unwrap();
    assert_eq!(physical.lines().count(), stored.len());
    assert!(
        !physical.contains("partial"),
        "retry must truncate an incomplete physical tail before appending"
    );
}

#[tokio::test]
async fn child_finalization_canonical_boundaries_retry_after_file_and_directory_sync_failures() {
    let root = tempfile::tempdir().unwrap();
    let parent_info = Info {
        id: acp::SessionId::new("finalization-matrix-parent"),
        cwd: "/test/parent".into(),
    };
    let child_info = Info {
        id: acp::SessionId::new("finalization-matrix-child"),
        cwd: "/test/child".into(),
    };
    let initial = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    initial
        .init_session(&parent_info, default_model_id())
        .await
        .unwrap();
    initial
        .init_session(&child_info, default_model_id())
        .await
        .unwrap();

    // No dedicated pre-write failure hook exists for canonical Timeline appends.
    // Exercise the existing sequence guard instead: a valid but skipped seq is
    // rejected before bytes are written, then seq 0 can still be committed.
    let mut skipped = chat_state::Timeline::default();
    skipped
        .record(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: "test".into(),
                name: "first".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .unwrap();
    let skipped_event = skipped
        .record(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: "test".into(),
                name: "skipped sequence".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .unwrap();
    let error = initial
        .append_timeline_event_durable(&parent_info, &skipped_event)
        .await
        .expect_err("a new ledger cannot begin at seq 1");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        std::fs::metadata(initial.timeline_file(&parent_info))
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        0,
        "pre-write sequence rejection leaves no canonical bytes"
    );
    let subagent_id = "finalization-matrix-child";
    let mut parent = chat_state::Timeline::default();
    let spawn = parent
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(chat_state::SubagentSpawnEvent {
                subagent_id: subagent_id.into(),
                child_session_id: child_info.id.0.to_string(),
                origin_tool_call_id: None,
                security_parent_session_id: parent_info.id.0.to_string(),
                parent_step_id: None,
                subagent_type: "worker".into(),
                description: "finalization boundary matrix".into(),
                prompt: "complete the task".into(),
                execution_recipe: serde_json::json!({}),
                context_source: chat_state::SubagentContextSource::New,
                source_ref: None,
                context_normalized: false,
                resumed_from: None,
                parent_prompt_id: Some("parent-prompt".into()),
                capability_mode: None,
                permission_mode: None,
                effective_permission_mode: None,
                workflow_run_id: None,
                goal_id: None,
                goal_definition_revision: None,
                surface_completion: true,
                child_cwd: child_info.cwd.clone(),
                worktree_path: None,
                effective_model_id: "test-model".into(),
                model_transport_key: sampling_types::ModelImageInputKey::new(
                    "test-model",
                    "responses",
                    "test",
                ),
                reasoning_effort: None,
            })),
        ))
        .unwrap();

    let mut child = chat_state::Timeline::default();
    let seed = child
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: parent_info.id.0.to_string(),
                parent_spawn_seq: spawn.seq.get(),
                subagent_id: subagent_id.into(),
                security_parent_session_id: parent_info.id.0.to_string(),
                context_source: chat_state::SubagentContextSource::New,
                source_ref: None,
                normalized: false,
            },
        ))
        .unwrap();
    let child_result = child
        .record(chat_state::TimelineEventKind::SubagentResult(
            chat_state::SubagentResultEvent {
                subagent_id: subagent_id.into(),
                outcome: chat_state::SubagentOutcome::Completed,
                duration_ms: 17,
                tool_calls: 1,
                turns: 1,
                tokens_used: 5,
                error: None,
                output_ref: Some(format!(
                    "artifact:subagent-output:blake3:{}",
                    "a".repeat(64)
                )),
                planning_summary: None,
            },
        ))
        .unwrap();
    let usage = parent
        .record(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: "session_usage".into(),
                name: "subagent_settled".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!({
                    "subagent_id": subagent_id,
                    "by_model": [["test-model", {
                        "input_tokens": 3,
                        "output_tokens": 2,
                        "model_calls": 1,
                        "cost_usd_ticks": null,
                        "cached_read_tokens": 0,
                        "cache_creation_tokens": 0,
                        "cache_read_known_input_tokens": 0,
                        "cache_read_unknown_calls": 0,
                        "cache_write_unknown_calls": 0,
                        "reasoning_tokens": 0,
                        "api_duration_ms": 0,
                        "cost_missing_calls": 0
                    }]],
                    "incomplete": false
                })),
            },
        ))
        .unwrap();
    let ended = parent
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                subagent_id: subagent_id.into(),
                child_session_id: child_info.id.0.to_string(),
                outcome: chat_state::SubagentOutcome::Completed,
                duration_ms: 17,
                tool_calls: 1,
                turns: 1,
                tokens_used: 5,
                error: None,
                result_ref: Some(chat_state::TimelineRangeRef {
                    timeline_id: child_info.id.0.to_string(),
                    first_seq: child_result.seq.get(),
                    last_seq: child_result.seq.get(),
                }),
                snapshot_ref: None,
            }),
        ))
        .unwrap();
    let parent_opened = initial.open_session(&parent_info).unwrap();
    let payload_ref = crate::session::notification_inbox::write_payload(
        parent_opened.directory(),
        "Child finalization completed",
    )
    .unwrap();
    let source = chat_state::NotificationSource::SubagentCompleted {
        subagent_id: subagent_id.into(),
        owner: chat_state::NotificationOwner::Session,
    };
    let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
    let receipt_id =
        chat_state::notification_id(parent_info.id.0.as_ref(), &source, &source_version).unwrap();
    let receipt = parent
        .record(chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received {
                id: receipt_id,
                owner_session_id: parent_info.id.0.to_string(),
                source,
                source_version,
                payload_ref,
            },
        ))
        .unwrap();
    drop(parent_opened);
    drop(initial);

    for (info, event, point, partial_tail) in [
        (&child_info, &seed, TimelineSyncPoint::Directory, false),
        (&child_info, &child_result, TimelineSyncPoint::File, true),
        (&parent_info, &spawn, TimelineSyncPoint::File, false),
        (&parent_info, &usage, TimelineSyncPoint::Directory, false),
        (&parent_info, &ended, TimelineSyncPoint::File, false),
        (&parent_info, &receipt, TimelineSyncPoint::Directory, false),
    ] {
        append_after_injected_sync_failure(root.path(), info, event, point, partial_tail).await;
    }

    let reader = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let child_events = reader.read_timeline_events_sync(&child_info).unwrap();
    let parent_events = reader.read_timeline_events_sync(&parent_info).unwrap();
    assert_eq!(
        serde_json::to_value(&child_events).unwrap(),
        serde_json::to_value(child.events()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&parent_events).unwrap(),
        serde_json::to_value(parent.events()).unwrap()
    );
    chat_state::Timeline::from_events(child_events).expect("child seed/result prefix");
    chat_state::Timeline::from_events(parent_events)
        .expect("parent spawn/usage/end/receipt prefix");
}

#[test]
fn timeline_append_rejects_same_length_interior_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let directory = crate::session::storage::ContainedDirectory::open(
        dir.path(),
        Path::new(""),
        "Timeline prefix integrity test",
        false,
    )
    .unwrap();
    let path = dir.path().join("timeline.jsonl");
    let prefix_state = std::sync::Mutex::new(None);
    let mut timeline = chat_state::Timeline::default();
    let first = timeline_event("first", &mut timeline);
    let second = timeline_event("second", &mut timeline);
    let third = timeline_event("third", &mut timeline);
    append_timeline_with_prefix_state(&directory, &path, &prefix_state, &first).unwrap();
    append_timeline_with_prefix_state(&directory, &path, &prefix_state, &second).unwrap();

    let original_len = std::fs::metadata(&path).unwrap().len();
    let mut replacement_timeline = chat_state::Timeline::default();
    let replacement = timeline_event("other", &mut replacement_timeline);
    let original_bytes = encode_journal(&[first.clone(), second.clone()]);
    let replacement_bytes = encode_journal(&[replacement, second]);
    assert_eq!(original_bytes.len(), replacement_bytes.len());
    std::fs::write(&path, replacement_bytes).unwrap();

    let error = append_timeline_with_prefix_state(&directory, &path, &prefix_state, &third)
        .expect_err("interior replacement must invalidate the committed prefix");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), original_len);
}

#[test]
fn timeline_append_rejects_interior_json_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let directory = crate::session::storage::ContainedDirectory::open(
        dir.path(),
        Path::new(""),
        "Timeline prefix corruption test",
        false,
    )
    .unwrap();
    let path = dir.path().join("timeline.jsonl");
    let prefix_state = std::sync::Mutex::new(None);
    let mut timeline = chat_state::Timeline::default();
    let first = timeline_event("first", &mut timeline);
    let second = timeline_event("second", &mut timeline);
    let third = timeline_event("third", &mut timeline);
    append_timeline_with_prefix_state(&directory, &path, &prefix_state, &first).unwrap();
    append_timeline_with_prefix_state(&directory, &path, &prefix_state, &second).unwrap();

    let original_len = std::fs::metadata(&path).unwrap().len();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] = b'[';
    std::fs::write(&path, bytes).unwrap();
    let error = append_timeline_with_prefix_state(&directory, &path, &prefix_state, &third)
        .expect_err("interior JSON corruption must invalidate the committed prefix");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), original_len);
}

#[test]
fn timeline_append_retries_are_idempotent_and_truncate_only_an_incomplete_tail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline.jsonl");
    let mut timeline = chat_state::Timeline::default();
    let first = timeline_event("first", &mut timeline);
    let second = timeline_event("second", &mut timeline);
    let first_line = serde_json::to_vec(&first).unwrap();

    JsonlStorageAdapter::append_timeline_line_sync(
        &path,
        [first_line.clone(), b"\n".to_vec()].concat(),
        first.seq.get(),
        AppendDurability::Buffered,
    )
    .unwrap();
    JsonlStorageAdapter::append_timeline_line_sync(
        &path,
        [first_line, b"\n".to_vec()].concat(),
        first.seq.get(),
        AppendDurability::Buffered,
    )
    .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"version\":2,\"seq\":")
        .unwrap();

    let mut second_line = serde_json::to_vec(&second).unwrap();
    second_line.push(b'\n');
    JsonlStorageAdapter::append_timeline_line_sync(
        &path,
        second_line,
        second.seq.get(),
        AppendDurability::Buffered,
    )
    .unwrap();

    let events = decode_journal(&path).unwrap().events;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].seq.get(), 0);
    assert_eq!(events[1].seq.get(), 1);
}

#[cfg(unix)]
#[test]
fn timeline_append_rejects_symlinked_ledger_and_lock_targets() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.jsonl");
    let path = dir.path().join("timeline.jsonl");
    std::fs::write(&target, b"").unwrap();
    symlink(&target, &path).unwrap();

    let mut timeline = chat_state::Timeline::default();
    let event = timeline_event("must-not-follow", &mut timeline);
    let mut line = serde_json::to_vec(&event).unwrap();
    line.push(b'\n');
    let error = JsonlStorageAdapter::append_timeline_line_sync(
        &path,
        line.clone(),
        event.seq.get(),
        AppendDurability::Buffered,
    )
    .expect_err("Timeline append must not follow a ledger symlink");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(std::fs::read(&target).unwrap().is_empty());

    std::fs::remove_file(&path).unwrap();
    let lock_path = path.with_extension("jsonl.lock");
    std::fs::remove_file(&lock_path).unwrap();
    symlink(&target, &lock_path).unwrap();
    let error = JsonlStorageAdapter::append_timeline_line_sync(
        &path,
        line,
        event.seq.get(),
        AppendDurability::Buffered,
    )
    .expect_err("Timeline append must not follow a lock symlink");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(std::fs::read(&target).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn timeline_append_and_replay_reject_symlinked_ledgers() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("outside.jsonl");
    let path = dir.path().join("timeline.jsonl");
    std::fs::write(&target, b"outside\n").unwrap();
    symlink(&target, &path).unwrap();

    let mut timeline = chat_state::Timeline::default();
    let event = timeline_event("symlink", &mut timeline);
    let mut line = serde_json::to_vec(&event).unwrap();
    line.push(b'\n');
    let error = JsonlStorageAdapter::append_timeline_line_sync(
        &path,
        line,
        event.seq.get(),
        AppendDurability::Buffered,
    )
    .expect_err("Timeline append must not follow a ledger symlink");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&target).unwrap(), b"outside\n");

    let adapter = JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
    let error = adapter
        .read_timeline(path)
        .expect_err("Timeline replay must not follow a ledger symlink");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn timeline_reader_ignores_only_the_uncommitted_final_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline.jsonl");
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
    let mut timeline = chat_state::Timeline::default();
    let event = timeline_event("complete", &mut timeline);
    let mut bytes = encode_journal(&[event]);
    bytes.extend_from_slice(b"{\"version\":");
    bytes.extend_from_slice(&[0xe2, 0x82]);
    std::fs::write(&path, bytes).unwrap();

    assert_eq!(adapter.read_timeline(path.clone()).unwrap().len(), 1);

    std::fs::write(&path, b"not-json\n{\"version\":").unwrap();
    assert!(adapter.read_timeline(path).is_err());
}

#[test]
fn disk_materialization_derives_surface_and_reference_from_one_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline.jsonl");
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
    let timeline = chat_state::Timeline::from_seed(vec![
        sampling_types::ConversationItem::user("first"),
        sampling_types::ConversationItem::assistant("second"),
    ])
    .unwrap();
    for event in timeline.events() {
        let mut line = serde_json::to_vec(event).unwrap();
        line.push(b'\n');
        JsonlStorageAdapter::append_timeline_line_sync(
            &path,
            line,
            event.seq.get(),
            AppendDurability::Buffered,
        )
        .unwrap();
    }

    let materialized = adapter
        .materialize_timeline_from_dir(dir.path(), "source-session")
        .unwrap();
    assert_eq!(materialized.input_ref.timeline_id, "source-session");
    assert_eq!(materialized.input_ref.first_seq, 0);
    assert_eq!(
        materialized.input_ref.last_seq,
        timeline.events().last().unwrap().seq.get()
    );
    assert_eq!(
        serde_json::to_value(&materialized.surface).unwrap(),
        serde_json::to_value(timeline.surface()).unwrap()
    );
    assert_eq!(materialized.surface_revision, timeline.surface_revision());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordinary_and_durable_appends_keep_every_physical_line_parseable() {
    const N: usize = 100;
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(dir.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let ordinary = adapter.clone();
    let durable = adapter.clone();
    let info_a = info.clone();
    let info_b = info.clone();
    let ordinary = tokio::spawn(async move {
        for index in 0..N {
            ordinary
                .append_update(&info_a, &update(&info_a, format!("ordinary-{index}")))
                .await
                .unwrap();
        }
    });
    let durable = tokio::spawn(async move {
        for index in 0..N {
            durable
                .append_update_durable_commit_aware(
                    &info_b,
                    &update(&info_b, format!("durable-{index}")),
                )
                .await
                .unwrap();
        }
    });
    ordinary.await.unwrap();
    durable.await.unwrap();

    let journal = decode_journal(&adapter.session_dir(&info).join(TIMELINE_FILE)).unwrap();
    assert!(journal.events.is_empty());
    assert_eq!(journal.history.sources.len(), N * 2);
}

#[tokio::test]
async fn append_commit_is_reported_when_bookkeeping_fails() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(dir.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let summary = adapter.session_dir(&info).join("summary.json");
    std::fs::remove_file(&summary).unwrap();
    std::fs::create_dir(&summary).unwrap();

    assert!(matches!(
        adapter
            .append_update_durable_commit_aware(&info, &update(&info, "committed".into()))
            .await,
        Err(crate::session::storage::AppendUpdateError::Committed(_))
    ));
    assert_eq!(
        decode_journal(&adapter.session_dir(&info).join(TIMELINE_FILE))
            .unwrap()
            .history
            .sources
            .len(),
        1
    );
}

#[tokio::test]
async fn directory_barrier_failure_is_retried_even_after_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let probe_attempts = Arc::clone(&attempts);
    let adapter =
        JsonlStorageAdapter::with_timeline_sync_probe(dir.path().to_path_buf(), move |point| {
            if point == TimelineSyncPoint::Directory
                && probe_attempts.fetch_add(1, Ordering::AcqRel) == 0
            {
                Err(io::Error::other("directory barrier failed"))
            } else {
                Ok(())
            }
        });
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    let event = timeline_event("directory-retry", &mut timeline);
    assert!(
        adapter
            .append_timeline_event_durable(&info, &event)
            .await
            .is_err()
    );
    adapter
        .append_timeline_event_durable(&info, &event)
        .await
        .unwrap();
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(
        decode_journal(&adapter.timeline_file(&info))
            .unwrap()
            .events
            .len(),
        1
    );
}

#[tokio::test]
async fn file_barrier_error_propagates() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let adapter =
        JsonlStorageAdapter::with_timeline_sync_probe(dir.path().to_path_buf(), |point| {
            if point == TimelineSyncPoint::File {
                Err(io::Error::other("file barrier failed"))
            } else {
                Ok(())
            }
        });
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    let event = timeline_event("file-barrier", &mut timeline);
    let error = adapter
        .append_timeline_event_durable(&info, &event)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "file barrier failed");
}

#[tokio::test]
async fn unified_admission_owns_one_body_and_preserves_candidate_order() {
    let root = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let timeline =
        super::tests::response_projection_production_timeline("request", 1, "only canonical body");
    for event in &timeline.events()[..4] {
        adapter
            .append_timeline_event_durable(&info, event)
            .await
            .unwrap();
    }
    let mut candidate = acp_event(&info, "provisional body must not be stored");
    candidate.meta.as_mut().unwrap().extend(
        serde_json::json!({"samplingRequestId":"request","samplingAttempt":1})
            .as_object()
            .unwrap()
            .clone(),
    );
    adapter
        .append_acp_event_exact(&info, &candidate)
        .await
        .unwrap();
    let notice = acp_event(&info, "independent after anchor");
    let mut notice = notice;
    notice
        .meta
        .as_mut()
        .unwrap()
        .insert("eventId".into(), "independent".into());
    adapter
        .append_acp_event_exact(&info, &notice)
        .await
        .unwrap();
    let admission = timeline.events().last().unwrap();
    adapter
        .append_timeline_event_durable(&info, admission)
        .await
        .unwrap();
    adapter
        .append_timeline_event_durable(&info, admission)
        .await
        .unwrap();
    let path = adapter.session_dir(&info).join("timeline.jsonl");
    let bytes = std::fs::read_to_string(&path).unwrap();
    assert_eq!(bytes.matches("only canonical body").count(), 1);
    assert!(!bytes.contains("provisional body"));
    assert!(!adapter.session_dir(&info).join("updates.jsonl").exists());
    let loaded = adapter.load_session(&info).await.unwrap();
    let texts = loaded
        .updates
        .into_iter()
        .filter_map(|value| match value {
            SessionUpdate::Acp(value) => match value.update {
                acp::SessionUpdate::AgentMessageChunk(chunk) => match chunk.content {
                    acp::ContentBlock::Text(value) => Some(value.text),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["only canonical body", "independent after anchor"]);
    let conflict =
        super::tests::response_projection_production_timeline("request", 1, "conflicting body");
    assert_eq!(
        adapter
            .append_timeline_event_durable(&info, conflict.events().last().unwrap())
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
}

#[tokio::test]
async fn unified_presentation_exact_retry_rejects_conflicts_and_foreign_owner() {
    let root = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let event = acp_event(&info, "original");
    adapter.append_acp_event_exact(&info, &event).await.unwrap();
    adapter.append_acp_event_exact(&info, &event).await.unwrap();
    let before = std::fs::read(adapter.session_dir(&info).join("timeline.jsonl")).unwrap();
    let conflict = acp_event(&info, "different");
    assert!(
        adapter
            .append_acp_event_exact(&info, &conflict)
            .await
            .is_err()
    );
    let mut foreign = event;
    foreign.session_id = acp::SessionId::new("foreign");
    assert!(
        adapter
            .append_acp_event_exact(&info, &foreign)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(adapter.session_dir(&info).join("timeline.jsonl")).unwrap(),
        before
    );
}

#[tokio::test]
async fn unified_presentation_resolves_post_commit_bookkeeping_failure() {
    let root = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let summary = adapter.session_dir(&info).join("summary.json");
    std::fs::remove_file(&summary).unwrap();
    std::fs::create_dir(&summary).unwrap();
    let event = acp_event(&info, "committed");
    assert!(matches!(
        adapter.append_acp_event_exact(&info, &event).await,
        Err(super::super::AppendUpdateError::Committed(_))
    ));
    let path = adapter.session_dir(&info).join("timeline.jsonl");
    let before = std::fs::read(&path).unwrap();
    adapter.append_acp_event_exact(&info, &event).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), before);
}

fn acp_event(info: &Info, text: &str) -> acp::SessionNotification {
    let mut notification = match update(info, text.to_owned()) {
        SessionUpdate::Acp(notification) => *notification,
        _ => unreachable!(),
    };
    notification.meta = Some(
        serde_json::json!({"eventId": "event-sync-retry"})
            .as_object()
            .unwrap()
            .clone(),
    );
    notification
}

async fn exact_acp_retries_after_sync_failure(point: TimelineSyncPoint) {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let fail_sync = Arc::new(AtomicBool::new(true));
    let probe_state = Arc::clone(&fail_sync);
    let adapter = JsonlStorageAdapter::with_timeline_sync_probe(
        dir.path().join("session"),
        move |observed| {
            if observed == point && probe_state.load(Ordering::SeqCst) {
                Err(io::Error::other("injected update sync failure"))
            } else {
                Ok(())
            }
        },
    );
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let expected = acp_event(&info, "answer");

    assert!(matches!(
        adapter.append_acp_event_exact(&info, &expected).await,
        Err(crate::session::storage::AppendUpdateError::NotCommitted(_))
    ));
    let path = adapter.session_dir(&info).join("timeline.jsonl");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    fail_sync.store(false, Ordering::SeqCst);
    adapter
        .append_acp_event_exact(&info, &expected)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    fail_sync.store(true, Ordering::SeqCst);
    assert!(matches!(
        adapter.append_acp_event_exact(&info, &expected).await,
        Err(crate::session::storage::AppendUpdateError::NotCommitted(_))
    ));
    fail_sync.store(false, Ordering::SeqCst);
    adapter
        .append_acp_event_exact(&info, &expected)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
}

#[tokio::test]
async fn exact_acp_file_sync_failure_is_not_acknowledged_and_retries() {
    exact_acp_retries_after_sync_failure(TimelineSyncPoint::File).await;
}

#[tokio::test]
async fn exact_acp_directory_sync_failure_is_not_acknowledged_and_retries() {
    exact_acp_retries_after_sync_failure(TimelineSyncPoint::Directory).await;
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_fullfsync_seam_reports_invalid_descriptor() {
    assert!(super::super::fullfsync_raw(-1).is_err());
}

#[test]
fn append_lock_wait_is_bounded() {
    use fs2::FileExt as _;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline.jsonl");
    let lock_path = path.with_extension("jsonl.lock");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .unwrap();
    held.lock_exclusive().unwrap();

    let started = std::time::Instant::now();
    let error =
        JsonlStorageAdapter::lock_append_with_timeout(&path, std::time::Duration::from_millis(25))
            .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    held.unlock().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn writer_load_reuses_only_its_fully_validated_prefix() {
    let root = tempfile::tempdir().unwrap();
    let info = info();
    let creator = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    creator
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    let first = timeline_event("first", &mut timeline);
    creator
        .append_timeline_event_durable(&info, &first)
        .await
        .unwrap();
    drop(creator);

    let writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    writer.load_session_without_updates(&info).await.unwrap();
    assert!(
        writer
            .timeline_prefix_state(&info)
            .unwrap()
            .lock()
            .unwrap()
            .is_none()
    );
    writer
        .load_session_for_write_without_updates(&info)
        .await
        .unwrap();
    writer
        .load_session_for_write_without_updates(&info)
        .await
        .unwrap();
    let state = writer.timeline_prefix_state(&info).unwrap();
    let prefix = state.lock().unwrap().clone().unwrap();
    let path = writer.timeline_file(&info);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(prefix.hash, blake3::hash(&bytes));
    assert_eq!(prefix.len, bytes.len() as u64);
    let second = timeline_event("second", &mut timeline);
    writer
        .append_timeline_event_durable(&info, &second)
        .await
        .unwrap();
    assert_eq!(
        state.lock().unwrap().as_ref().unwrap().hash,
        blake3::hash(&std::fs::read(&path).unwrap())
    );

    // A valid same-length rewrite must not be blessed by a repeated load.
    let mut replacement_timeline = chat_state::Timeline::default();
    let replacement = timeline_event("other", &mut replacement_timeline);
    let replacement_bytes = encode_journal(&[replacement, second.clone()]);
    assert_eq!(
        replacement_bytes.len(),
        std::fs::metadata(&path).unwrap().len() as usize
    );
    std::fs::write(&path, replacement_bytes).unwrap();
    assert_eq!(
        writer
            .load_session_for_write_without_updates(&info)
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[cfg(unix)]
#[tokio::test]
async fn writer_load_does_not_install_prefix_when_prompt_validation_fails() {
    let root = tempfile::tempdir().unwrap();
    let info = info();
    let creator = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    creator
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let body = b"required prompt";
    let hash = blake3::hash(body).to_hex().to_string();
    let reference = format!(
        "{}{hash}",
        crate::session::persistence::PROMPT_BLOB_REF_PREFIX
    );
    let timeline =
        chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(reference)])
            .unwrap();
    for event in timeline.events() {
        creator
            .append_timeline_event_durable(&info, event)
            .await
            .unwrap();
    }
    drop(creator);
    let writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    assert!(
        writer
            .load_session_for_write_without_updates(&info)
            .await
            .is_err()
    );
    assert!(
        writer
            .timeline_prefix_state(&info)
            .unwrap()
            .lock()
            .unwrap()
            .is_none()
    );
    crate::session::persistence::write_immutable_blob(
        &writer.session_dir(&info),
        &Path::new("prompts").join(format!("{hash}.txt")),
        body,
    )
    .unwrap();
    writer
        .load_session_for_write_without_updates(&info)
        .await
        .unwrap();
    assert!(
        writer
            .timeline_prefix_state(&info)
            .unwrap()
            .lock()
            .unwrap()
            .is_some()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn writer_load_candidate_rejects_rewrite_replacement_and_truncation_before_install() {
    for mutation in ["rewrite", "replace", "truncate"] {
        let root = tempfile::tempdir().unwrap();
        let info = info();
        let creator = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        creator
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let event = timeline_event("first", &mut timeline);
        creator
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        drop(creator);
        let writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        writer.ensure_writer_lease(&info).unwrap();
        let opened = writer.open_session(&info).unwrap();
        let (_, prefix) = writer
            .load_validated_writer_timeline(&opened, &info)
            .unwrap();
        let path = writer.timeline_file(&info);
        let bytes = std::fs::read(&path).unwrap();
        match mutation {
            "rewrite" => {
                let mut replacement_timeline = chat_state::Timeline::default();
                let replacement = timeline_event("other", &mut replacement_timeline);
                std::fs::write(&path, encode_journal(&[replacement])).unwrap();
            }
            "replace" => {
                let replacement = path.with_extension("replacement");
                std::fs::write(&replacement, bytes).unwrap();
                std::fs::rename(replacement, &path).unwrap();
            }
            "truncate" => std::fs::write(&path, b"").unwrap(),
            _ => unreachable!(),
        }
        assert_eq!(
            writer
                .install_validated_writer_prefix(&opened, &info, prefix.unwrap())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData,
            "{mutation}"
        );
        assert!(
            writer
                .timeline_prefix_state(&info)
                .unwrap()
                .lock()
                .unwrap()
                .is_none()
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn writer_load_torn_tail_and_same_bytes_replacement_keep_append_validation() {
    for replace in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let info = info();
        let creator = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        creator
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let first = timeline_event("first", &mut timeline);
        creator
            .append_timeline_event_durable(&info, &first)
            .await
            .unwrap();
        let path = creator.timeline_file(&info);
        let committed = std::fs::read(&path).unwrap();
        if !replace {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(b"{partial")
                .unwrap();
        }
        drop(creator);
        let writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        writer
            .load_session_for_write_without_updates(&info)
            .await
            .unwrap();
        assert_eq!(
            writer
                .timeline_prefix_state(&info)
                .unwrap()
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .len,
            committed.len() as u64
        );
        if replace {
            let replacement = path.with_extension("replacement");
            std::fs::write(&replacement, &committed).unwrap();
            std::fs::rename(replacement, &path).unwrap();
        }
        let second = timeline_event("second", &mut timeline);
        writer
            .append_timeline_event_durable(&info, &second)
            .await
            .unwrap();
        let events = writer
            .open_session(&info)
            .unwrap()
            .timeline_events()
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            serde_json::to_value(events).unwrap(),
            serde_json::to_value([first, second]).unwrap()
        );
    }
}

#[tokio::test]
async fn notice_reference_validates_body_and_source_before_discarding_payload() {
    use crate::extensions::notification::{
        AgentMessageNotice, SessionNotification, SessionUpdate as GrowUpdate,
    };
    use chat_state::{
        NotificationEvent, NotificationSource, NotificationSourceVersion, TimelineEventKind,
    };
    let root = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let opened = adapter.open_session(&info).unwrap();
    let body = "the canonical parent message";
    let payload_ref =
        crate::session::notification_inbox::write_payload(opened.directory(), body).unwrap();
    let source = NotificationSource::ParentMessage {
        parent_session_id: "parent".into(),
        message_id: "message".into(),
        interrupt: true,
    };
    let source_version = NotificationSourceVersion::Ordinal {
        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let id = chat_state::notification_id(info.id.0.as_ref(), &source, &source_version).unwrap();
    let mut timeline = chat_state::Timeline::default();
    let receipt = timeline
        .record(TimelineEventKind::Notification(
            NotificationEvent::Received {
                id,
                owner_session_id: info.id.0.to_string(),
                source,
                source_version,
                payload_ref,
            },
        ))
        .unwrap();
    adapter
        .append_timeline_event_durable(&info, &receipt)
        .await
        .unwrap();
    let notice =
        crate::session::notification_inbox::parent_message_notice(&receipt, Some(body.into()))
            .unwrap();
    let wrap = |notice, event_id: &str| {
        SessionUpdate::Grow(Box::new(SessionNotification {
            session_id: info.id.clone(),
            update: GrowUpdate::UiNotice(notice),
            meta: Some(serde_json::json!({"eventId":event_id})),
        }))
    };
    let valid = wrap(notice.clone(), "notice-exact");
    adapter
        .append_update_durable_commit_aware(&info, &valid)
        .await
        .unwrap();
    adapter
        .append_update_durable_commit_aware(&info, &valid)
        .await
        .unwrap();
    let path = adapter.session_dir(&info).join(TIMELINE_FILE);
    let before = std::fs::read(&path).unwrap();
    for mutation in ["body", "source", "display"] {
        let mut wrong = notice.clone();
        let mut details = AgentMessageNotice::from_notice(&wrong).unwrap();
        match mutation {
            "body" => details.message = Some("different body".into()),
            "source" => details.source_session_id = "wrong-parent".into(),
            "display" => wrong.description = Some("different display".into()),
            _ => unreachable!(),
        }
        wrong.details = Some(serde_json::to_string(&details).unwrap());
        for event_id in ["notice-exact", "new-invalid-notice"] {
            assert!(
                adapter
                    .append_update_durable_commit_aware(&info, &wrap(wrong.clone(), event_id))
                    .await
                    .is_err(),
                "{mutation} must not be erased by conversion to a source reference"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }
    let loaded = adapter.load_session(&info).await.unwrap();
    let notices = loaded.updates.iter().filter(|update| matches!(update, SessionUpdate::Grow(notification) if matches!(&notification.update, GrowUpdate::UiNotice(_)))).count();
    assert_eq!(
        notices, 1,
        "one canonical receipt at its original presentation position"
    );
}

#[tokio::test]
async fn incoming_inquiry_notices_are_bound_to_typed_source_facts() {
    use crate::coordination::{IncomingInquiryAudit, InquiryDirection, InquiryEvent};
    use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};

    let root = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let path = adapter.session_dir(&info).join(TIMELINE_FILE);
    let mut timeline = chat_state::Timeline::default();

    let incoming = |source_peer_id: &str, source_session_id: &str| InquiryEvent::Incoming {
        inquiry_id: "shared-inquiry".into(),
        audit: IncomingInquiryAudit {
            direction: InquiryDirection::ParentToChild,
            source_peer_id: source_peer_id.into(),
            source_session_id: source_session_id.into(),
            source_cwd: "/source/workspace".into(),
            delegated_subagent_task_name: None,
            question: "Can you inspect this?".into(),
            approval: None,
            outcome: None,
        },
    };
    let wrap = |notice, event_id: &str| {
        SessionUpdate::Grow(Box::new(SessionNotification {
            session_id: info.id.clone(),
            update: GrowUpdate::UiNotice(notice),
            meta: Some(serde_json::json!({"eventId": event_id})),
        }))
    };
    let mut stored_notices = Vec::new();
    for (peer, source_session, event_id) in [
        ("peer-a", "source-session-a", "inquiry-notice-a"),
        ("peer-b", "source-session-b", "inquiry-notice-b"),
    ] {
        let fact = incoming(peer, source_session);
        let event = timeline.record(fact.timeline_kind()).unwrap();
        adapter
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        let notice = fact.notice();
        adapter
            .append_update_durable_commit_aware(&info, &wrap(notice.clone(), event_id))
            .await
            .unwrap();
        stored_notices.push(notice);
    }

    let loaded = adapter.load_session(&info).await.unwrap();
    let notices = loaded
        .updates
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Grow(notification) = update else {
                return None;
            };
            let GrowUpdate::UiNotice(notice) = &notification.update else {
                return None;
            };
            Some(notice)
        })
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 2);
    assert_eq!(
        notices
            .iter()
            .map(|notice| {
                IncomingInquiryAudit::from_notice(notice)
                    .unwrap()
                    .source_session_id
            })
            .collect::<Vec<_>>(),
        ["source-session-a", "source-session-b"]
    );

    let before_invalid = std::fs::read(&path).unwrap();
    let mut unknown_source = stored_notices[0].clone();
    let mut audit = IncomingInquiryAudit::from_notice(&unknown_source).unwrap();
    audit.source_session_id = "no-domain-source".into();
    unknown_source.details = Some(serde_json::to_string(&audit).unwrap());
    assert!(
        adapter
            .append_update_durable_commit_aware(
                &info,
                &wrap(unknown_source, "unknown-source-event"),
            )
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before_invalid);

    let mut malformed = stored_notices[0].clone();
    malformed.details = Some("{malformed details".into());
    for event_id in ["malformed-new-event", "inquiry-notice-a"] {
        assert!(
            adapter
                .append_update_durable_commit_aware(&info, &wrap(malformed.clone(), event_id))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before_invalid);
    }

    // Transport peer identity may change while the source session and inquiry
    // phase are the same; this must not create a second display phase.
    let duplicate = incoming("reconnected-peer", "source-session-a");
    let duplicate_event = timeline.record(duplicate.timeline_kind()).unwrap();
    adapter
        .append_timeline_event_durable(&info, &duplicate_event)
        .await
        .unwrap();
    let exact_retry = wrap(stored_notices[0].clone(), "inquiry-notice-a");
    adapter
        .append_update_durable_commit_aware(&info, &exact_retry)
        .await
        .unwrap();

    let loaded = adapter.load_session(&info).await.unwrap();
    let notices = loaded
        .updates
        .iter()
        .filter(|update| matches!(update, SessionUpdate::Grow(notification) if matches!(&notification.update, GrowUpdate::UiNotice(_))))
        .count();
    assert_eq!(notices, 2, "the repeated source phase remains one notice");
}
