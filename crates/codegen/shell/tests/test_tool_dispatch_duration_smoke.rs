//! Smoke: dispatch wall-clock tool duration (not post-flight ~0ms).
//!
//! Scripts a model turn that calls `run_terminal_command` with `sleep 2`,
//! runs headless against the mock inference server, then asserts
//! `timeline.jsonl` has a `tool/completed` fact with multi-second `duration_ms`
//! and a non-empty `tool_call_id`.
//!
//! `#[ignore]` (needs a built binary). Run locally (auto-builds the pager):
//! ```bash
//! cargo test -p shell --test test_tool_dispatch_duration_smoke -- --ignored
//! ```

#![cfg(unix)]

use std::path::Path;
use std::time::Duration;

use test_support::sse::{
    chat_completions_reasoning_then_tool_call_events, responses_api_reasoning_then_tool_call_events,
};
use test_support::*;

const CALL_ID: &str = "call_sleep_timing";
const SLEEP_SECS: u64 = 2;

fn enqueue_sleep_tool_turn(server: &MockInferenceServer) {
    let args = serde_json::json!({
        "command": format!("sleep {SLEEP_SECS}"),
        "description": "timing smoke sleep",
    })
    .to_string();

    // Both backends: mock may pick either depending on model/settings.
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(responses_api_reasoning_then_tool_call_events(
            "running a short sleep",
            CALL_ID,
            "run_terminal_command",
            &args,
            "test-model",
        )),
    );
    server.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
            "running a short sleep",
            CALL_ID,
            "run_terminal_command",
            &args,
            "test-model",
        )),
    );
    // After tool result, model finishes with plain text.
    server.set_response("slept");
}

/// Canonical Timeline `tool/completed` for this call id.
fn find_tool_completed(timeline_jsonl: &str, call_id: &str) -> Option<serde_json::Value> {
    timeline_jsonl.lines().find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let event = v.get("event")?;
        let is_match = v.get("type").and_then(|t| t.as_str()) == Some("tool")
            && event.get("state").and_then(|state| state.as_str()) == Some("completed")
            && event.get("call_id").and_then(|id| id.as_str()) == Some(call_id);
        is_match.then_some(event.clone())
    })
}

fn collect_timeline_jsonl(root: &Path) -> Vec<(std::path::PathBuf, String)> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(std::path::PathBuf, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("timeline.jsonl")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push((path, text));
            }
        }
    }
    walk(root, &mut out);
    out
}

#[tokio::test]
#[ignore] // requires pre-built binary; run with --ignored
async fn sleep_tool_records_multi_second_dispatch_duration() {
    let server = MockInferenceServer::start()
        .await
        .expect("start mock server");
    enqueue_sleep_tool_turn(&server);

    let sandbox = TestSandbox::builder().mock_url(server.url()).git().build();

    let mut cmd = tokio::process::Command::new(grow_binary());
    cmd.args(["-p", "please sleep two seconds", "--permission-mode", "always-approve"])
        .current_dir(sandbox.workspace());

    let started = std::time::Instant::now();
    let result = run_headless_in_sandbox_borrowed(cmd, &sandbox).await;
    let wall = started.elapsed();

    eprintln!(
        "headless elapsed={wall:?} timed_out={} status={:?}\nstderr_tail:\n{}",
        result.timed_out,
        result.status.code(),
        stderr_tail(&result.stderr, 2500)
    );

    assert_headless_success(&result, "grow -p sleep timing smoke", Some(&server));
    assert_no_crashes(&result.stderr);

    // Session artifacts live under the sandbox GROW_HOME.
    let home = sandbox.grow_home();
    let timeline_files = collect_timeline_jsonl(home);
    assert!(
        !timeline_files.is_empty(),
        "no timeline.jsonl under GROW_HOME {}\nstderr:\n{}",
        home.display(),
        stderr_tail(&result.stderr, 2000)
    );

    let (path, ev) = timeline_files
        .iter()
        .find_map(|(path, text)| find_tool_completed(text, CALL_ID).map(|ev| (path, ev)))
        .unwrap_or_else(|| {
            panic!(
                "no Timeline tool/completed for {CALL_ID} under {}\nfiles: {:?}",
                home.display(),
                timeline_files
                    .iter()
                    .map(|(p, t)| (p.display().to_string(), t.lines().count()))
                    .collect::<Vec<_>>()
            )
        });

    let duration_ms = ev
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("duration_ms missing in {ev} ({})", path.display()));
    let tool_name = ev.get("name").and_then(|v| v.as_str()).unwrap_or("?");

    eprintln!(
        "tool/completed path={} tool_name={tool_name} duration_ms={duration_ms}",
        path.display()
    );

    // A post-flight clock reports single-digit ms for this call; 1.5s floor
    // absorbs CI jitter, the ceiling only catches a nonsense value.
    assert!(
        duration_ms >= 1_500,
        "expected duration_ms >= 1500 for sleep {SLEEP_SECS}s, got {duration_ms}ms \
         (if ~0–50ms, dispatch timing regressed to post-flight clock)"
    );
    assert!(
        duration_ms < 30_000,
        "duration_ms={duration_ms} implausibly high for sleep {SLEEP_SECS}"
    );

    // Sanity: the sleep really happened inside this run.
    assert!(
        wall >= Duration::from_millis(1_500),
        "headless wall {wall:?} shorter than sleep — tool may not have run"
    );
}
