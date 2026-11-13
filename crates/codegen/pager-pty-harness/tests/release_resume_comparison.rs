//! Manual PTY A/B acceptance for fixed-surface, growing-closed-history resume.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pager_pty_harness::{ContentController, PtyHarness};
use serde_json::{Value, json};
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};

const BUDGET: Duration = Duration::from_secs(120);
const WORKING_TURNS: usize = 1;
// A closed-history observation is about 16 KiB on disk: these points target
// roughly 1, 128, and 384 MiB while keeping the model-visible surface fixed.
const HISTORY_POINTS: &str = "64,8192,24576";
const SAME_HOME_COLD_RESUMES: usize = 1;

// Observe only closed synchronized-update frames, not partially received rows.
fn closed_frame(h: &PtyHarness) -> bool {
    let raw = h.raw_output();
    let begin = raw.windows(8).rposition(|w| w == b"\x1b[?2026h");
    let end = raw.windows(8).rposition(|w| w == b"\x1b[?2026l");
    matches!((begin, end), (Some(b), Some(e)) if e > b)
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

async fn measure(binary: &Path, label: &str, history_observations: usize, out: &Path) -> Value {
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
        other => panic!(
            "unsupported GROW_COMPARE_CLOSED_HISTORY_KIND={other}; expected presentation or domain-observation"
        ),
    };
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
        synth::admission::commit_fixture_through_normal_writer(
            root,
            &info,
            &source_directory,
            "mock/mock",
        )
        .await;
        let directory = synth::locate_session_dir(root, info.id.0.as_ref());
        let manifest = directory.join("working-history.json");
        assert!(manifest.is_file());
        // The baseline predates publication during writes. Give it the same
        // canonical facts with the initial publication it would have lacked.
        if label.ends_with("-baseline") {
            std::fs::remove_file(manifest).unwrap();
        }
        directory
    } else {
        // Direct fixture files model a pre-existing journal without a publication.
        let manifest = source_directory.join("working-history.json");
        if manifest.exists() {
            std::fs::remove_file(manifest).unwrap();
        }
        source_directory
    };
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

    let history_bytes: u64 = ["timeline.jsonl", "updates.jsonl"]
        .iter()
        .map(|name| std::fs::metadata(directory.join(name)).map_or(0, |metadata| metadata.len()))
        .sum();
    let started = Instant::now();
    let mut h = PtyHarness::spawn_with_content_env_in_dir(
        binary,
        50,
        120,
        &content,
        &["--continue", "--fullscreen"],
        &[("GROW_INSTRUMENTATION", "off")],
        Some(&cwd),
    )
    .unwrap();

    let latest_marker = "GROW_SEQ_000001";
    h.wait_until("latest history page marker", BUDGET, |h| {
        closed_frame(h) && h.contains_text(latest_marker)
    })
    .unwrap();
    let latest_page_ms = started.elapsed().as_secs_f64() * 1000.0;

    h.wait_until("resumed session is ready", BUDGET, |h| {
        closed_frame(h)
            && !h.screen_contents().contains("Loading session")
            && h.screen_contents().contains("grow · Mock")
    })
    .unwrap();
    let resume_ready_ms = started.elapsed().as_secs_f64() * 1000.0;

    // PTY key injection exercises the same editable prompt as a user's typing;
    // require the exact text to be echoed in a completed terminal frame.
    let typed_confirmation = "GROW_TYPED_INPUT_CONFIRMED";
    h.inject_keys(typed_confirmation.as_bytes()).unwrap();
    h.wait_until("typed input confirmation", BUDGET, |h| {
        closed_frame(h) && h.contains_text(typed_confirmation)
    })
    .unwrap();
    let input_confirmed_ms = started.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(
        pager_pty_harness::inference_request_count(&content),
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
    let eof_started = Instant::now();
    h.inject_keys(b"\x04\x04").unwrap();
    let mut exit_retries = 0;
    let _ = h.wait_until("exit confirmation or process teardown", BUDGET, |h| {
        h.contains_text("Exit not confirmed") || h.contains_text("Saving continues automatically")
    });
    while h.is_running().unwrap() && h.contains_text("Session startup is still pending") {
        assert!(
            eof_started.elapsed() < BUDGET,
            "exit retries did not settle"
        );
        h.update(Duration::from_millis(250));
        if h.is_running().unwrap() {
            h.inject_keys(b"r").unwrap();
        }
        exit_retries += 1;
        h.update(Duration::from_millis(250));
    }
    assert_eq!(
        h.wait_for_exit_and_drain(BUDGET, Duration::from_secs(1))
            .unwrap(),
        0,
        "two Ctrl+D keys must close the resumed CLI successfully"
    );
    let eof_exit_ms = eof_started.elapsed().as_secs_f64() * 1000.0;

    // The first pass may include one-time history migration. Repeat cold
    // process starts against this same home and session to expose steady resume
    // cost without rebuilding the fixture or changing its provider route.
    let mut same_home_cold_resumes = Vec::with_capacity(SAME_HOME_COLD_RESUMES);
    for repeat in 0..SAME_HOME_COLD_RESUMES {
        let cold_started = Instant::now();
        let mut cold = PtyHarness::spawn_with_content_env_in_dir(
            binary,
            50,
            120,
            &content,
            &["--continue", "--fullscreen"],
            &[("GROW_INSTRUMENTATION", "off")],
            Some(&cwd),
        )
        .unwrap();
        cold.wait_until("same-home latest history page marker", BUDGET, |h| {
            closed_frame(h) && h.contains_text(latest_marker)
        })
        .unwrap();
        let cold_latest_page_ms = cold_started.elapsed().as_secs_f64() * 1000.0;
        cold.wait_until("same-home cold resume ready", BUDGET, |h| {
            closed_frame(h)
                && !h.screen_contents().contains("Loading session")
                && h.screen_contents().contains("grow · Mock")
        })
        .unwrap();
        let cold_resume_ready_ms = cold_started.elapsed().as_secs_f64() * 1000.0;
        cold.inject_keys(typed_confirmation.as_bytes()).unwrap();
        cold.wait_until("same-home typed input confirmation", BUDGET, |h| {
            closed_frame(h) && h.contains_text(typed_confirmation)
        })
        .unwrap();
        let cold_input_confirmed_ms = cold_started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(
            pager_pty_harness::inference_request_count(&content),
            0,
            "same-home typed confirmation must not submit a model request"
        );
        cold.inject_keys(b"\x15").unwrap();
        cold.wait_until("same-home empty prompt before exit", BUDGET, |h| {
            closed_frame(h) && !h.contains_text(typed_confirmation)
        })
        .unwrap();
        let older_page_ms = if closed_history_kind == synth::ClosedHistoryKind::PresentationNotice
            && label.ends_with("-candidate")
            && history_observations > 256
        {
            cold.inject_keys(b"\t").unwrap();
            let requested = Instant::now();
            let deadline = requested + BUDGET;
            while !cold.contains_text("Older history · Pg") {
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
        let cold_eof_started = Instant::now();
        cold.inject_keys(b"\x04\x04").unwrap();
        let _ = cold.wait_until("same-home exit confirmation", BUDGET, |h| {
            h.contains_text("Exit not confirmed")
                || h.contains_text("Saving continues automatically")
        });
        while cold.is_running().unwrap() && cold.contains_text("Session startup is still pending") {
            assert!(
                cold_eof_started.elapsed() < BUDGET,
                "cold resume exit did not settle"
            );
            cold.update(Duration::from_millis(250));
            if cold.is_running().unwrap() {
                cold.inject_keys(b"r").unwrap();
            }
            cold.update(Duration::from_millis(250));
        }
        assert_eq!(
            cold.wait_for_exit_and_drain(BUDGET, Duration::from_secs(1))
                .unwrap(),
            0,
            "same-home cold resume {repeat} must close successfully"
        );
        let cold_eof_exit_ms = cold_eof_started.elapsed().as_secs_f64() * 1000.0;
        let mut phases = json!({
            "latest_page": cold_latest_page_ms,
            "resume_ready": cold_resume_ready_ms,
            "typed_input_confirmed": cold_input_confirmed_ms,
            "two_ctrl_d_eof_exit": cold_eof_exit_ms,
        });
        if let Some(older_page_ms) = older_page_ms {
            phases["older_page"] = json!(older_page_ms);
        }
        same_home_cold_resumes.push(json!({
            "trial": repeat,
            "phases_ms": phases,
        }));
        assert_eq!(
            pager_pty_harness::inference_request_count(&content),
            0,
            "same-home cold resume must not submit a model request"
        );
    }

    json!({
        "label": label,
        "history_observations": history_observations,
        "closed_history_kind": match closed_history_kind {
            synth::ClosedHistoryKind::PresentationNotice => "presentation",
            synth::ClosedHistoryKind::DomainObservation => "domain-observation",
        },
        "fixed_surface_messages": WORKING_TURNS * 2,
        "fixed_surface_blake3": surface_hash,
        "fixture_format": "timeline_v7",
        "normal_writes": normal_writes,
        "history_bytes": history_bytes,
        "phases_ms": {
            "latest_page": latest_page_ms,
            "resume_ready": resume_ready_ms,
            "typed_input_confirmed": input_confirmed_ms,
            "two_ctrl_d_eof_exit": eof_exit_ms,
        },
        "same_home_cold_resumes": same_home_cold_resumes,
        "latest_page_marker": latest_marker,
        "typed_input_confirmed": typed_confirmation,
        "exit_code": 0,
        "exit_retries": exit_retries,
        "inference_requests": pager_pty_harness::inference_request_count(&content),
    })
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
    let mut phase_samples: std::collections::BTreeMap<(usize, String, String, String), Vec<f64>> =
        std::collections::BTreeMap::new();
    let pairs = 3;
    for history_observations in points {
        for pair in 0..pairs {
            let order = if pair % 2 == 0 {
                [("baseline", &baseline), ("candidate", &candidate)]
            } else {
                [("candidate", &candidate), ("baseline", &baseline)]
            };
            for (side, binary) in order {
                let label = format!("history-{history_observations}-pair-{pair}-{side}");
                let row = measure(binary, &label, history_observations, &out).await;
                assert_eq!(row["latest_page_marker"], "GROW_SEQ_000001");
                assert_eq!(row["typed_input_confirmed"], "GROW_TYPED_INPUT_CONFIRMED");
                assert_eq!(row["exit_code"], 0);
                assert_eq!(row["inference_requests"], 0);
                let history_bytes = row["history_bytes"].as_u64().unwrap();
                assert!(history_bytes >= (history_observations as u64) * 16 * 1024);
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
        }
    }
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
