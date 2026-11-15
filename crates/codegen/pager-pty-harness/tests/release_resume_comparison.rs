//! Manual PTY A/B acceptance for fixed-surface, growing-closed-history resume.
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pager_pty_harness::{ContentController, PtyExitPoll, PtyHarness};
use serde_json::{Value, json};
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};

const BUDGET: Duration = Duration::from_secs(120);
const WORKING_TURNS: usize = 1;
// A closed-history observation is about 16 KiB on disk: these points target
// roughly 1, 128, and 490 MiB while keeping the model-visible surface fixed.
const HISTORY_POINTS: &str = "64,8192,30000";
const SAME_HOME_COLD_RESUMES: usize = 1;
const UNPUBLISHED_TAIL_RECORDS: usize = 2;

// Observe only closed synchronized-update frames, not partially received rows.
fn closed_frame(h: &PtyHarness) -> bool {
    let raw = h.raw_output();
    let begin = raw.windows(8).rposition(|w| w == b"\x1b[?2026h");
    let end = raw.windows(8).rposition(|w| w == b"\x1b[?2026l");
    matches!((begin, end), (Some(b), Some(e)) if e > b)
}

struct ResumeMilestones {
    first_frame: f64,
    latest_page: f64,
    ready: f64,
    input: f64,
}

fn observe_resume_milestones(
    harness: &mut PtyHarness,
    started: Instant,
    latest_marker: &str,
    typed_confirmation: &str,
) -> ResumeMilestones {
    let deadline = started + BUDGET;
    let mut first_frame_ms = None;
    let mut latest_page_ms = None;
    let mut resume_ready_ms = None;
    let mut input_confirmed_ms = None;
    while latest_page_ms.is_none() || input_confirmed_ms.is_none() {
        assert!(
            Instant::now() < deadline,
            "resume milestone observation timed out (recent={latest_page_ms:?}, ready={resume_ready_ms:?}, input={input_confirmed_ms:?})\n{}",
            harness.screen_contents()
        );
        harness.update(Duration::from_millis(10));
        if !closed_frame(harness) {
            continue;
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        first_frame_ms.get_or_insert(elapsed);
        let screen = harness.screen_contents();
        if latest_page_ms.is_none() && screen.contains(latest_marker) {
            latest_page_ms = Some(elapsed);
        }
        if resume_ready_ms.is_none()
            && !screen.contains("Loading session")
            && screen.contains("grow · Mock")
        {
            resume_ready_ms = Some(elapsed);
            harness.inject_keys(typed_confirmation.as_bytes()).unwrap();
        }
        if resume_ready_ms.is_some()
            && input_confirmed_ms.is_none()
            && screen.contains(typed_confirmation)
        {
            input_confirmed_ms = Some(elapsed);
        }
    }
    ResumeMilestones {
        first_frame: first_frame_ms.unwrap(),
        latest_page: latest_page_ms.unwrap(),
        ready: resume_ready_ms.unwrap(),
        input: input_confirmed_ms.unwrap(),
    }
}

fn confirm_exit(harness: &mut PtyHarness) -> (f64, usize) {
    let started = Instant::now();
    harness.inject_keys(b"\x04\x04").unwrap();
    let mut retries = 0;
    let mut waiting_to_retry = false;
    loop {
        assert!(started.elapsed() < BUDGET, "Ctrl+D did not settle");
        if let PtyExitPoll::Exited(code) = harness.wait_exit_code(Duration::ZERO).unwrap() {
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(code, 0, "confirmed Ctrl+D must exit successfully");
            harness
                .wait_for_exit_and_drain(Duration::from_secs(1), Duration::from_secs(1))
                .unwrap();
            return (elapsed, retries);
        }
        harness.update(Duration::from_millis(10));
        let pending = harness.contains_text("Session startup is still pending");
        if pending && !waiting_to_retry && harness.is_running().unwrap() {
            harness.inject_keys(b"r").unwrap();
            retries += 1;
        }
        waiting_to_retry = pending;
    }
}

fn foreground_request_count(content: &ContentController) -> usize {
    content
        .requests()
        .iter()
        .filter(|request| {
            request.header("x-grow-turn-idx").is_some()
                || (request.header("x-grow-req-id").is_none()
                    && request
                        .body
                        .as_ref()
                        .and_then(|body| body.get("tools"))
                        .and_then(Value::as_array)
                        .is_some_and(|tools| tools.len() >= 2))
        })
        .count()
}

fn fixed_surface_spec() -> SessionSpec {
    SessionSpec {
        // This is the only model-visible user/assistant pair at every size.
        turns: WORKING_TURNS,
        // Closed-history records are added separately as committed Grow
        // presentation facts, so command regeneration stays out of the fixture.
        acu_per_turn: 0,
        catalog_commands: 16,
        catalog_desc_len: 96,
        agent_chunks_per_turn: 1,
        agent_chunk_len: 96,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    }
}

struct UnpublishedTail {
    manifest_version: u64,
    bytes: u64,
    records: usize,
    digest: String,
    normalized_signature: String,
}

fn unpublished_tail(directory: &Path) -> UnpublishedTail {
    let manifest_file =
        std::fs::read(directory.join("working-history.json")).expect("read publication manifest");
    let (digest, payload) = manifest_file.split_at(32);
    assert_eq!(
        digest,
        blake3::hash(payload).as_bytes(),
        "manifest checksum matches"
    );
    let manifest: Value = serde_json::from_slice(payload).expect("parse publication manifest");
    let published_bytes = manifest["bytes"]
        .as_u64()
        .expect("publication manifest carries its journal frontier");
    let manifest_version = manifest["version"]
        .as_u64()
        .expect("publication manifest carries its format version");
    let path = directory.join("timeline.jsonl");
    let mut journal = std::fs::File::open(&path).expect("open fixture journal");
    let journal_bytes = journal.metadata().expect("stat fixture journal").len();
    assert!(
        journal_bytes > published_bytes,
        "fixture must contain a nonempty unpublished journal tail"
    );
    journal
        .seek(SeekFrom::Start(published_bytes))
        .expect("seek to published journal frontier");
    let mut tail = Vec::new();
    journal
        .read_to_end(&mut tail)
        .expect("read unpublished tail");
    assert_eq!(tail.last(), Some(&b'\n'), "tail contains complete records");
    let lines: Vec<_> = tail
        .strip_suffix(b"\n")
        .unwrap()
        .split(|b| *b == b'\n')
        .collect();
    assert_eq!(lines.len(), UNPUBLISHED_TAIL_RECORDS);

    let mut normalized = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let mut record: Value = serde_json::from_slice(line).expect("parse unpublished record");
        assert_eq!(record["fact"]["kind"], "domain");
        let timeline_event = record["fact"]["event"]
            .as_object_mut()
            .expect("domain record contains its Timeline event");
        let expected = if index == 0 { "step" } else { "turn" };
        assert_eq!(timeline_event["type"], expected);
        let event = timeline_event["event"]
            .as_object_mut()
            .expect("Timeline event kind contains its payload");
        assert_eq!(event["state"], "ended");
        timeline_event.insert("seq".into(), Value::from(0));
        record["seq"] = Value::from(0);
        record["checksum"] = Value::from("normalized");
        normalized.extend_from_slice(
            &serde_json::to_vec(&record).expect("serialize normalized tail record"),
        );
        normalized.push(b'\n');
    }
    UnpublishedTail {
        manifest_version,
        bytes: tail.len() as u64,
        records: UNPUBLISHED_TAIL_RECORDS,
        digest: blake3::hash(&tail).to_hex().to_string(),
        normalized_signature: blake3::hash(&normalized).to_hex().to_string(),
    }
}

fn warm_executable(binary: &Path) {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .expect("launch benchmark executable warmup");
    assert!(
        output.status.success(),
        "benchmark executable warmup failed for {}: {}",
        binary.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct PreparedFixture {
    content: ContentController,
    _project: tempfile::TempDir,
    cwd: PathBuf,
    surface_hash: String,
    history_bytes: u64,
    unpublished_tail: UnpublishedTail,
    closed_history_kind: synth::ClosedHistoryKind,
    normal_writes: bool,
    baseline_manifest_removed: bool,
}

async fn prepare_measure_fixture(label: &str, history_observations: usize) -> PreparedFixture {
    let content = ContentController::start().await.unwrap();
    content.seed_llm_config().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    let cwd = dunce::canonicalize(project.path()).unwrap();
    let root = content.sandbox().grow_home();
    let spec = fixed_surface_spec();
    let normal_writes = std::env::var("GROW_COMPARE_NORMAL_WRITES").as_deref() == Ok("1");
    let donor = normal_writes.then(|| tempfile::tempdir().unwrap());
    let fixture_root = donor.as_ref().map_or(root, |dir| dir.path());
    let (info, source_directory) =
        synth::replay::prepare_legacy_session(fixture_root, &cwd, &spec).await;
    let closed_history_kind = std::env::var("GROW_COMPARE_CLOSED_HISTORY_KIND")
        .unwrap_or_else(|_| "presentation".to_owned());
    let closed_history_kind = match closed_history_kind.as_str() {
        "presentation" => synth::ClosedHistoryKind::PresentationNotice,
        "domain-observation" => synth::ClosedHistoryKind::DomainObservation,
        "tool-lifecycle" => synth::ClosedHistoryKind::ToolLifecycle,
        other => panic!(
            "unsupported GROW_COMPARE_CLOSED_HISTORY_KIND={other}; expected presentation, domain-observation, or tool-lifecycle"
        ),
    };
    assert!(
        closed_history_kind != synth::ClosedHistoryKind::ToolLifecycle || normal_writes,
        "tool-lifecycle fixture requires GROW_COMPARE_NORMAL_WRITES=1"
    );
    let surface_hash = synth::write_admitted_history_with_closed_observations_kind(
        &info,
        &source_directory,
        &spec,
        "mock/mock",
        0,
        history_observations,
        closed_history_kind,
        None,
    );
    let directory = if normal_writes {
        if let Some(writer) = label
            .ends_with("-baseline")
            .then(|| std::env::var_os("GROW_COMPARE_BASELINE_FIXTURE_WRITER"))
            .flatten()
        {
            let status = std::process::Command::new(writer)
                .args(["--exact", "commit_comparison_fixture", "--ignored"])
                .env("GROW_COMPARE_WRITER_ROOT", root)
                .env("GROW_COMPARE_WRITER_SOURCE", &source_directory)
                .env("GROW_COMPARE_WRITER_CWD", &cwd)
                .env("GROW_COMPARE_WRITER_SESSION", info.id.0.as_ref())
                .status()
                .unwrap();
            assert!(status.success(), "baseline fixture writer failed");
        } else {
            synth::admission::commit_fixture_through_normal_writer_with_unpublished_tail(
                root,
                &info,
                &source_directory,
                "mock/mock",
                UNPUBLISHED_TAIL_RECORDS,
            )
            .await;
        }
        let directory = synth::locate_session_dir(root, info.id.0.as_ref());
        let manifest = directory.join("working-history.json");
        assert!(manifest.is_file());
        if label.ends_with("-baseline")
            && closed_history_kind != synth::ClosedHistoryKind::ToolLifecycle
        {
            std::fs::remove_file(manifest).unwrap();
        }
        directory
    } else {
        let manifest = source_directory.join("working-history.json");
        if manifest.exists() {
            std::fs::remove_file(manifest).unwrap();
        }
        source_directory
    };
    let baseline_manifest_removed = normal_writes
        && label.ends_with("-baseline")
        && closed_history_kind != synth::ClosedHistoryKind::ToolLifecycle;
    drop(donor);
    JsonlStorageAdapter::with_root(root.to_path_buf())
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("mock/mock"),
            None,
            None,
        )
        .await
        .unwrap();
    let unpublished_tail = unpublished_tail(&directory);
    assert_eq!(unpublished_tail.records, UNPUBLISHED_TAIL_RECORDS);
    let history_bytes: u64 = ["timeline.jsonl", "updates.jsonl"]
        .iter()
        .map(|name| std::fs::metadata(directory.join(name)).map_or(0, |metadata| metadata.len()))
        .sum();

    PreparedFixture {
        content,
        _project: project,
        cwd,
        surface_hash,
        history_bytes,
        unpublished_tail,
        closed_history_kind,
        normal_writes,
        baseline_manifest_removed,
    }
}

async fn measure(
    binary: &Path,
    label: &str,
    history_observations: usize,
    out: &Path,
    fixture: PreparedFixture,
) -> Value {
    let PreparedFixture {
        content,
        _project,
        cwd,
        surface_hash,
        history_bytes,
        unpublished_tail,
        closed_history_kind,
        normal_writes,
        baseline_manifest_removed,
    } = fixture;
    let stage_logs = std::env::var("GROW_COMPARE_STAGE_LOGS").as_deref() == Ok("1");
    let first_stage_log = out.join(format!("{label}-first-open.instrumentation.jsonl"));
    let first_stage_log = first_stage_log.to_string_lossy().into_owned();
    let mut first_child_env = vec![("GROW_INSTRUMENTATION", "off")];
    if stage_logs {
        std::fs::write(&first_stage_log, b"").unwrap();
        first_child_env = vec![
            ("GROW_INSTRUMENTATION", "log"),
            ("GROW_INSTRUMENTATION_LOG", first_stage_log.as_str()),
        ];
    }
    let started = Instant::now();
    let mut h = PtyHarness::spawn_with_content_env_in_dir(
        binary,
        50,
        120,
        &content,
        &["--continue", "--fullscreen"],
        &first_child_env,
        Some(&cwd),
    )
    .unwrap();
    h.set_respond_to_queries(true);

    let latest_marker = "GROW_SEQ_000001";
    // PTY key injection exercises the same editable prompt as a user's typing;
    // require the exact text to be echoed in a completed terminal frame.
    let typed_confirmation = "GROW_TYPED_INPUT_CONFIRMED";
    let resume = observe_resume_milestones(&mut h, started, latest_marker, typed_confirmation);

    assert_eq!(
        foreground_request_count(&content),
        0,
        "typing the confirmation must not submit a model request"
    );
    h.write_cast(&out.join(format!("{label}.cast"))).unwrap();
    std::fs::write(out.join(format!("{label}.txt")), h.screen_contents()).unwrap();

    // Clear the typed confirmation, then send the terminal's two-step EOF.
    h.inject_keys(b"\x15").unwrap();
    h.wait_until("empty prompt before exit", BUDGET, |h| {
        closed_frame(h) && !h.contains_text(typed_confirmation)
    })
    .unwrap();
    let mut first_operation =
        content.expect_agent_turn("first accepted operation", "GROW_FIRST_OPERATION_ACCEPTED");
    let operation_started = Instant::now();
    h.inject_keys(b"GROW_FIRST_OPERATION_REQUEST\r").unwrap();
    h.wait_until("first accepted operation", BUDGET, |h| {
        closed_frame(h) && h.contains_text("GROW_FIRST_OPERATION_ACCEPTED")
    })
    .unwrap();
    tokio::time::timeout(BUDGET, first_operation.wait_satisfied())
        .await
        .expect("first operation response reached its terminal boundary");
    let first_operation_ms = operation_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(foreground_request_count(&content), 1);
    h.wait_for_turn_idle(BUDGET).unwrap();
    let (eof_exit_ms, exit_retries) = confirm_exit(&mut h);

    // The first pass may include one-time history migration. Repeat cold
    // process starts against this same home and session to expose steady resume
    // cost without rebuilding the fixture or changing its provider route.
    let mut same_home_cold_resumes = Vec::with_capacity(SAME_HOME_COLD_RESUMES);
    for repeat in 0..SAME_HOME_COLD_RESUMES {
        let repeat_stage_log = out.join(format!("{label}-repeat-{repeat}.instrumentation.jsonl"));
        let repeat_stage_log = repeat_stage_log.to_string_lossy().into_owned();
        let mut repeat_child_env = vec![("GROW_INSTRUMENTATION", "off")];
        if stage_logs {
            std::fs::write(&repeat_stage_log, b"").unwrap();
            repeat_child_env = vec![
                ("GROW_INSTRUMENTATION", "log"),
                ("GROW_INSTRUMENTATION_LOG", repeat_stage_log.as_str()),
            ];
        }
        let cold_started = Instant::now();
        let mut cold = PtyHarness::spawn_with_content_env_in_dir(
            binary,
            50,
            120,
            &content,
            &["--continue", "--fullscreen"],
            &repeat_child_env,
            Some(&cwd),
        )
        .unwrap();
        cold.set_respond_to_queries(true);
        let cold_resume =
            observe_resume_milestones(&mut cold, cold_started, latest_marker, typed_confirmation);
        assert_eq!(
            foreground_request_count(&content),
            repeat + 1,
            "same-home typed confirmation must not submit another foreground request"
        );
        cold.inject_keys(b"\x15").unwrap();
        cold.wait_until("same-home empty prompt before exit", BUDGET, |h| {
            closed_frame(h) && !h.contains_text(typed_confirmation)
        })
        .unwrap();
        let older_page_ms = if closed_history_kind == synth::ClosedHistoryKind::ToolLifecycle
            || (closed_history_kind == synth::ClosedHistoryKind::PresentationNotice
                && label.ends_with("-candidate")
                && history_observations > 256)
        {
            cold.inject_keys(b"\t").unwrap();
            let requested = Instant::now();
            let deadline = requested + BUDGET;
            while !(cold.contains_text("Older history · Pg")
                || cold.contains_text("Older history · start reached"))
            {
                assert!(
                    Instant::now() < deadline,
                    "older page did not appear: {}",
                    cold.screen_contents()
                );
                cold.inject_keys(b"\x1b[5~").unwrap();
                cold.update(Duration::from_millis(30));
            }
            let elapsed = requested.elapsed().as_secs_f64() * 1000.0;
            cold.inject_keys(b"\x1b").unwrap();
            cold.wait_until("close older history view", BUDGET, |h| {
                !h.contains_text("Older history")
            })
            .unwrap();
            cold.inject_keys(b"\t").unwrap();
            Some(elapsed)
        } else {
            None
        };
        let mut first_operation = content.expect_agent_turn(
            format!("same-home accepted operation {repeat}"),
            "GROW_REPEAT_OPERATION_ACCEPTED",
        );
        let operation_started = Instant::now();
        cold.inject_keys(b"GROW_REPEAT_OPERATION_REQUEST\r")
            .unwrap();
        cold.wait_until("same-home first accepted operation", BUDGET, |h| {
            closed_frame(h) && h.contains_text("GROW_REPEAT_OPERATION_ACCEPTED")
        })
        .unwrap();
        tokio::time::timeout(BUDGET, first_operation.wait_satisfied())
            .await
            .expect("same-home operation response reached its terminal boundary");
        let cold_first_operation_ms = operation_started.elapsed().as_secs_f64() * 1000.0;
        assert!(
            foreground_request_count(&content) >= repeat + 2,
            "the terminal reached a foreground response for each explicit operation"
        );
        cold.wait_for_turn_idle(BUDGET).unwrap();
        let (cold_eof_exit_ms, _) = confirm_exit(&mut cold);
        let mut phases = json!({
            "first_frame": cold_resume.first_frame,
            "latest_page": cold_resume.latest_page,
            "resume_ready": cold_resume.ready,
            "typed_input_confirmed": cold_resume.input,
            "first_accepted_operation_roundtrip_after_resume_ready": cold_first_operation_ms,
            "two_ctrl_d_eof_exit": cold_eof_exit_ms,
        });
        if let Some(older_page_ms) = older_page_ms {
            phases["older_page"] = json!(older_page_ms);
        }
        same_home_cold_resumes.push(json!({
            "trial": repeat,
            "phases_ms": phases,
        }));
    }

    json!({
        "label": label,
        "history_observations": history_observations,
        "closed_tool_lifecycle_pairs": if closed_history_kind
            == synth::ClosedHistoryKind::ToolLifecycle
        {
            history_observations
        } else {
            0
        },
        "closed_history_kind": match closed_history_kind {
            synth::ClosedHistoryKind::PresentationNotice => "presentation",
            synth::ClosedHistoryKind::DomainObservation => "domain-observation",
            synth::ClosedHistoryKind::ToolLifecycle => "tool-lifecycle",
        },
        "fixed_surface_messages": WORKING_TURNS * 2,
        "fixed_surface_blake3": surface_hash,
        "fixture_format": "timeline_v7",
        "normal_writes": normal_writes,
        "fixture_writer": if normal_writes && label.ends_with("-baseline")
            && std::env::var_os("GROW_COMPARE_BASELINE_FIXTURE_WRITER").is_some()
        {
            "baseline-subprocess-shell-storage-adapter"
        } else if normal_writes {
            "test-process-shell-storage-adapter"
        } else {
            "direct-fixture-writer"
        },
        "baseline_manifest_removed": baseline_manifest_removed,
        "unpublished_tail_bytes": unpublished_tail.bytes,
        "unpublished_tail_records": unpublished_tail.records,
        "unpublished_tail_blake3": unpublished_tail.digest,
        "unpublished_tail_signature": unpublished_tail.normalized_signature,
        "publication_manifest_version": unpublished_tail.manifest_version,
        "unpublished_tail_durable": true,
        "history_bytes": history_bytes,
        "stage_logs": stage_logs,
        "phases_ms": {
            "first_frame": resume.first_frame,
            "latest_page": resume.latest_page,
            "resume_ready": resume.ready,
            "typed_input_confirmed": resume.input,
            "first_accepted_operation_roundtrip_after_latest_page": first_operation_ms,
            "two_ctrl_d_eof_exit": eof_exit_ms,
        },
        "same_home_cold_resumes": same_home_cold_resumes,
        "latest_page_marker": latest_marker,
        "typed_input_confirmed": typed_confirmation,
        "exit_code": 0,
        "exit_retries": exit_retries,
        "inference_requests": foreground_request_count(&content),
    })
}

async fn measure_cold_exit(
    binary: &Path,
    label: &str,
    history_observations: usize,
    out: &Path,
    fixture: PreparedFixture,
) -> Value {
    let PreparedFixture {
        content,
        _project,
        cwd,
        surface_hash,
        history_bytes,
        unpublished_tail: tail,
        ..
    } = fixture;
    assert!(history_bytes >= (history_observations as u64) * 16 * 1024);
    if history_observations == 30_000 {
        assert!(history_bytes > 515_000_000);
    }

    let stage_log = out.join(format!("{label}.instrumentation.jsonl"));
    let stage_log = stage_log.to_string_lossy().into_owned();
    let child_env = if std::env::var("GROW_COMPARE_STAGE_LOGS").as_deref() == Ok("1") {
        std::fs::write(&stage_log, b"").unwrap();
        vec![
            ("GROW_INSTRUMENTATION", "log"),
            ("GROW_INSTRUMENTATION_LOG", stage_log.as_str()),
        ]
    } else {
        vec![("GROW_INSTRUMENTATION", "off")]
    };
    let started = Instant::now();
    let mut h = PtyHarness::spawn_with_content_env_in_dir(
        binary,
        50,
        120,
        &content,
        &["--continue", "--fullscreen"],
        &child_env,
        Some(&cwd),
    )
    .unwrap();
    h.set_respond_to_queries(true);
    h.wait_until("first cold-exit frame", BUDGET, closed_frame)
        .unwrap();
    let first_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
    let latest_marker = "GROW_SEQ_000001";
    h.wait_until("cold-exit latest page", BUDGET, |h| {
        closed_frame(h) && h.contains_text(latest_marker)
    })
    .unwrap();
    let latest_page_ms = started.elapsed().as_secs_f64() * 1000.0;
    h.wait_until("cold-exit session ready", BUDGET, |h| {
        closed_frame(h)
            && !h.screen_contents().contains("Loading session")
            && h.screen_contents().contains("grow · Mock")
    })
    .unwrap();
    let ready_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        foreground_request_count(&content),
        0,
        "cold exit must not submit a prompt or operation"
    );
    let screen_before_exit = h.screen_contents();
    let (exit_from_ready_ms, exit_retries) = confirm_exit(&mut h);
    h.write_cast(&out.join(format!("{label}.cast"))).unwrap();
    std::fs::write(out.join(format!("{label}.txt")), screen_before_exit).unwrap();

    json!({
        "label": label,
        "history_observations": history_observations,
        "closed_tool_lifecycle_pairs": history_observations,
        "fixed_surface_messages": WORKING_TURNS * 2,
        "fixed_surface_blake3": surface_hash,
        "fixture_writer": if label.ends_with("-baseline") {
            "baseline-subprocess-shell-storage-adapter"
        } else {
            "test-process-shell-storage-adapter"
        },
        "baseline_manifest_removed": false,
        "history_bytes": history_bytes,
        "unpublished_tail_bytes": tail.bytes,
        "unpublished_tail_records": tail.records,
        "unpublished_tail_blake3": tail.digest,
        "unpublished_tail_signature": tail.normalized_signature,
        "publication_manifest_version": tail.manifest_version,
        "unpublished_tail_durable": true,
        "phases_ms": {
            "first_frame": first_frame_ms,
            "latest_page": latest_page_ms,
            "resume_ready_after_latest_page": ready_ms,
            "ctrl_d_from_resume_ready_exit": exit_from_ready_ms,
        },
        "exit_retries": exit_retries,
        "exit_code": 0,
        "inference_requests": foreground_request_count(&content),
        "latest_page_marker": latest_marker,
    })
}

/// Run the identical writer driver linked against the baseline checkout.
#[tokio::test(flavor = "current_thread")]
#[ignore = "subprocess driver for a pinned A/B fixture writer"]
async fn commit_comparison_fixture() {
    let root = PathBuf::from(std::env::var_os("GROW_COMPARE_WRITER_ROOT").unwrap());
    let source = PathBuf::from(std::env::var_os("GROW_COMPARE_WRITER_SOURCE").unwrap());
    let info = shell::session::info::Info {
        id: synth::sid(&std::env::var("GROW_COMPARE_WRITER_SESSION").unwrap()),
        cwd: std::env::var("GROW_COMPARE_WRITER_CWD").unwrap(),
    };
    synth::admission::commit_fixture_through_normal_writer_with_unpublished_tail(
        &root,
        &info,
        &source,
        "mock/mock",
        UNPUBLISHED_TAIL_RECORDS,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn normal_writer_publishes_padding_before_fixed_terminal_tail() {
    let source_root = tempfile::tempdir().unwrap();
    let target_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let (info, source_directory) = synth::replay::prepare_legacy_session(
        source_root.path(),
        cwd.path(),
        &fixed_surface_spec(),
    )
    .await;
    synth::write_admitted_history_with_closed_observations_kind(
        &info,
        &source_directory,
        &fixed_surface_spec(),
        "mock/mock",
        0,
        1,
        synth::ClosedHistoryKind::ToolLifecycle,
        None,
    );

    synth::admission::commit_fixture_through_normal_writer_with_unpublished_tail(
        target_root.path(),
        &info,
        &source_directory,
        "mock/mock",
        UNPUBLISHED_TAIL_RECORDS,
    )
    .await;

    let directory = synth::locate_session_dir(target_root.path(), info.id.0.as_ref());
    let tail = unpublished_tail(&directory);
    assert_eq!(tail.records, UNPUBLISHED_TAIL_RECORDS);
    let manifest_file = std::fs::read(directory.join("working-history.json")).unwrap();
    let manifest: Value = serde_json::from_slice(&manifest_file[32..]).unwrap();
    let published_bytes = manifest["bytes"].as_u64().unwrap() as usize;
    let journal = std::fs::read(directory.join("timeline.jsonl")).unwrap();
    assert!(
        journal[..published_bytes]
            .windows(b"unpublished-terminal-tail".len())
            .any(|window| window == b"unpublished-terminal-tail")
    );
    assert!(journal.len() > published_bytes);
    assert_eq!(
        tail.bytes as usize,
        journal.len() - published_bytes,
        "only the fixed Step and Turn terminal records follow the published frontier"
    );
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual explicit-binary PTY acceptance; run with GROW_COMPARE_ACCEPT_SIZE_CURVE=1"]
async fn fixed_surface_resume_size_curve_acceptance() {
    use std::io::Write;
    assert_eq!(
        std::env::var("GROW_COMPARE_ACCEPT_SIZE_CURVE").as_deref(),
        Ok("1"),
        "set GROW_COMPARE_ACCEPT_SIZE_CURVE=1 to enable the complete size-curve acceptance"
    );
    assert_eq!(
        std::env::var("GROW_COMPARE_NORMAL_WRITES").as_deref(),
        Ok("1"),
        "the resume comparison requires production-written fixtures"
    );
    assert_eq!(
        std::env::var("GROW_COMPARE_CLOSED_HISTORY_KIND").as_deref(),
        Ok("tool-lifecycle"),
        "the resume comparison varies closed Tool lifecycle facts"
    );
    assert!(
        std::env::var_os("GROW_COMPARE_BASELINE_FIXTURE_WRITER").is_some(),
        "the baseline fixture must be written by its pinned Shell adapter"
    );
    let baseline = PathBuf::from(std::env::var_os("GROW_COMPARE_BASELINE_BINARY").unwrap());
    let candidate = PathBuf::from(std::env::var_os("GROW_COMPARE_CANDIDATE_BINARY").unwrap());
    let out = PathBuf::from(std::env::var_os("GROW_COMPARE_OUTPUT").unwrap());
    assert_ne!(
        baseline.canonicalize().unwrap(),
        candidate.canonicalize().unwrap(),
        "baseline and candidate must be distinct binaries"
    );
    std::fs::create_dir_all(&out).unwrap();
    let points: Vec<usize> = std::env::var("GROW_COMPARE_HISTORY_POINTS")
        .unwrap_or_else(|_| HISTORY_POINTS.to_owned())
        .split(',')
        .map(|s| {
            s.parse()
                .expect("comma-separated positive observation counts")
        })
        .collect();
    assert!(
        points.len() >= 3,
        "acceptance requires at least three history sizes"
    );
    assert!(points.iter().all(|point| *point > 0));
    assert!(
        points.windows(2).all(|pair| pair[0] < pair[1]),
        "history sizes must increase"
    );

    let mut report = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("measurements.jsonl"))
        .unwrap();
    let mut fixed_surface: Option<Value> = None;
    let mut history_bytes_by_size = std::collections::BTreeMap::new();
    let mut tail_digests_by_size: std::collections::BTreeMap<usize, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut expected_tail_signature: Option<String> = None;
    let mut phase_samples: std::collections::BTreeMap<(usize, String, String, String), Vec<f64>> =
        std::collections::BTreeMap::new();
    let pairs = 3;
    let mut trials = Vec::with_capacity(2 * pairs * points.len());
    for pair in 0..pairs {
        for offset in 0..points.len() {
            let observations = points[(offset + pair) % points.len()];
            let order = if pair % 2 == 0 {
                [("baseline", &baseline), ("candidate", &candidate)]
            } else {
                [("candidate", &candidate), ("baseline", &baseline)]
            };
            for (side, binary) in order {
                let label = format!("history-{observations}-pair-{pair}-{side}");
                let fixture = prepare_measure_fixture(&label, observations).await;
                trials.push((pair, offset, observations, side, binary, label, fixture));
            }
        }
    }
    // Keep the write workload for the large fixtures outside every timed
    // launch, and warm both executables after fixture preparation.
    for binary in [&baseline, &candidate] {
        warm_executable(binary);
    }
    for (pair, size_order_index, history_observations, side, binary, label, fixture) in trials {
        let mut row = measure(binary, &label, history_observations, &out, fixture).await;
        row["pair"] = json!(pair);
        row["size_order_index"] = json!(size_order_index);
        assert_eq!(row["latest_page_marker"], "GROW_SEQ_000001");
        assert_eq!(row["typed_input_confirmed"], "GROW_TYPED_INPUT_CONFIRMED");
        assert_eq!(row["exit_code"], 0);
        assert!(row["inference_requests"].as_u64().unwrap() >= (SAME_HOME_COLD_RESUMES + 1) as u64);
        assert_eq!(
            row["fixture_writer"],
            if side == "baseline" {
                "baseline-subprocess-shell-storage-adapter"
            } else {
                "test-process-shell-storage-adapter"
            }
        );
        let history_bytes = row["history_bytes"].as_u64().unwrap();
        assert!(history_bytes >= (history_observations as u64) * 16 * 1024);
        assert_eq!(row["baseline_manifest_removed"], false);
        assert_eq!(row["unpublished_tail_records"], UNPUBLISHED_TAIL_RECORDS);
        assert!(row["publication_manifest_version"].as_u64().is_some());
        assert_eq!(row["unpublished_tail_durable"], true);
        assert!(row["unpublished_tail_bytes"].as_u64().unwrap() > 0);
        let tail_signature = row["unpublished_tail_signature"]
            .as_str()
            .expect("normalized unpublished tail signature")
            .to_owned();
        if let Some(expected) = &expected_tail_signature {
            assert_eq!(
                expected, &tail_signature,
                "all sizes and arms must carry the same logical unpublished tail"
            );
        } else {
            expected_tail_signature = Some(tail_signature);
        }
        tail_digests_by_size
            .entry(history_observations)
            .or_default()
            .push(row["unpublished_tail_blake3"].as_str().unwrap().to_owned());
        history_bytes_by_size
            .entry(history_observations)
            .and_modify(|sizes: &mut Vec<u64>| sizes.push(history_bytes))
            .or_insert_with(|| vec![history_bytes]);
        let surface = row["fixed_surface_blake3"].clone();
        if let Some(expected) = &fixed_surface {
            assert_eq!(
                expected, &surface,
                "model-visible Surface changed across history sizes or A/B runs"
            );
        } else {
            fixed_surface = Some(surface);
        }
        for (phase, value) in row["phases_ms"].as_object().unwrap() {
            phase_samples
                .entry((
                    history_observations,
                    side.to_owned(),
                    "first_open".to_owned(),
                    phase.clone(),
                ))
                .or_default()
                .push(value.as_f64().unwrap());
        }
        for repeat in row["same_home_cold_resumes"].as_array().unwrap() {
            for (phase, value) in repeat["phases_ms"].as_object().unwrap() {
                phase_samples
                    .entry((
                        history_observations,
                        side.to_owned(),
                        "repeat_open".to_owned(),
                        phase.clone(),
                    ))
                    .or_default()
                    .push(value.as_f64().unwrap());
            }
        }
        writeln!(report, "{row}").unwrap();
        report.flush().unwrap();
        eprintln!("[fixed-surface-pty-ab] {row}");
    }
    assert!(
        tail_digests_by_size
            .values()
            .all(|digests| { digests.windows(2).all(|pair| pair[0] == pair[1]) }),
        "both arms and all pairs must carry byte-identical tails at each size"
    );
    let size_curve: Vec<_> = history_bytes_by_size
        .iter()
        .map(|(observations, sizes)| (*observations, *sizes.iter().min().unwrap()))
        .collect();
    assert!(
        size_curve.windows(2).all(|pair| pair[0].1 < pair[1].1),
        "committed history bytes must grow at every size-curve point"
    );
    assert!(
        size_curve
            .last()
            .is_some_and(|(_, bytes)| *bytes >= 350 * 1024 * 1024),
        "final size-curve point must contain at least 350 MiB of committed history"
    );
    assert!(
        size_curve
            .last()
            .is_some_and(|(_, bytes)| *bytes > 515_000_000),
        "final tool-lifecycle point must contain more than 515,000,000 bytes of committed history"
    );

    let summaries: Vec<_> = phase_samples
        .into_iter()
        .map(|((history_observations, side, pass, phase), mut samples)| {
            let median_ms = median(&mut samples);
            json!({
                "history_observations": history_observations,
                "binary": side,
                "pass": pass,
                "phase": phase,
                "samples": samples.len(),
                "median_ms": median_ms,
                "samples_ms": samples,
            })
        })
        .collect();
    std::fs::write(
        out.join("phase-summary.json"),
        serde_json::to_vec_pretty(&summaries).unwrap(),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual paired idle Ctrl+D PTY control; run with GROW_COMPARE_COLD_EXIT=1"]
async fn fixed_surface_resume_cold_exit_acceptance() {
    use std::io::Write;

    assert_eq!(
        std::env::var("GROW_COMPARE_COLD_EXIT").as_deref(),
        Ok("1"),
        "set GROW_COMPARE_COLD_EXIT=1 to enable the supplemental idle Ctrl+D control"
    );
    assert_eq!(
        std::env::var("GROW_COMPARE_NORMAL_WRITES").as_deref(),
        Ok("1")
    );
    assert_eq!(
        std::env::var("GROW_COMPARE_CLOSED_HISTORY_KIND").as_deref(),
        Ok("tool-lifecycle")
    );
    let baseline = PathBuf::from(std::env::var_os("GROW_COMPARE_BASELINE_BINARY").unwrap());
    let candidate = PathBuf::from(std::env::var_os("GROW_COMPARE_CANDIDATE_BINARY").unwrap());
    let out = PathBuf::from(std::env::var_os("GROW_COMPARE_OUTPUT").unwrap());
    assert_ne!(
        baseline.canonicalize().unwrap(),
        candidate.canonicalize().unwrap()
    );
    assert!(std::env::var_os("GROW_COMPARE_BASELINE_FIXTURE_WRITER").is_some());
    std::fs::create_dir_all(&out).unwrap();
    let points: Vec<usize> = std::env::var("GROW_COMPARE_HISTORY_POINTS")
        .unwrap_or_else(|_| HISTORY_POINTS.to_owned())
        .split(',')
        .map(|point| {
            point
                .parse()
                .expect("comma-separated positive observation counts")
        })
        .collect();
    assert_eq!(
        points.len(),
        3,
        "idle Ctrl+D acceptance requires the same three history sizes"
    );
    assert!(points.iter().all(|point| *point > 0));
    assert!(points.windows(2).all(|pair| pair[0] < pair[1]));

    let mut report = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("cold-exit-measurements.jsonl"))
        .unwrap();
    let mut fixed_surface: Option<Value> = None;
    let mut tail_signature: Option<String> = None;
    let mut tails_by_size: std::collections::BTreeMap<usize, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut sizes_by_point: std::collections::BTreeMap<usize, Vec<u64>> =
        std::collections::BTreeMap::new();
    let mut phase_samples: std::collections::BTreeMap<(usize, String), Vec<f64>> =
        std::collections::BTreeMap::new();

    // Rotate size order and arm order on each trial so neither is consistently first.
    let mut trials = Vec::with_capacity(6 * points.len());
    for trial in 0..3 {
        for offset in 0..points.len() {
            let size_order_index = (offset + trial) % points.len();
            let history_observations = points[size_order_index];
            let arms = if (trial + offset) % 2 == 0 {
                [("baseline", &baseline), ("candidate", &candidate)]
            } else {
                [("candidate", &candidate), ("baseline", &baseline)]
            };
            for (side, binary) in arms {
                let label =
                    format!("cold-exit-history-{history_observations}-trial-{trial}-{side}");
                let fixture = prepare_measure_fixture(&label, history_observations).await;
                trials.push((
                    trial,
                    size_order_index,
                    history_observations,
                    side,
                    binary,
                    label,
                    fixture,
                ));
            }
        }
    }
    for binary in [&baseline, &candidate] {
        warm_executable(binary);
    }
    for (trial, size_order_index, history_observations, side, binary, label, fixture) in trials {
        let mut row = measure_cold_exit(binary, &label, history_observations, &out, fixture).await;
        assert_eq!(
            row["inference_requests"], 0,
            "idle exit must not submit a prompt"
        );
        assert_eq!(row["exit_code"], 0);
        assert_eq!(row["unpublished_tail_records"], UNPUBLISHED_TAIL_RECORDS);
        assert_eq!(row["baseline_manifest_removed"], false);
        assert_eq!(row["latest_page_marker"], "GROW_SEQ_000001");
        assert!(row["unpublished_tail_bytes"].as_u64().unwrap() > 0);
        if let Some(expected) = &fixed_surface {
            assert_eq!(
                expected, &row["fixed_surface_blake3"],
                "cold-exit Surface changed"
            );
        } else {
            fixed_surface = Some(row["fixed_surface_blake3"].clone());
        }
        let signature = row["unpublished_tail_signature"]
            .as_str()
            .unwrap()
            .to_owned();
        if let Some(expected) = &tail_signature {
            assert_eq!(expected, &signature, "normalized terminal tail changed");
        } else {
            tail_signature = Some(signature);
        }
        tails_by_size
            .entry(history_observations)
            .or_default()
            .push(row["unpublished_tail_blake3"].as_str().unwrap().to_owned());
        sizes_by_point
            .entry(history_observations)
            .or_default()
            .push(row["history_bytes"].as_u64().unwrap());
        for phase in ["ctrl_d_from_resume_ready_exit"] {
            phase_samples
                .entry((history_observations, side.to_owned()))
                .or_default()
                .push(row["phases_ms"][phase].as_f64().unwrap());
        }
        row["trial"] = json!(trial);
        row["size_order_index"] = json!(size_order_index);
        row["binary"] = json!(side);
        writeln!(report, "{row}").unwrap();
        report.flush().unwrap();
        eprintln!("[fixed-surface-idle-ctrl-d-pty] {row}");
    }
    for (size, digests) in tails_by_size {
        assert_eq!(digests.len(), 6, "three paired trials required at {size}");
        assert!(
            digests.windows(2).all(|pair| pair[0] == pair[1]),
            "A/B tails must match at {size}"
        );
    }
    for (size, bytes) in sizes_by_point {
        assert_eq!(bytes.len(), 6);
        assert!(
            bytes.iter().max().unwrap() - bytes.iter().min().unwrap() <= 1,
            "only the one-byte fixture metadata variation is expected at {size}"
        );
    }
    let summaries: Vec<_> = phase_samples
        .into_iter()
        .map(|((size, binary), mut values)| {
            values.sort_by(f64::total_cmp);
            json!({
                "history_observations": size,
                "binary": binary,
                "phase": "ctrl_d_from_resume_ready_exit",
                "trials": values.len(),
                "median_ms": median(&mut values),
                "min_ms": values[0],
                "max_ms": values[values.len() - 1],
                "samples_ms": values,
            })
        })
        .collect();
    std::fs::write(
        out.join("cold-exit-phase-summary.json"),
        serde_json::to_vec_pretty(&summaries).unwrap(),
    )
    .unwrap();
}
