//! Smoke: dispatch wall-clock tool duration (not post-flight ~0ms).
//!
//! Scripts a model turn that calls `run_terminal_command` with `sleep 2`,
//! runs headless against the mock inference server, then asserts
//! `timeline.jsonl` has a `tool/completed` fact with multi-second `duration_ms`
//! and a non-empty `tool_call_id`.
//!
//! `#[ignore]` (needs a built binary). Run locally (auto-builds the pager):
//! ```bash
//! cargo test -p shell --features test-support --test test_tool_dispatch_duration_smoke -- --ignored
//! ```

#![cfg(unix)]

use std::path::Path;
use std::time::Duration;

use acp_transport::protocol as acp;
use shell::session::storage::load_updates_for_replay_at;
use test_support::sse::{
    chat_completions_reasoning_then_tool_call_events, responses_api_reasoning_then_tool_call_events,
};
use test_support::*;

const CALL_ID: &str = "call_sleep_timing";
const SLEEP_SECS: u64 = 2;
const DISPATCH_MARKER: &str = "TOOL_DISPATCH_ONCE_8f9204";
const OUTPUT_MARKER: &str = "TOOL_OUTPUT_HISTORY_6c32ae";

fn enqueue_sleep_tool_turn(
    server: &MockInferenceServer,
    dispatch_marker: &Path,
) -> Vec<InferenceExpectation> {
    let args = serde_json::json!({
        "command": format!(
            "sleep {SLEEP_SECS}; printf '{DISPATCH_MARKER}\\n' >> '{}'; printf '{OUTPUT_MARKER}\\n'",
            dispatch_marker.display()
        ),
        "description": "timing smoke sleep",
    })
    .to_string();

    // Match the actual foreground turn so title/classifier/other sideband
    // requests cannot consume the scripted tool response before the prompt.
    let responses = server.expect_response(
        "foreground-responses-tool-call",
        InferenceRequestMatcher::foreground(InferenceEndpoint::Responses),
        ScriptedResponse::sse(responses_api_reasoning_then_tool_call_events(
            "running a short sleep",
            CALL_ID,
            "run_terminal_command",
            &args,
            "test-model",
        )),
    );
    let chat = server.expect_response(
        "foreground-chat-tool-call",
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
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
    vec![responses, chat]
}

/// Canonical Timeline `tool/completed` for this call id.
fn find_tool_completed(timeline_jsonl: &str, call_id: &str) -> Option<serde_json::Value> {
    let timeline = shell::session::testkit::decode_timeline_fixture(timeline_jsonl).ok()?;
    timeline.events().iter().find_map(|event| {
        let chat_state::TimelineEventKind::Tool(tool) = &event.kind else {
            return None;
        };
        let is_match = matches!(
            tool,
            chat_state::ToolEvent::Completed { call_id: id, .. } if id == call_id
        );
        is_match.then(|| serde_json::to_value(tool).ok()).flatten()
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

fn is_sideband_timeline(path: &Path) -> bool {
    path.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .is_some_and(|name| name == "sidebands")
}

fn inference_request_count(server: &MockInferenceServer) -> usize {
    server
        .requests()
        .iter()
        .filter(|request| {
            matches!(
                request.path.as_str(),
                "/v1/responses" | "/v1/chat/completions" | "/v1/messages"
            )
        })
        .count()
}

fn mock_request_diagnostic(server: &MockInferenceServer) -> String {
    server
        .requests()
        .iter()
        .enumerate()
        .map(|(index, request)| {
            let body = request
                .body
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "<no body>".into());
            let body = body.chars().take(900).collect::<String>();
            format!(
                "#{index} {} {} x-grow-turn-idx={:?} x-grow-req-id={:?} body={body}",
                request.method,
                request.path,
                request.header("x-grow-turn-idx"),
                request.header("x-grow-req-id")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn timeline_diagnostic(path: &Path, contents: &str) -> String {
    let Ok(timeline) = shell::session::testkit::decode_timeline_fixture(contents) else {
        return format!("{}: <timeline decode failed>", path.display());
    };
    let events = timeline
        .events()
        .iter()
        .map(|event| {
            let payload =
                serde_json::to_string(&event.kind).unwrap_or_else(|_| "<encode error>".into());
            let payload = payload.chars().take(900).collect::<String>();
            format!("seq={} at_ms={} {}", event.seq.get(), event.at_ms, payload)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}:\n{}", path.display(), events)
}

#[tokio::test]
#[ignore] // requires pre-built binary; run with --ignored
async fn sleep_tool_records_multi_second_dispatch_duration() {
    let server = MockInferenceServer::start_with_models(vec![
        MockModelEntry::with_agent_type("test-model", "grow-build").with_api_backend("responses"),
    ])
    .await
    .expect("start mock server");

    let sandbox = TestSandbox::builder().mock_url(server.url()).git().build();
    let dispatch_marker = sandbox.workspace().join("tool-dispatch-count.txt");
    let expectations = enqueue_sleep_tool_turn(&server, &dispatch_marker);
    std::fs::write(
        sandbox.grow_home().join("config.toml"),
        format!(
            r#"[models]
default = "mock/test-model"

[provider.mock.options]
base_url = "{}"
env_key = "GROW_API_KEY"

[provider.mock.models.test-model]
model = "test-model"
api_backend = "responses"
context_window = 200000
agent_type = "grow-build"
"#,
            server.url()
        ),
    )
    .expect("write explicit mock provider config");

    let mut cmd = tokio::process::Command::new(grow_binary());
    cmd.args([
        "-p",
        "please sleep two seconds",
        "--permission-mode",
        "always-approve",
    ])
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
        .filter(|(path, _)| !is_sideband_timeline(path))
        .find_map(|(path, text)| find_tool_completed(text, CALL_ID).map(|ev| (path, ev)))
        .unwrap_or_else(|| {
            panic!(
                "no root Timeline tool/completed for {CALL_ID} under {}\nforeground scripts: {:?}\nroot timeline events:\n{}\nheadless stdout:\n{}\nheadless stderr:\n{}\nmock requests (endpoint, turn classification headers, body):\n{}\nfiles: {:?}",
                home.display(),
                expectations.iter().map(InferenceExpectation::diagnostic).collect::<Vec<_>>(),
                timeline_files
                    .iter()
                    .find(|(path, _)| !is_sideband_timeline(path))
                    .map(|(path, text)| timeline_diagnostic(path, text))
                    .unwrap_or_else(|| "<root timeline missing>".into()),
                result.stdout,
                stderr_tail(&result.stderr, 2500),
                mock_request_diagnostic(&server),
                timeline_files
                    .iter()
                    .map(|(p, t)| (p.display().to_string(), t.lines().count()))
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        expectations.iter().any(InferenceExpectation::is_satisfied),
        "a root tool completion exists but no foreground model request consumed the scripted tool response: {:?}",
        expectations
            .iter()
            .map(InferenceExpectation::diagnostic)
            .collect::<Vec<_>>()
    );

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

    let completed_count = timeline_files
        .iter()
        .filter(|(path, _)| !is_sideband_timeline(path))
        .filter(|(_, text)| find_tool_completed(text, CALL_ID).is_some())
        .count();
    assert_eq!(completed_count, 1, "one root ToolEvent completion per call");
    assert_eq!(
        std::fs::read_to_string(&dispatch_marker).expect("tool dispatch marker"),
        format!("{DISPATCH_MARKER}\n"),
        "the command must execute exactly once before history load"
    );

    // The typed history projector and a real, fresh ACP subprocess load must
    // agree on the tool call/result and final assistant text without causing
    // another command execution or inference request.
    let session_directory = path.parent().expect("root session directory");
    let session_id = session_directory
        .file_name()
        .and_then(|name| name.to_str())
        .expect("session ID directory");
    let projected = load_updates_for_replay_at(session_id, home)
        .expect("validate and project cold Timeline")
        .expect("session exists");
    let tool_call_indexes = projected
        .iter()
        .enumerate()
        .filter_map(|(index, update)| match update {
            acp::SessionUpdate::ToolCall(call) if call.tool_call_id.to_string() == CALL_ID => {
                Some((index, call))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_call_indexes.len(),
        1,
        "replay must preserve one tool call"
    );
    let (tool_call_index, tool_call) = tool_call_indexes[0];
    assert!(
        serde_json::to_string(&tool_call.raw_input)
            .unwrap()
            .contains(DISPATCH_MARKER),
        "replayed tool call input must retain the exact command"
    );
    let completed_updates = projected
        .iter()
        .enumerate()
        .filter_map(|(index, update)| match update {
            acp::SessionUpdate::ToolCallUpdate(update)
                if update.tool_call_id.to_string() == CALL_ID
                    && update.fields.status == Some(acp::ToolCallStatus::Completed) =>
            {
                Some((index, update))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        completed_updates.len(),
        1,
        "replay must preserve one completed tool update"
    );
    let (tool_update_index, completed_update) = completed_updates[0];
    assert!(
        tool_update_index > tool_call_index,
        "tool completion follows its call"
    );
    assert!(
        serde_json::to_string(&completed_update.fields.raw_output)
            .unwrap()
            .contains(OUTPUT_MARKER),
        "replayed tool update must retain rawOutput"
    );
    let final_answer_index = projected
        .iter()
        .position(|update| matches!(update, acp::SessionUpdate::AgentMessageChunk(chunk)
            if matches!(&chunk.content, acp::ContentBlock::Text(text) if text.text.contains("slept"))))
        .expect("final assistant answer is part of cold replay");
    assert!(
        final_answer_index > tool_update_index,
        "final assistant text follows the completed tool update"
    );
    let expected_agent_text = projected
        .iter()
        .filter_map(|update| match update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .concat();

    let timeline_text = std::fs::read_to_string(path).expect("read root Timeline");
    let timeline = shell::session::testkit::decode_timeline_fixture(&timeline_text)
        .expect("decode root Timeline");
    let completed_events = timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed {
                call_id,
                outcome,
                ..
            }) if call_id == CALL_ID => Some((event.seq, outcome)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        completed_events.len(),
        1,
        "one physical ToolEvent completion"
    );
    // Generic tool completion records lifecycle, while its UI rawOutput and
    // model ToolResult own their respective bodies. Do not invent another
    // body requirement for this metadata-only source.
    assert_eq!(completed_events[0].1, "success");
    let surface = serde_json::to_value(timeline.surface()).expect("serialize domain history");
    let items = surface.as_array().expect("conversation surface array");
    let assistant_call_index = items
        .iter()
        .position(|item| {
            item["type"] == "assistant"
                && item["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| calls.iter().any(|call| call["id"] == CALL_ID))
        })
        .expect("domain assistant tool call");
    let tool_result_index = items
        .iter()
        .position(|item| {
            item["type"] == "tool_result"
                && item["tool_call_id"] == CALL_ID
                && item["content"]
                    .as_str()
                    .is_some_and(|s| s.contains(OUTPUT_MARKER))
        })
        .expect("model conversation ToolResult body");
    let assistant_final_index = items
        .iter()
        .rposition(|item| item["type"] == "assistant" && item["content"].as_str() == Some("slept"))
        .expect("model final answer");
    assert!(
        assistant_call_index < tool_result_index && tool_result_index < assistant_final_index,
        "domain tool call, ToolResult and final answer retain causal order"
    );

    // Headless startup uses the OS current directory (canonical /private/var
    // on macOS); ACP load must address the exact persisted cwd namespace.
    let summary: shell::session::persistence::Summary = serde_json::from_slice(
        &std::fs::read(session_directory.join("summary.json")).expect("read root summary"),
    )
    .expect("decode root summary");
    let workspace = std::path::PathBuf::from(summary.info.cwd);
    assert_eq!(
        std::fs::canonicalize(&workspace).unwrap(),
        std::fs::canonicalize(sandbox.workspace()).unwrap(),
        "the saved cwd must name the same isolated test workspace"
    );
    tokio::task::LocalSet::new()
        .run_until(async move {
            let mut client =
                GrowStdioClient::spawn_with_sandbox(&server, &workspace, sandbox).await;
            client.initialize().await;
            let inference_count_before_load = inference_request_count(&server);
            client
                .load_session_with_timeout(&acp::SessionId::new(session_id), &workspace)
                .await;
            assert!(
                client.captured_text().contains("slept"),
                "cold ACP load must deliver the final historical assistant text"
            );
            assert_eq!(
                client.captured_text(),
                expected_agent_text,
                "cold ACP load must deliver the same ordered assistant text as the validated Timeline projector"
            );
            assert_eq!(
                std::fs::read_to_string(&dispatch_marker).expect("dispatch marker after load"),
                format!("{DISPATCH_MARKER}\n"),
                "cold load must not execute the tool again"
            );
            assert_eq!(
                inference_request_count(&server),
                inference_count_before_load,
                "cold load must not call the model"
            );
        })
        .await;
}
