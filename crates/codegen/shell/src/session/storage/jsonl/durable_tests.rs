use super::*;
use crate::session::info::Info;
use crate::session::persistence::default_model_id;
use crate::session::storage::{SessionUpdate, StorageAdapter};

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
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
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

    let bytes = std::fs::read(dir.path().join("updates.jsonl")).unwrap();
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
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let summary = dir.path().join("summary.json");
    std::fs::remove_file(&summary).unwrap();
    std::fs::create_dir(&summary).unwrap();

    assert!(matches!(
        adapter
            .append_update_durable_commit_aware(&info, &update(&info, "committed".into()))
            .await,
        Err(crate::session::storage::AppendUpdateError::Committed(_))
    ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("updates.jsonl"))
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
