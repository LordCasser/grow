//! Opt-in process-isolated measurement of the storage phase of concurrent resume_from.
mod indexed;
use super::*;
use crate::session::storage::JsonlStorageAdapter;
use chat_state::{SubagentEvent, Timeline, TimelineEventKind};
use std::process::Command;

const PARENT: &str = "parent-session";
const CWD: &str = "/workspace";
const WIDTH: &str = "GROW_RESUME_BENCH_WIDTH";

fn directory(home: &Path, id: &str) -> PathBuf {
    home.join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname(CWD))
        .join(id)
}

fn store(home: &Path, id: &str, timeline: &Timeline, parent: Option<&str>) {
    let path = directory(home, id);
    std::fs::create_dir_all(&path).unwrap();
    let info = crate::session::info::Info {
        id: acp::SessionId::new(id.to_owned()),
        cwd: CWD.into(),
    };
    let mut summary = crate::session::persistence::Summary::new(
        &info,
        crate::session::persistence::default_model_id(),
    )
    .unwrap();
    if let Some(parent) = parent {
        summary.session_kind = Some("subagent".into());
        summary.parent_session_id = Some(parent.into());
    }
    std::fs::write(
        path.join("summary.json"),
        crate::session::storage::serialize_summary(&summary).unwrap(),
    )
    .unwrap();
    std::fs::write(
        path.join("timeline.jsonl"),
        JsonlStorageAdapter::encode_journal(timeline.events(), &[]).unwrap(),
    )
    .unwrap();
}

fn create_sources(home: &Path, extra_parent_blocks: usize) {
    let mut parent = Timeline::default();
    for i in 0..4 {
        let id = format!("source-{i}");
        let spawn = recovery_spawn(&id, &id);
        let spawn_seq = parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
                Box::new(spawn.clone()),
            )))
            .unwrap()
            .seq
            .get();
        let mut child = Timeline::default();
        child
            .record(TimelineEventKind::SubagentSeed(
                chat_state::SubagentSeedEvent {
                    parent_timeline_id: PARENT.into(),
                    parent_spawn_seq: spawn_seq,
                    subagent_id: id.clone(),
                    security_parent_session_id: spawn.security_parent_session_id,
                    context_source: spawn.context_source,
                    source_ref: spawn.source_ref,
                    normalized: spawn.context_normalized,
                },
            ))
            .unwrap();
        let result = SubagentResult {
            success: true,
            subagent_id: id.clone(),
            child_session_id: id.clone(),
            ..Default::default()
        };
        let result_seq = child
            .record(TimelineEventKind::SubagentResult(subagent_result_fact(
                &result, None,
            )))
            .unwrap()
            .seq
            .get();
        parent
            .record(TimelineEventKind::Subagent(SubagentEvent::Ended(
                chat_state::SubagentTerminalEvent {
                    subagent_id: id.clone(),
                    child_session_id: id.clone(),
                    outcome: chat_state::SubagentOutcome::Completed,
                    duration_ms: 0,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: None,
                    result_ref: Some(chat_state::TimelineRangeRef {
                        timeline_id: id.clone(),
                        first_seq: result_seq,
                        last_seq: result_seq,
                    }),
                    snapshot_ref: None,
                },
            )))
            .unwrap();
        store(home, &id, &child, Some(PARENT));
    }
    // Only the parent changes. All four resume source journals and result links stay identical.
    for _ in 0..extra_parent_blocks {
        parent
            .record(TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "benchmark".into(),
                    name: "parent history".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::Value::String("x".repeat(256 * 1024))),
                },
            ))
            .unwrap();
    }
    store(home, PARENT, &parent, None);
}

#[cfg(target_os = "macos")]
fn physical_read_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v2>::uninit();
    // libproc writes the complete V2 structure on success.
    unsafe {
        assert_eq!(
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V2,
                usage.as_mut_ptr().cast()
            ),
            0
        );
        usage.assume_init().ri_diskio_bytesread
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "isolated memory and I/O measurement; run with --ignored --nocapture"]
async fn measure_concurrent_resume_source_reads() {
    if let Some(width) = std::env::var_os(WIDTH) {
        let width: usize = width.to_str().unwrap().parse().unwrap();
        let home = PathBuf::from(std::env::var_os("GROW_HOME").unwrap());
        let requests = (0..width).map(|i| {
            durable_resume_source_for(
                home.clone(),
                format!("source-{i}"),
                PARENT.into(),
                PARENT.into(),
                None,
            )
        });
        let before = crate::session::signals::sample_rss_bytes();
        #[cfg(target_os = "macos")]
        let disk = physical_read_bytes();
        let start = std::time::Instant::now();
        let sources = futures_util::future::join_all(requests).await;
        let elapsed = start.elapsed().as_millis();
        let peak = crate::session::signals::sample_rss_bytes();
        #[cfg(target_os = "macos")]
        println!("physical_read_bytes={}", physical_read_bytes() - disk);
        for (i, source) in sources.into_iter().enumerate() {
            assert_eq!(source.unwrap().data.child_session_id, format!("source-{i}"));
        }
        println!(
            "width={width} elapsed_ms={elapsed} peak_rss_bytes={peak} baseline_hwm_bytes={before}"
        );
        return;
    }
    let test_name = concat!(module_path!(), "::measure_concurrent_resume_source_reads")
        .strip_prefix("shell::")
        .unwrap();
    for blocks in [0, 128] {
        let home = tempfile::tempdir().unwrap();
        create_sources(home.path(), blocks);
        let parent_path = directory(home.path(), PARENT).join("timeline.jsonl");
        println!(
            "fixture_parent_bytes={}",
            parent_path.metadata().unwrap().len()
        );
        for width in [1, 2, 4] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", test_name, "--ignored", "--nocapture"])
                .env("GROW_HOME", home.path())
                .env(WIDTH, width.to_string());
            #[cfg(target_os = "macos")]
            if let Some(tracer) = std::env::var_os("GROW_RESUME_BENCH_READ_TRACER") {
                command
                    .env("DYLD_INSERT_LIBRARIES", tracer)
                    .env("GROW_RESUME_BENCH_PARENT_JOURNAL", &parent_path);
            }
            let output = command.output().unwrap();
            print!("{}", String::from_utf8_lossy(&output.stdout));
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }
}
