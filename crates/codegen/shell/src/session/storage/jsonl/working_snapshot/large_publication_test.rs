use std::path::Path;
use std::time::Instant;

use chat_state::{
    EventSeq, MessageCause, MessageEvent, ObservationEvent, StepEvent, StepId, SurfaceOp,
    TimelineEvent, TimelineEventKind, ToolEvent, TurnEvent, TurnId,
};

const LARGE_PUBLICATION_TEST: &str = "session::storage::jsonl::working_snapshot::tests::large_index_process_exit_around_incremental_publication_recovers_fixed_tail";
const TOOL_PAIRS: usize = 30_000;
const TOOL_PAYLOAD_BYTES: usize = 8 * 1024;
const CRASH_TAIL_BYTES: usize = 2 * 1024 * 1024;
const SESSION_ID: &str = "large-index-crash";
const FIXED_CUT_NEXT_SEQ: u64 = 8 + (TOOL_PAIRS as u64 * 2);
const PUBLISHED_NEXT_SEQ: u64 = FIXED_CUT_NEXT_SEQ + 1;

fn event(seq: u64, kind: TimelineEventKind) -> TimelineEvent {
    TimelineEvent {
        version: chat_state::TIMELINE_SCHEMA_VERSION,
        seq: EventSeq::new(seq),
        at_ms: 1,
        kind,
    }
}

async fn append(
    storage: &JsonlStorageAdapter,
    info: &Info,
    next_seq: &mut u64,
    kind: TimelineEventKind,
    durable: bool,
) -> TimelineEvent {
    let committed = event(*next_seq, kind);
    if durable {
        storage
            .append_timeline_event_durable(info, &committed)
            .await
            .unwrap();
    } else {
        storage
            .append_timeline_event(info, &committed)
            .await
            .unwrap();
    }
    *next_seq += 1;
    committed
}

fn turn_start(
    id: TurnId,
    prompt: &str,
    input_message_count: usize,
    prompt_index: usize,
) -> TimelineEventKind {
    TimelineEventKind::Turn(TurnEvent::Started {
        id,
        prompt_id: format!("large-index-prompt-{}", id.0),
        input_ids: Vec::new(),
        identity: chat_state::TurnIdentity {
            origin: "user".into(),
            turn_kind: "internal".into(),
            goal_id: None,
            goal_definition_revision: None,
            stage_id: None,
        },
        model_id: "mock/mock".into(),
        input_message_count,
        prompt_index,
        prompt_text: prompt.into(),
        input_kind: chat_state::TurnInputKind::Prompt,
        redirect_kind: None,
    })
}

fn observation(label: &str) -> TimelineEventKind {
    TimelineEventKind::Observation(ObservationEvent {
        scope: "shell".into(),
        name: "artifact".into(),
        turn: None,
        step: None,
        data: Some(serde_json::Value::String(format!(
            "{label}:{}",
            "x".repeat(CRASH_TAIL_BYTES)
        ))),
    })
}

fn fixture(root: &Path) -> (JsonlStorageAdapter, Info) {
    (
        JsonlStorageAdapter::with_root(root.to_path_buf()),
        Info {
            id: acp::SessionId::new(SESSION_ID),
            cwd: root.to_string_lossy().into_owned(),
        },
    )
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "manual >515MB process-crash acceptance; run: cargo test --locked -p shell --lib session::storage::jsonl::working_snapshot::tests::large_index_process_exit_around_incremental_publication_recovers_fixed_tail -- --ignored --exact --nocapture --test-threads=1"]
async fn large_index_process_exit_around_incremental_publication_recovers_fixed_tail() {
    if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
        let (storage, info) = fixture(Path::new(&root));
        let stage = std::env::var(CRASH_STAGE_ENV).unwrap();
        let mut next_seq = if stage == "advance-before" {
            PUBLISHED_NEXT_SEQ
        } else {
            PUBLISHED_NEXT_SEQ + 1
        };
        let label = if stage == "advance-before" {
            "fixed-crash-tail"
        } else {
            "post-recovery-force"
        };
        append(&storage, &info, &mut next_seq, observation(label), true).await;
        panic!("large publication crash probe was not reached at {stage}");
    }

    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    let setup_started = Instant::now();
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();

    let mut initial = chat_state::Timeline::default();
    let seed = initial
        .append(
            sampling_types::ConversationItem::system("fixed active context"),
            MessageCause::Seed,
        )
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &seed)
        .await
        .unwrap();
    let mut next_seq = 1;
    let closed_turn = TurnId(1);
    let closed_step = StepId {
        turn: closed_turn,
        index: 0,
    };
    append(
        &storage,
        &info,
        &mut next_seq,
        turn_start(closed_turn, "closed lifecycle history", 1, 0),
        false,
    )
    .await;
    append(
        &storage,
        &info,
        &mut next_seq,
        TimelineEventKind::Step(StepEvent::Started { id: closed_step }),
        false,
    )
    .await;

    for index in 0..TOOL_PAIRS {
        let call_id = format!("large-index-tool-{index:08}");
        append(
            &storage,
            &info,
            &mut next_seq,
            TimelineEventKind::Tool(ToolEvent::Started {
                call_id: call_id.clone(),
                turn: closed_turn,
                step: closed_step,
                name: "shell.exec".into(),
                input: Some(serde_json::json!({
                    "command": "inspect workspace and summarize the result",
                    "context": "x".repeat(TOOL_PAYLOAD_BYTES),
                })),
            }),
            false,
        )
        .await;
        append(
            &storage,
            &info,
            &mut next_seq,
            TimelineEventKind::Tool(ToolEvent::Completed {
                call_id,
                name: "shell.exec".into(),
                outcome: "completed".into(),
                duration_ms: 12,
                details: Some(serde_json::json!({
                    "exit_code": 0,
                    "stdout": "x".repeat(TOOL_PAYLOAD_BYTES),
                })),
            }),
            false,
        )
        .await;
    }
    append(
        &storage,
        &info,
        &mut next_seq,
        TimelineEventKind::Step(StepEvent::Ended {
            id: closed_step,
            outcome: "completed".into(),
            duration_ms: 12,
        }),
        false,
    )
    .await;
    append(
        &storage,
        &info,
        &mut next_seq,
        TimelineEventKind::Turn(TurnEvent::Ended {
            id: closed_turn,
            outcome: "completed".into(),
            duration_ms: 12,
            tool_count: TOOL_PAIRS as u32,
            terminal: chat_state::TurnTerminal {
                source: chat_state::TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }),
        false,
    )
    .await;

    let active_turn = TurnId(2);
    let active_step = StepId {
        turn: active_turn,
        index: 0,
    };
    append(
        &storage,
        &info,
        &mut next_seq,
        turn_start(active_turn, "fixed active prompt", 1, 1),
        false,
    )
    .await;
    append(
        &storage,
        &info,
        &mut next_seq,
        TimelineEventKind::Messages(MessageEvent {
            cause: MessageCause::User,
            items: vec![sampling_types::ConversationItem::user(
                "fixed active prompt",
            )],
            surface: SurfaceOp::Append,
            response_admission: None,
        }),
        false,
    )
    .await;
    append(
        &storage,
        &info,
        &mut next_seq,
        TimelineEventKind::Step(StepEvent::Started { id: active_step }),
        true,
    )
    .await;
    assert_eq!(next_seq, FIXED_CUT_NEXT_SEQ);
    // A large inert diagnostic forces publication of any bounded suffix left
    // by the preceding active-state write, without changing the model context.
    append(
        &storage,
        &info,
        &mut next_seq,
        observation("fixed-published-padding"),
        true,
    )
    .await;
    assert_eq!(next_seq, PUBLISHED_NEXT_SEQ);

    let session_directory = storage.open_session(&info).unwrap().directory_handle();
    let journal_path = session_directory.display_path().join(TIMELINE_FILE);
    let history_bytes = std::fs::metadata(&journal_path).unwrap().len();
    assert!(
        history_bytes > 515_000_000,
        "closed Tool lifecycle history is only {history_bytes} bytes"
    );
    let ready = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    assert_eq!(ready.timeline.surface_len(), 2);
    assert_eq!(ready.timeline.next_seq().get(), PUBLISHED_NEXT_SEQ);
    let base_manifest = read_manifest(&session_directory).unwrap().unwrap();
    assert_eq!(base_manifest.bytes, history_bytes);
    assert_eq!(base_manifest.domain_events, PUBLISHED_NEXT_SEQ);
    assert_eq!(base_manifest.tool_frontier.domain, PUBLISHED_NEXT_SEQ);
    drop(ready);
    drop(storage);

    let before_status = run_crash_stage(root.path(), "advance-before");
    assert_eq!(before_status, Some(73));
    let after_before_crash = read_manifest(&session_directory).unwrap().unwrap();
    assert_eq!(after_before_crash.generation, base_manifest.generation);
    assert_eq!(after_before_crash.bytes, base_manifest.bytes);

    let recovery_started = Instant::now();
    let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let recovered = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let before_recovery_ms = recovery_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(recovered.timeline.surface_len(), 2);
    assert_eq!(recovered.timeline.next_seq().get(), PUBLISHED_NEXT_SEQ + 1);
    let recovered_manifest = read_manifest(&session_directory).unwrap().unwrap();
    assert_eq!(recovered_manifest.generation, base_manifest.generation);
    assert_eq!(recovered_manifest.bytes, base_manifest.bytes);
    assert_eq!(recovered_manifest.domain_events, PUBLISHED_NEXT_SEQ);
    assert_eq!(recovered_manifest.tool_frontier.domain, PUBLISHED_NEXT_SEQ);
    let fixed_tail = read_domain_range(
        &session_directory,
        PUBLISHED_NEXT_SEQ..PUBLISHED_NEXT_SEQ + 1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(fixed_tail.len(), 1);
    assert_eq!(fixed_tail[0].seq.get(), PUBLISHED_NEXT_SEQ);
    assert!(
        serde_json::to_string(&fixed_tail[0])
            .unwrap()
            .contains("fixed-crash-tail")
    );
    let fixed_tail_digest = blake3::hash(&serde_json::to_vec(&fixed_tail[0]).unwrap());
    let before_records = recovered_manifest.retained_records;
    storage
        .append_timeline_event_durable(&info, &fixed_tail[0])
        .await
        .unwrap();
    let retried_manifest = read_manifest(&session_directory).unwrap().unwrap();
    assert_eq!(retried_manifest.generation, base_manifest.generation);
    assert!(retried_manifest.bytes > base_manifest.bytes);
    assert_eq!(retried_manifest.domain_events, PUBLISHED_NEXT_SEQ + 1);
    assert_eq!(
        retried_manifest.tool_frontier.domain,
        PUBLISHED_NEXT_SEQ + 1
    );
    drop(recovered);
    drop(storage);

    let after_status = run_crash_stage(root.path(), "advance-after");
    assert_eq!(after_status, Some(73));
    let after_crash_manifest = read_manifest(&session_directory).unwrap().unwrap();
    assert_eq!(after_crash_manifest.generation, retried_manifest.generation);
    assert!(after_crash_manifest.bytes > retried_manifest.bytes);

    let recovery_started = Instant::now();
    let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let recovered = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let after_recovery_ms = recovery_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(recovered.timeline.surface_len(), 2);
    assert_eq!(recovered.timeline.next_seq().get(), PUBLISHED_NEXT_SEQ + 2);
    let final_manifest = read_manifest(&session_directory).unwrap().unwrap();
    assert_eq!(
        final_manifest.bytes,
        std::fs::metadata(&journal_path).unwrap().len()
    );
    assert_eq!(final_manifest.domain_events, PUBLISHED_NEXT_SEQ + 2);
    assert_eq!(final_manifest.tool_frontier.domain, PUBLISHED_NEXT_SEQ + 2);
    let exact_tail = read_domain_range(
        &session_directory,
        PUBLISHED_NEXT_SEQ..PUBLISHED_NEXT_SEQ + 2,
    )
    .unwrap()
    .unwrap();
    assert_eq!(exact_tail.len(), 2);
    assert_eq!(exact_tail[0].seq.get(), PUBLISHED_NEXT_SEQ);
    assert_eq!(exact_tail[1].seq.get(), PUBLISHED_NEXT_SEQ + 1);
    assert!(
        serde_json::to_string(&exact_tail[0])
            .unwrap()
            .contains("fixed-crash-tail")
    );
    assert!(
        serde_json::to_string(&exact_tail[1])
            .unwrap()
            .contains("post-recovery-force")
    );
    assert_eq!(
        blake3::hash(&serde_json::to_vec(&exact_tail[0]).unwrap()),
        fixed_tail_digest,
        "the durable tail recovered after advance-before must be byte-identical after advance-after"
    );
    eprintln!(
        "large publication recovery: history_bytes={history_bytes}, setup_ms={:.1}, before_recovery_ms={before_recovery_ms:.1}, after_recovery_ms={after_recovery_ms:.1}, retained_records_before={before_records}, retained_records_after={}, indexed_domain_events={}, manifest_bytes={}",
        setup_started.elapsed().as_secs_f64() * 1000.0,
        final_manifest.retained_records,
        final_manifest.domain_events,
        final_manifest.bytes,
    );
}

fn run_crash_stage(root: &Path, stage: &str) -> Option<i32> {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            LARGE_PUBLICATION_TEST,
            "--nocapture",
            "--ignored",
        ])
        .env(CRASH_HOME_ENV, root)
        .env(CRASH_STAGE_ENV, stage)
        .status()
        .unwrap()
        .code()
}
