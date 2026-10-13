use super::*;
use crate::session::info::Info;
use crate::session::persistence::default_model_id;
use crate::session::storage::{SessionUpdate, StorageAdapter};
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
        let event = timeline_event(&format!("event-{index}"), &mut timeline);
        serde_json::to_writer(&mut bytes, &event).unwrap();
        // Whitespace is semantically insignificant, but must be included in
        // the immutable prefix digest just like the old whole-file reader.
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
    let initial_writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let error = initial_writer
        .append_timeline_event_durable(&parent_info, &skipped_event)
        .await
        .expect_err("a new ledger cannot begin at seq 1");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        std::fs::metadata(initial_writer.timeline_file(&parent_info))
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        0,
        "pre-write sequence rejection leaves no canonical bytes"
    );
    drop(initial_writer);

    let subagent_id = "finalization-matrix-child";
    let mut parent = chat_state::Timeline::default();
    let spawn = parent
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                subagent_id: subagent_id.into(),
                child_session_id: child_info.id.0.to_string(),
                security_parent_session_id: parent_info.id.0.to_string(),
                subagent_type: "worker".into(),
                description: "finalization boundary matrix".into(),
                prompt: "complete the task".into(),
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
            }),
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
    let original_line = serde_json::to_vec(&first).unwrap();
    let replacement_line = serde_json::to_vec(&replacement).unwrap();
    assert_eq!(original_line.len(), replacement_line.len());
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..replacement_line.len()].copy_from_slice(&replacement_line);
    std::fs::write(&path, bytes).unwrap();

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

    let lines = std::fs::read_to_string(path).unwrap();
    assert_eq!(lines.lines().count(), 2);
    let events = lines
        .lines()
        .map(|line| serde_json::from_str::<chat_state::TimelineEvent>(line).unwrap())
        .collect::<Vec<_>>();
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
fn update_append_and_replay_reject_symlinked_ledgers() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("outside.jsonl");
    let path = dir.path().join("updates.jsonl");
    std::fs::write(&target, b"outside\n").unwrap();
    symlink(&target, &path).unwrap();

    let error = JsonlStorageAdapter::append_jsonl_line_sync(
        &path,
        b"{\"record\":1}\n".to_vec(),
        AppendDurability::Buffered,
    )
    .expect_err("updates append must not follow a ledger symlink");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&target).unwrap(), b"outside\n");

    let Err(error) = crate::session::storage::UpdatesIterator::open(&path) else {
        panic!("updates replay must not follow a ledger symlink");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn timeline_reader_ignores_only_the_uncommitted_final_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline.jsonl");
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
    let mut timeline = chat_state::Timeline::default();
    let event = timeline_event("complete", &mut timeline);
    let mut bytes = serde_json::to_vec(&event).unwrap();
    bytes.extend_from_slice(b"\n{\"version\":");
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

    let bytes = std::fs::read(adapter.session_dir(&info).join("updates.jsonl")).unwrap();
    let parsed = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<SessionUpdateEnvelope>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(parsed.len(), N * 2);
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
        std::fs::read_to_string(adapter.session_dir(&info).join("updates.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn directory_barrier_failure_is_retried_even_after_file_exists() {
    let mut attempts = 0;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("updates.jsonl");
    let mut flaky_parent = || {
        attempts += 1;
        if attempts == 1 {
            Err(io::Error::other("directory barrier failed"))
        } else {
            Ok(())
        }
    };
    assert!(
        JsonlStorageAdapter::append_jsonl_line_sync_with(
            &path,
            b"{\"record\":1}\n".to_vec(),
            AppendDurability::Durable,
            std::fs::File::sync_all,
            &mut flaky_parent,
        )
        .is_err()
    );
    JsonlStorageAdapter::append_jsonl_line_sync_with(
        &path,
        b"{\"record\":1}\n".to_vec(),
        AppendDurability::Durable,
        std::fs::File::sync_all,
        &mut flaky_parent,
    )
    .unwrap();
    assert_eq!(attempts, 2);
}

#[test]
fn file_barrier_error_propagates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("updates.jsonl");
    let error = JsonlStorageAdapter::append_jsonl_line_sync_with(
        &path,
        b"{\"record\":1}\n".to_vec(),
        AppendDurability::Durable,
        |_| Err(io::Error::other("file barrier failed")),
        || Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "file barrier failed");
}

#[tokio::test]
async fn exact_projection_is_physically_idempotent_and_conflicts_on_payload() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(dir.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let first = projection(&info, "answer");

    adapter
        .commit_response_projection(&info, &first)
        .await
        .unwrap();
    adapter
        .commit_response_projection(&info, &first)
        .await
        .unwrap();
    let path = adapter.session_dir(&info).join("updates.jsonl");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    let mut conflict = first.clone();
    conflict.timeline_event += 1;
    let error = adapter
        .commit_response_projection(&info, &conflict)
        .await
        .unwrap_err()
        .into_io_error();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    let conflict = projection(&info, "different answer");
    let error = adapter
        .commit_response_projection(&info, &conflict)
        .await
        .unwrap_err()
        .into_io_error();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
}

#[tokio::test]
async fn exact_projection_reconciles_a_post_commit_bookkeeping_failure() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(dir.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let expected = projection(&info, "answer");
    let summary = adapter.session_dir(&info).join("summary.json");
    std::fs::remove_file(&summary).unwrap();
    std::fs::create_dir(&summary).unwrap();

    adapter
        .commit_response_projection(&info, &expected)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(adapter.session_dir(&info).join("updates.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    adapter
        .commit_response_projection(&info, &expected)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(adapter.session_dir(&info).join("updates.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[tokio::test]
async fn exact_projection_pre_commit_append_failure_is_not_committed() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let session_dir = dir.path().join("session");
    let adapter = JsonlStorageAdapter::with_update_append_probe(session_dir, |_| {
        Err(io::Error::other("append failed before commit"))
    });
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();

    let error = adapter
        .commit_response_projection(&info, &projection(&info, "answer"))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        crate::session::storage::AppendUpdateError::NotCommitted(_)
    ));
    let path = adapter.session_dir(&info).join("updates.jsonl");
    let physical_lines = match std::fs::read_to_string(path) {
        Ok(contents) => contents.lines().count(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => panic!("failed to inspect updates ledger: {error}"),
    };
    assert_eq!(physical_lines, 0);
}

#[tokio::test]
async fn exact_acp_event_is_physically_idempotent_and_conflicts_on_payload() {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let adapter = JsonlStorageAdapter::with_root(dir.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut first = match update(&info, "answer".into()) {
        SessionUpdate::Acp(notification) => *notification,
        _ => unreachable!(),
    };
    first.meta = Some(
        serde_json::json!({"eventId": "event-1"})
            .as_object()
            .unwrap()
            .clone(),
    );

    adapter.append_acp_event_exact(&info, &first).await.unwrap();
    adapter.append_acp_event_exact(&info, &first).await.unwrap();
    let path = adapter.session_dir(&info).join("updates.jsonl");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    let mut conflict = first.clone();
    conflict.update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("different")),
    ));
    let error = adapter
        .append_acp_event_exact(&info, &conflict)
        .await
        .unwrap_err()
        .into_io_error();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
}

async fn exact_projection_retries_after_sync_failure(point: UpdateSyncPoint) {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let fail_sync = Arc::new(AtomicBool::new(true));
    let probe_state = Arc::clone(&fail_sync);
    let adapter =
        JsonlStorageAdapter::with_update_sync_probe(dir.path().join("session"), move |observed| {
            if observed == point && probe_state.load(Ordering::SeqCst) {
                Err(io::Error::other("injected update sync failure"))
            } else {
                Ok(())
            }
        });
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let expected = projection(&info, "answer");

    assert!(matches!(
        adapter.commit_response_projection(&info, &expected).await,
        Err(crate::session::storage::AppendUpdateError::NotCommitted(_))
    ));
    let path = adapter.session_dir(&info).join("updates.jsonl");
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    fail_sync.store(false, Ordering::SeqCst);
    adapter
        .commit_response_projection(&info, &expected)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 1);

    fail_sync.store(true, Ordering::SeqCst);
    assert!(matches!(
        adapter.commit_response_projection(&info, &expected).await,
        Err(crate::session::storage::AppendUpdateError::NotCommitted(_))
    ));
    fail_sync.store(false, Ordering::SeqCst);
    adapter
        .commit_response_projection(&info, &expected)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
}

#[tokio::test]
async fn exact_projection_file_sync_failure_is_not_acknowledged_and_retries() {
    exact_projection_retries_after_sync_failure(UpdateSyncPoint::File).await;
}

#[tokio::test]
async fn exact_projection_directory_sync_failure_is_not_acknowledged_and_retries() {
    exact_projection_retries_after_sync_failure(UpdateSyncPoint::Directory).await;
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

async fn exact_acp_retries_after_sync_failure(point: UpdateSyncPoint) {
    let dir = tempfile::tempdir().unwrap();
    let info = info();
    let fail_sync = Arc::new(AtomicBool::new(true));
    let probe_state = Arc::clone(&fail_sync);
    let adapter =
        JsonlStorageAdapter::with_update_sync_probe(dir.path().join("session"), move |observed| {
            if observed == point && probe_state.load(Ordering::SeqCst) {
                Err(io::Error::other("injected update sync failure"))
            } else {
                Ok(())
            }
        });
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let expected = acp_event(&info, "answer");

    assert!(matches!(
        adapter.append_acp_event_exact(&info, &expected).await,
        Err(crate::session::storage::AppendUpdateError::NotCommitted(_))
    ));
    let path = adapter.session_dir(&info).join("updates.jsonl");
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
    exact_acp_retries_after_sync_failure(UpdateSyncPoint::File).await;
}

#[tokio::test]
async fn exact_acp_directory_sync_failure_is_not_acknowledged_and_retries() {
    exact_acp_retries_after_sync_failure(UpdateSyncPoint::Directory).await;
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
    let path = dir.path().join("updates.jsonl");
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
