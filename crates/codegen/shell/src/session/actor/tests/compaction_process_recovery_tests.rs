//! Real JSONL writers killed after durable append but before the ChatState ACK.
//! Each recovery runs in a fresh OS process; no in-memory Timeline is shared.
use super::support::*;
use super::*;
use crate::session::persistence;
use crate::session::timeline_persistence::ChannelTimelinePersistence;
use chat_state::{CompactionEvent, TimelineEventKind, TimelinePersistence};
use serde_json::json;
use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

const TEST: &str = "session::actor::tests::compaction_process_recovery_tests::compaction_real_files_process_recovery";
const MODE: &str = "GROW_TEST_COMPACTION_PROCESS_MODE";
const CUT: &str = "GROW_TEST_COMPACTION_PROCESS_CUT";
const ID: &str = "process-compaction";
const ROLE: &str = "retained active authority";
const SUMMARY: &str = "Verified old work summarized before process interruption.";
const RETRY_SUMMARY: &str = "A different summary from a new attempt over the same old range.";

fn info(root: &Path) -> crate::session::info::Info {
    crate::session::info::Info {
        id: acp::SessionId::new("compaction-process-fixture"),
        cwd: root.join("workspace").to_string_lossy().into_owned(),
    }
}

fn publish_json(path: &Path, value: serde_json::Value) {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(&value).unwrap()).unwrap();
    std::fs::rename(temporary, path).unwrap();
}

/// Only the return ACK is intercepted. Production SessionPersistence owns
/// encoding, append, file/directory sync, artifact validation and writer lease.
struct HoldDurableAck {
    inner: ChannelTimelinePersistence,
    cut: String,
    ready: PathBuf,
}

impl TimelinePersistence for HoldDurableAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<io::Result<()>> {
        let hold = match (&*self.cut, &event.kind) {
            ("promoted", TimelineEventKind::Compaction(CompactionEvent::Promoted { .. }))
            | ("summary", TimelineEventKind::Compaction(CompactionEvent::Summary { .. }))
            | ("completed", TimelineEventKind::Compaction(CompactionEvent::Completed { .. })) => {
                true
            }
            ("replacement", TimelineEventKind::Messages(message)) => {
                message.cause == chat_state::MessageCause::Compaction
            }
            ("control", TimelineEventKind::Control(control)) => {
                control.model_contexts.iter().any(|context| {
                    context.activation == chat_state::ControlContextActivation::Reprojection
                })
            }
            _ => false,
        };
        let receipt = self.inner.persist_timeline_event_and_ack(event);
        if !hold {
            return receipt;
        }
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let ready = self.ready.clone();
        let event = event.clone();
        tokio::spawn(async move {
            receipt
                .await
                .expect("production persistence receipt")
                .expect("real durable append");
            publish_json(&ready, json!({"pid": std::process::id(), "event": event}));
            // Keep the ACK unresolved, including after the ready marker. The
            // parent kills this process; no graceful cancellation runs here.
            std::future::pending::<()>().await;
            drop(reply);
        });
        receiver
    }
    fn flush(&mut self) {
        self.inner.flush();
    }
}

async fn build_actor(root: &Path, recovering: bool, cut: &str) -> SessionActor {
    let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
    let (unused_persistence, _) = tokio::sync::mpsc::unbounded_channel();
    let (mut actor, _) =
        create_test_actor_with_planning_receiver(0, 100_000, 85, gateway, unused_persistence).await;
    let info = info(root);
    actor.session_info.id = info.id.clone();
    actor.session_info.cwd = info.cwd.clone();
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let (handle, timeline) = if recovering {
        let (loaded, handle) = persistence::load_light(&info, None, true).await.unwrap();
        actor.control_revision.store(
            loaded
                .control_snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.control_revision),
            std::sync::atomic::Ordering::SeqCst,
        );
        (handle, Some(loaded.timeline))
    } else {
        (
            persistence::new(&info, persistence::default_model_id(), None)
                .await
                .unwrap(),
            None,
        )
    };
    let writer = Box::new(HoldDurableAck {
        inner: ChannelTimelinePersistence::new(handle.tx.clone()),
        cut: cut.into(),
        ready: root.join("ready.json"),
    });
    actor.notifications.persistence_tx = handle.tx;
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let cancel = tokio_util::sync::CancellationToken::new();
    actor.chat_state_handle = if let Some(timeline) = timeline {
        chat_state::ChatStateActor::spawn_from_validated_timeline(
            timeline, config, None, writer, events, cancel,
        )
        .await
        .unwrap()
    } else {
        chat_state::ChatStateActor::spawn(
            vec![ConversationItem::system("process fixture system")],
            config,
            writer,
            events,
            cancel,
        )
    };
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    actor
}

async fn write_until_interrupted(actor: &SessionActor, root: &Path) {
    let handle = &actor.chat_state_handle;
    handle
        .push_user_message_durably(ConversationItem::user("old task"))
        .await
        .unwrap();
    actor
        .persist_control_snapshot_with_contexts_durably(
            actor.behavior.lock().snapshot(),
            None,
            None,
            vec![(
                chat_state::ControlContextLayer::AgentRole,
                chat_state::ControlContextActivation::Transition,
                ConversationItem::system_reminder(ROLE),
            )],
        )
        .await
        .unwrap();
    handle.push_assistant_response(ConversationItem::assistant("old verified answer"));
    handle
        .push_user_message_durably(ConversationItem::user("newer task retained verbatim"))
        .await
        .unwrap();
    handle.push_assistant_response(ConversationItem::assistant(
        "newer answer retained verbatim",
    ));
    let materialized = handle
        .materialize_timeline(info(root).id.to_string())
        .await
        .unwrap();
    let original = materialized.surface;
    let ids = materialized.surface_ids;
    publish_json(
        &root.join("expected.json"),
        json!({"surface": original, "ids": ids}),
    );
    commit_summary(actor, root, ID, SUMMARY).await;
    panic!("the selected write must remain pending until the parent kills this process");
}

async fn commit_summary(actor: &SessionActor, root: &Path, id: &str, summary: &str) {
    let handle = &actor.chat_state_handle;
    let ids = handle
        .materialize_timeline(info(root).id.to_string())
        .await
        .unwrap()
        .surface_ids;
    let target = chat_state::SurfaceRange {
        start: ids[1],
        end: ids[3],
        shadowed: ids[1..4].to_vec(),
    };
    for event in [
        CompactionEvent::Started {
            mode: chat_state::CompactionMode::Background,
            id: id.into(),
            source_items: 3,
            prompt_index: 0,
        },
        CompactionEvent::Promoted { id: id.into() },
    ] {
        handle
            .record_timeline_event_durably(TimelineEventKind::Compaction(event))
            .await
            .unwrap();
    }
    let last = handle
        .timeline_events()
        .await
        .unwrap()
        .last()
        .unwrap()
        .seq
        .get();
    let input_ref = chat_state::TimelineRangeRef {
        timeline_id: info(root).id.to_string(),
        first_seq: 0,
        last_seq: last,
    };
    let sideband_id = uuid::Uuid::now_v7().to_string();
    handle
        .record_timeline_event_durably(TimelineEventKind::Sideband(
            chat_state::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: chat_state::SidebandPurpose::CompactionSummary,
                source_refs: vec![input_ref.clone()],
            },
        ))
        .await
        .unwrap();
    let mut sideband = chat_state::SidebandTimeline::new(sideband_id.clone()).unwrap();
    for kind in [
        chat_state::SidebandEventKind::Request(chat_state::SidebandRequest {
            purpose: chat_state::SidebandPurpose::CompactionSummary,
            prompt: "summarize old range".into(),
            source_refs: vec![input_ref.clone()],
            budget_policy: chat_state::SidebandBudgetPolicy {
                max_attempts: 1,
                max_input_tokens_per_attempt: 100,
                max_output_tokens_per_attempt: Some(100),
            },
            route: chat_state::SidebandRoute {
                model: "test-model".into(),
                backend: sampling_types::ApiBackend::Messages,
            },
            initiator_ref: format!("t:{}/sideband:{sideband_id}", info(root).id),
            executor: "main".into(),
            output_schema: None,
        }),
        chat_state::SidebandEventKind::Attempt(chat_state::SidebandAttempt {
            attempt_no: 1,
            input_refs: vec![input_ref.clone()],
            assembly_manifest: chat_state::SidebandAssemblyManifest {
                strategy: "compaction-range".into(),
                strategy_version: 1,
                source_revision: Some(1),
                context_surface_ids: Vec::new(),
                selected_surface_ids: target.shadowed.clone(),
                materialized_input_tokens: 100,
                max_output_tokens: Some(100),
            },
            feedback: None,
        }),
        chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
            raw_output: summary.into(),
            structured_output: None,
            usage: chat_state::SidebandUsage::default(),
            finish: "stop".into(),
            source_event_seqs: [0, 1],
            evidence_refs: Vec::new(),
        }),
        chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
            outcome: chat_state::SidebandOutcome::Completed,
            error: None,
        }),
    ] {
        let event = sideband.prepare(kind).unwrap();
        let (respond_to, receipt) = tokio::sync::oneshot::channel();
        actor
            .notifications
            .persistence_tx
            .send(PersistenceMsg::SidebandDurablyAndAck {
                event: event.clone(),
                respond_to,
            })
            .unwrap();
        receipt.await.unwrap().unwrap();
        sideband.accept(event).unwrap();
    }
    handle
        .record_timeline_event_durably(TimelineEventKind::Compaction(CompactionEvent::Summary {
            id: id.into(),
            input_ref,
            result_ref: chat_state::TimelineRangeRef {
                timeline_id: sideband_id,
                first_seq: 2,
                last_seq: 2,
            },
            target: target.clone(),
            source_tokens: 100,
            summary_chars: summary.chars().count(),
        }))
        .await
        .unwrap();
    handle
        .replace_compaction_range(
            target,
            vec![ConversationItem::user_meta(
                ::compaction::format_compact_summary_content(summary),
            )],
        )
        .await
        .unwrap();
    actor
        .repair_missing_control_contexts_durably()
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(TimelineEventKind::Compaction(CompactionEvent::Completed {
            id: id.into(),
            source_items: 3,
            result_items: handle.get_conversation_len().await,
            duration_ms: 1,
        }))
        .await
        .unwrap();
}

async fn recover(actor: &SessionActor, root: &Path) {
    actor
        .chat_state_handle
        .recover_interrupted_durably()
        .await
        .unwrap();
    actor
        .repair_missing_control_contexts_durably()
        .await
        .unwrap();
    let materialized = actor
        .chat_state_handle
        .materialize_timeline(info(root).id.to_string())
        .await
        .unwrap();
    let events = actor.chat_state_handle.timeline_events().await.unwrap();
    publish_json(
        &root.join("recovered.json"),
        json!({
            "pid": std::process::id(), "events": events, "surface": materialized.surface,
            "ids": materialized.surface_ids, "tokens": actor.chat_state_handle.get_projected_tokens().await,
        }),
    );
}

struct ChildOwner {
    process: Child,
    log: PathBuf,
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
impl ChildOwner {
    fn spawn(root: &Path, mode: &str, cut: &str, label: &str) -> Self {
        let log = root.join(format!("{label}.log"));
        let output = std::fs::File::create(&log).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "owned test subprocess has bounded wait and kill/wait Drop"
        )]
        let process = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(MODE, mode)
            .env(CUT, cut)
            .env("GROW_HOME", root)
            .env("TMPDIR", root.join("tmp"))
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        Self { process, log }
    }
    fn wait_ready(&mut self, root: &Path) -> serde_json::Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if root.join("ready.json").exists() {
                let value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("ready.json")).unwrap())
                        .unwrap();
                assert_eq!(value["pid"], self.process.id());
                return value;
            }
            assert!(
                self.process.try_wait().unwrap().is_none(),
                "child exited before ACK cut: {}",
                std::fs::read_to_string(&self.log).unwrap()
            );
            assert!(
                std::time::Instant::now() < deadline,
                "child did not reach ACK cut: {}",
                std::fs::read_to_string(&self.log).unwrap()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    fn wait_success(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(status) = self.process.try_wait().unwrap() {
                let log = std::fs::read_to_string(&self.log).unwrap();
                assert!(
                    status.success() && log.contains("1 passed"),
                    "recovery process failed at {}: {log}",
                    self.log.display()
                );
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "recovery exceeded deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    fn kill(&mut self) {
        self.process.kill().unwrap();
        let status = self.process.wait().unwrap();
        assert!(!status.success());
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(
                status.signal(),
                Some(9),
                "the owned writer must die by SIGKILL"
            );
        }
    }
}

fn ledger(root: &Path) -> PathBuf {
    root.join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname(&info(root).cwd))
        .join(info(root).id.to_string())
        .join(crate::session::storage::TIMELINE_FILE)
}

#[test]
fn compaction_real_files_process_recovery() {
    if let Ok(mode) = std::env::var(MODE) {
        let root = PathBuf::from(std::env::var("GROW_HOME").unwrap());
        let cut = std::env::var(CUT).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            if mode == "reject" {
                let Err(error) = persistence::load_light(&info(&root), None, true).await else {
                    panic!("a modified complete journal record cannot be admitted");
                };
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                publish_json(
                    &root.join("rejected.json"),
                    json!({"error": error.to_string()}),
                );
                return;
            }
            let actor = build_actor(&root, mode != "write", &cut).await;
            if mode == "write" {
                write_until_interrupted(&actor, &root).await;
            } else {
                if mode == "retry" {
                    commit_summary(&actor, &root, "retry-compaction", RETRY_SUMMARY).await;
                }
                recover(&actor, &root).await;
            }
        }));
        return;
    }
    for cut in [
        "promoted",
        "summary",
        "replacement",
        "control",
        "completed",
        "recovery-completed",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("workspace")).unwrap();
        std::fs::create_dir(root.join("tmp")).unwrap();
        let initial_cut = if cut == "recovery-completed" {
            "replacement"
        } else {
            cut
        };
        let mut writer = ChildOwner::spawn(root, "write", initial_cut, "writer");
        let ready = writer.wait_ready(root);
        let event: chat_state::TimelineEvent =
            serde_json::from_value(ready["event"].clone()).unwrap();
        let prefix = std::fs::read(ledger(root)).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let observer =
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf());
        let stored = observer.read_timeline_events_sync(&info(root)).unwrap();
        assert_eq!(
            serde_json::to_value(stored.last().unwrap()).unwrap(),
            serde_json::to_value(&event).unwrap()
        );
        assert_eq!(
            runtime
                .block_on(observer.load_session_for_write_without_updates(&info(root)))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock,
            "live writer must exclude another incarnation at {cut}"
        );
        writer.kill();
        std::fs::remove_file(root.join("ready.json")).unwrap();
        if cut == "recovery-completed" {
            let mut interrupted =
                ChildOwner::spawn(root, "recover", "completed", "interrupted-recovery");
            interrupted.wait_ready(root);
            assert!(std::fs::read(ledger(root)).unwrap().starts_with(&prefix));
            interrupted.kill();
            std::fs::remove_file(root.join("ready.json")).unwrap();
        }
        ChildOwner::spawn(root, "recover", "none", "recovery-1").wait_success();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("recovered.json")).unwrap()).unwrap();
        assert_ne!(report["pid"], ready["pid"]);
        let first_bytes = std::fs::read(ledger(root)).unwrap();
        assert!(
            first_bytes.starts_with(&prefix),
            "recovery must preserve physical prefix: {cut}"
        );
        let events: Vec<chat_state::TimelineEvent> =
            serde_json::from_value(report["events"].clone()).unwrap();
        let count = |predicate: fn(&TimelineEventKind) -> bool| {
            events.iter().filter(|event| predicate(&event.kind)).count()
        };
        let committed = !matches!(cut, "promoted" | "summary");
        assert_eq!(
            count(
                |kind| matches!(kind, TimelineEventKind::Messages(message) if message.cause == chat_state::MessageCause::Compaction)
            ),
            usize::from(committed),
            "{cut}"
        );
        assert_eq!(
            count(|kind| matches!(
                kind,
                TimelineEventKind::Compaction(CompactionEvent::Completed { .. })
            )),
            usize::from(committed),
            "{cut}"
        );
        assert_eq!(
            count(|kind| matches!(
                kind,
                TimelineEventKind::Compaction(CompactionEvent::Failed { .. })
            )),
            usize::from(!committed),
            "{cut}"
        );
        assert_eq!(
            count(
                |kind| matches!(kind, TimelineEventKind::Control(control) if control.model_contexts.iter().any(|context| context.activation == chat_state::ControlContextActivation::Reprojection))
            ),
            usize::from(committed),
            "{cut}"
        );
        let expected: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("expected.json")).unwrap()).unwrap();
        let surface: Vec<ConversationItem> =
            serde_json::from_value(report["surface"].clone()).unwrap();
        assert_eq!(
            surface
                .iter()
                .filter(|item| item.text_content() == ROLE)
                .count(),
            1,
            "{cut}"
        );
        if committed {
            assert_eq!(
                surface
                    .iter()
                    .filter(|item| item.text_content().contains(SUMMARY))
                    .count(),
                1,
                "{cut}"
            );
            assert_eq!(
                report["surface"][2], expected["surface"][4],
                "retained prompt order/text: {cut}"
            );
            assert_eq!(
                report["surface"][3], expected["surface"][5],
                "retained answer order/text: {cut}"
            );
            for index in [0, 4, 5] {
                assert!(
                    report["ids"]
                        .as_array()
                        .unwrap()
                        .contains(&expected["ids"][index]),
                    "unselected identity at {cut}"
                );
            }
        } else {
            assert_eq!(report["surface"], expected["surface"], "{cut}");
            assert_eq!(report["ids"], expected["ids"], "{cut}");
        }
        assert!(report["tokens"].as_u64().unwrap() > 0);
        ChildOwner::spawn(root, "recover", "none", "recovery-2").wait_success();
        assert_eq!(
            std::fs::read(ledger(root)).unwrap(),
            first_bytes,
            "second recovery is byte-for-byte idempotent: {cut}"
        );
        if cut == "summary" {
            ChildOwner::spawn(root, "retry", "none", "same-range-retry").wait_success();
            let retried: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("recovered.json")).unwrap())
                    .unwrap();
            let retried_surface: Vec<ConversationItem> =
                serde_json::from_value(retried["surface"].clone()).unwrap();
            assert_eq!(
                retried_surface
                    .iter()
                    .filter(|item| item.text_content().contains(RETRY_SUMMARY))
                    .count(),
                1
            );
            let retried_events: Vec<chat_state::TimelineEvent> =
                serde_json::from_value(retried["events"].clone()).unwrap();
            assert_eq!(
                retried_events
                    .iter()
                    .filter(|event| matches!(&event.kind,
                TimelineEventKind::Compaction(CompactionEvent::Failed { id, .. }) if id == ID))
                    .count(),
                1
            );
            assert_eq!(retried_events.iter().filter(|event| matches!(&event.kind,
                TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. }) if id == "retry-compaction")).count(), 1);
            let retried_bytes = std::fs::read(ledger(root)).unwrap();
            assert!(retried_bytes.starts_with(&first_bytes));
            ChildOwner::spawn(root, "recover", "none", "after-same-range-retry").wait_success();
            assert_eq!(std::fs::read(ledger(root)).unwrap(), retried_bytes);
            println!("real file/process same-range retry passed with different summary");
            let old_sideband = events
                .iter()
                .find_map(|event| match &event.kind {
                    TimelineEventKind::Sideband(spawn) => Some(&spawn.sideband_id),
                    _ => None,
                })
                .unwrap();
            std::fs::remove_file(
                ledger(root)
                    .parent()
                    .unwrap()
                    .join(crate::session::storage::SIDEBANDS_DIR)
                    .join(old_sideband)
                    .join(crate::session::storage::TIMELINE_FILE),
            )
            .unwrap();
            ChildOwner::spawn(root, "reject", "none", "missing-failed-summary-evidence")
                .wait_success();
            assert_eq!(
                std::fs::read(ledger(root)).unwrap(),
                retried_bytes,
                "even a Failed Summary must retain its source/result proof"
            );
            println!(
                "missing failed-summary Sideband evidence rejected without rewriting parent ledger"
            );
        }
        if cut == "replacement" {
            // Change content without changing framing/JSON syntax. A fresh
            // writer must reject integrity loss, not silently replay/rewrite it.
            let mut corrupted = first_bytes.clone();
            let offset = corrupted
                .windows(SUMMARY.len())
                .position(|bytes| bytes == SUMMARY.as_bytes())
                .expect("the real journal retains the summary source/result evidence");
            corrupted[offset] = b'X';
            std::fs::write(ledger(root), &corrupted).unwrap();
            ChildOwner::spawn(root, "reject", "none", "corrupt-recovery").wait_success();
            assert!(root.join("rejected.json").exists());
            assert_eq!(
                std::fs::read(ledger(root)).unwrap(),
                corrupted,
                "rejected recovery cannot rewrite the corrupt complete record"
            );
        }
        println!(
            "real file/process boundary passed: {cut}; writer_pid={}; recovered_pid={}; durable_bytes={}; prefix_bytes={}; prefix_hash={}",
            ready["pid"],
            report["pid"],
            first_bytes.len(),
            prefix.len(),
            blake3::hash(&prefix)
        );
    }
}
