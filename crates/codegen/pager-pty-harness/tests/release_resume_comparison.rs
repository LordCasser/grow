//! Manual, external-observation comparison of two official release binaries.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pager_pty_harness::{ContentController, PtyHarness, inference_request_count};
use serde_json::{Value, json};
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};

const BUDGET: Duration = Duration::from_secs(120);

// Observe only closed synchronized-update frames, not partially received rows.
fn closed_frame(h: &PtyHarness) -> bool {
    let raw = h.raw_output();
    let begin = raw.windows(8).rposition(|w| w == b"\x1b[?2026h");
    let end = raw.windows(8).rposition(|w| w == b"\x1b[?2026l");
    matches!((begin, end), (Some(b), Some(e)) if e > b)
}

// Full body/order oracle runs only in excluded warmups, never in timed samples.
async fn wire_oracle(
    binary: &Path,
    cwd: &Path,
    content: &ContentController,
    sid: &str,
    expected: &str,
    count: usize,
) {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut cmd = tokio::process::Command::new(binary);
    content.sandbox().apply_to_tokio_command(&mut cmd);
    cmd.args(["agent", "--no-leader", "stdio"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let scope = tty_utils::ProcessScope::new();
    scope.prepare(&mut cmd);
    #[allow(
        clippy::disallowed_methods,
        reason = "immediately enrolled in the owned process scope"
    )]
    let mut child = cmd.spawn().unwrap();
    let group = scope.enroll(&child).unwrap();
    // Drop the scope before its owning group on unwind, so it can reap the tree.
    let _scope = scope;
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut bodies = Vec::new();
    let calls = [
        (
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":{},"_meta":{"clientType":"test-client","startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}}),
        ),
        (
            "authenticate",
            json!({"methodId":"provider.api_key","_meta":{"headless":true}}),
        ),
        (
            "session/load",
            json!({"sessionId":sid,"cwd":cwd,"mcpServers":[]}),
        ),
    ];
    tokio::time::timeout(BUDGET, async {
        for (index, (method, params)) in calls.into_iter().enumerate() {
            let id = index + 1;
            input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).as_bytes()).await.unwrap();
            input.flush().await.unwrap();
            loop {
                let line = output.next_line().await.unwrap().expect("oracle stdout closed");
                let value: Value = serde_json::from_str(&line).unwrap();
                let messages = value.as_array().cloned().unwrap_or_else(|| vec![value]);
                let mut done = false;
                for v in messages {
                    if v["method"] == "session/update" {
                        let u = &v["params"]["update"];
                        if let Some(kind) = u["sessionUpdate"].as_str()
                            && matches!(kind, "user_message_chunk" | "agent_message_chunk") {
                            bodies.push((kind.to_owned(), u["content"]["text"].as_str().unwrap().to_owned()));
                        }
                    } else if v.get("method").is_some() && v.get("id").is_some() {
                        input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":v["id"],"error":{"code":-32601,"message":"unsupported benchmark reverse request"}})).as_bytes()).await.unwrap();
                        input.flush().await.unwrap();
                    } else if v["id"] == id {
                        assert!(v.get("error").is_none(), "oracle RPC failure: {v}");
                        done = true;
                    }
                }
                if done { break; }
            }
        }
    }).await.expect("wire oracle timeout");
    drop(input);
    assert!(
        tokio::time::timeout(Duration::from_secs(20), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    drop(group);
    assert_eq!(bodies.len(), count);
    assert_eq!(
        blake3::hash(&serde_json::to_vec(&bodies).unwrap())
            .to_hex()
            .as_str(),
        expected
    );
}

async fn measure(binary: &Path, legacy: bool, turns: usize, label: &str, out: &Path) -> Value {
    let content = ContentController::start().await.unwrap();
    content.seed_llm_config().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    let cwd = dunce::canonicalize(project.path()).unwrap();
    let root = content.sandbox().grow_home();
    let spec = SessionSpec {
        turns,
        acu_per_turn: 8,
        catalog_commands: 16,
        catalog_desc_len: 96,
        agent_chunks_per_turn: 8,
        agent_chunk_len: 4096,
        rewind_points: 2,
        files_per_rewind: 2,
        file_content_len: 256,
    };
    let (info, directory) = synth::replay::prepare_legacy_session(root, &cwd, &spec).await;
    let route = sampling_types::model_image_input_key_from_parts(
        "mock",
        &sampling_types::ApiBackend::ChatCompletions,
        &content.url(),
        &Default::default(),
    );
    synth::write_admitted_history(
        &info,
        &directory,
        &spec,
        "mock/mock",
        0,
        Some(("mock", &route)),
    );
    JsonlStorageAdapter::with_root(root.to_path_buf())
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("mock/mock"),
            None,
            None,
        )
        .await
        .unwrap();
    let updates = shell::session::storage::load_updates_for_replay_at(info.id.0.as_ref(), root)
        .unwrap()
        .unwrap();
    let messages: Vec<_> = updates
        .iter()
        .filter_map(|u| {
            let v = serde_json::to_value(u).unwrap();
            let kind = v["sessionUpdate"].as_str()?;
            matches!(kind, "user_message_chunk" | "agent_message_chunk").then(|| {
                (
                    kind.to_owned(),
                    v["content"]["text"].as_str().unwrap().to_owned(),
                )
            })
        })
        .collect();
    assert_eq!(messages.len(), turns * 2);
    let logical_count = messages.len();
    let logical_bytes: usize = messages.iter().map(|(_, text)| text.len()).sum();
    let logical_hash = blake3::hash(&serde_json::to_vec(&messages).unwrap())
        .to_hex()
        .to_string();
    drop(messages);
    drop(updates);
    if legacy {
        shell::session::testkit::legacy_comparison::export_legacy_comparison_fixture(
            &info, &directory,
        )
        .unwrap();
    }
    if label.contains("pair0-") {
        wire_oracle(
            binary,
            &cwd,
            &content,
            &info.id.0,
            &logical_hash,
            logical_count,
        )
        .await;
    }
    let ledger_bytes: u64 = ["timeline.jsonl", "updates.jsonl"]
        .iter()
        .map(|name| std::fs::metadata(directory.join(name)).map_or(0, |m| m.len()))
        .sum();
    let start = Instant::now();
    let mut h = PtyHarness::spawn_with_content_env_in_dir(
        binary,
        50,
        120,
        &content,
        &["--continue"],
        &[("GROW_INSTRUMENTATION", "off")],
        Some(&cwd),
    )
    .unwrap();
    let last_marker = format!("GROW_SEQ_{turns:06}");
    let mut first_ms = None;
    let mut last_ms = None;
    h.wait_until("complete history and non-loading prompt", BUDGET, |h| {
        if !closed_frame(h) {
            return false;
        }
        let screen = h.screen_contents();
        let now = start.elapsed().as_secs_f64() * 1000.0;
        if screen.contains("user prompt for turn")
            || screen.contains("alpha bravo charlie")
            || screen.contains("GROW_SEQ_")
        {
            first_ms.get_or_insert(now);
        }
        if screen.contains(&last_marker) {
            last_ms.get_or_insert(now);
        }
        last_ms.is_some() && !screen.contains("Loading session") && screen.contains("grow · Mock")
    })
    .unwrap();
    let ready_view_ms = start.elapsed().as_secs_f64() * 1000.0;
    let probe = "GROW_INPUT_ECHO_PROBE";
    h.inject_keys(probe.as_bytes()).unwrap();
    h.wait_until("input echo in completed frame", BUDGET, |h| {
        closed_frame(h) && h.contains_text(probe)
    })
    .unwrap();
    let input_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        inference_request_count(&content),
        0,
        "resume/probe must not sample"
    );
    h.write_cast(&out.join(format!("{label}.cast"))).unwrap();
    std::fs::write(out.join(format!("{label}.txt")), h.screen_contents()).unwrap();
    h.inject_keys(b"\x15\x04\x04").unwrap();
    assert_eq!(
        h.wait_for_exit_and_drain(Duration::from_secs(20), Duration::from_secs(1))
            .unwrap(),
        0
    );
    json!({"label":label,"version":if legacy {"2.4.3"} else {"2.5.0"},"turns":turns,
        "first_history_ms":first_ms.unwrap(),"last_history_ms":last_ms.unwrap(),
        "ready_view_ms":ready_view_ms,"input_echo_ms":input_ms,"logical_messages":logical_count,
        "logical_text_bytes":logical_bytes,"logical_blake3":logical_hash,"ledger_bytes":ledger_bytes,
        "inference_requests":inference_request_count(&content)})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual official-release comparison; requires two verified binaries"]
async fn official_release_resume_comparison() {
    use std::io::Write;
    let old = PathBuf::from(std::env::var_os("GROW_COMPARE_OLD_BINARY").unwrap());
    let new = PathBuf::from(std::env::var_os("GROW_COMPARE_NEW_BINARY").unwrap());
    let out = PathBuf::from(std::env::var_os("GROW_COMPARE_OUTPUT").unwrap());
    let mut report = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("measurements.jsonl"))
        .unwrap();
    let mut logical = std::collections::BTreeMap::new();
    let smoke = std::env::var_os("GROW_COMPARE_SMOKE").is_some();
    let scales = if smoke {
        vec![512]
    } else {
        std::env::var("GROW_COMPARE_TURNS")
            .unwrap_or_else(|_| "512,2048".to_owned())
            .split(',')
            .map(|s| {
                s.parse::<usize>()
                    .expect("comma-separated positive turn counts")
            })
            .collect()
    };
    let selected_pair = std::env::var("GROW_COMPARE_PAIR")
        .ok()
        .map(|s| s.parse::<usize>().expect("pair index 0 through 5"));
    assert!(selected_pair.is_none_or(|pair| pair < 6));
    for turns in scales {
        for pair in 0..if smoke { 1 } else { 6 } {
            if selected_pair.is_some_and(|selected| selected != pair) {
                continue;
            }
            let versions = if pair % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            };
            for legacy in versions {
                let version = if legacy { "old" } else { "new" };
                let label = format!("{turns}-pair{pair}-{version}");
                let mut row = measure(
                    if legacy { &old } else { &new },
                    legacy,
                    turns,
                    &label,
                    &out,
                )
                .await;
                let identity = (
                    row["logical_messages"].clone(),
                    row["logical_text_bytes"].clone(),
                    row["logical_blake3"].clone(),
                );
                assert_eq!(logical.entry(turns).or_insert(identity.clone()), &identity);
                row["warmup"] = json!(pair == 0);
                writeln!(report, "{row}").unwrap();
                report.flush().unwrap();
                eprintln!("[release-comparison] {row}");
            }
        }
    }
}
