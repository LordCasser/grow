//! Plan approval and retained-plan review after a process restart.
//!
//! Both cases create the Plan through the normal model/tool path. One resumes
//! its live approval waiter; the other defers, resumes, and explicitly reviews
//! the retained artifact.

use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::wait_for_welcome;
use crate::{
    AgentTurnExpectation, ContentController, PtyHarness, ScriptedResponse, SseEvent, pager_binary,
};

const DEFAULT_ROWS: u16 = 50;
const DEFAULT_COLS: u16 = 120;
const WELCOME_TIMEOUT: Duration = Duration::from_secs(20);
const SETUP_SENTINEL: &str = "GBT3703SETUP";
const IMPLEMENT_SENTINEL: &str = "GBT3703IMPLEMENTED";
const COMPLETION_REPORT: &str = "GBT3703REPORT: implemented and verified all three approved steps.";
const SUMMARY_SENTINEL: &str = "GBT3703SUMMARY";
const PLAN_BODY: &str = "\
# Plan GBT3703Repro

## Steps
1. Exercise the real plan approval flow
2. Defer, restart, and review the retained plan
3. Implement the approved plan
";

/// Regression: a pending approval waiter is restored after a process restart.
pub async fn assert_plan_approval_restored_after_resume() -> Result<()> {
    assert_plan_resume(false).await
}

/// Regression: a deferred Plan remains reviewable after a process restart.
pub async fn assert_deferred_plan_reviewed_after_resume() -> Result<()> {
    assert_plan_resume(true).await
}

/// Regression: two consecutive revision rounds carry their feedback into the
/// next model request, then approval runs through durable completion and a
/// normal user-facing summary.
pub async fn assert_plan_two_revisions_complete() -> Result<()> {
    let content = ContentController::start()
        .await
        .context("start ContentController")?;
    content.seed_llm_config().context("seed mock LLM config")?;

    let mut setup_turn = content.expect_agent_turn(
        "initial ordinary turn",
        format!("{SETUP_SENTINEL}: ready to draft a plan."),
    );
    let plan_v1_body = plan_body("v1");
    let mut plan_v1 = expect_plan_submit_with_body(&content, "call_plan_v1", &plan_v1_body);
    let inline_comment = "GBT3703INLINE: keep the public interface stable";
    let feedback_1 = "GBT3703FEEDBACK1: add recovery verification";
    let plan_v2_body = plan_body("v2");
    let mut plan_v2 = expect_plan_submit_with_body(&content, "call_plan_v2", &plan_v2_body);
    let feedback_2 = "GBT3703FEEDBACK2: include recovery verification";
    let plan_v3_body = plan_body("v3");
    let mut plan_v3 = expect_plan_submit_with_body(&content, "call_plan_v3", &plan_v3_body);
    let mut complete = expect_plan_tool_call(
        &content,
        "call_plan_complete",
        serde_json::json!({ "action": "complete", "report": COMPLETION_REPORT }).to_string(),
    );
    let mut summary = content.expect_agent_turn(
        "normal summary after Plan completion",
        format!("{SUMMARY_SENTINEL}: all approved work is complete."),
    );

    let project = tempfile::tempdir().context("project dir")?;
    std::fs::create_dir_all(project.path().join(".git")).context("create .git")?;
    let binary = pager_binary().context("resolve pager binary")?;
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        Some(project.path()),
    )
    .context("spawn pager")?;

    wait_for_welcome(&mut harness).await?;
    harness
        .inject_keys(b"go\r")
        .context("submit ordinary turn")?;
    harness
        .wait_for_text(SETUP_SENTINEL, Duration::from_secs(30))
        .context("ordinary turn rendered")?;
    tokio::time::timeout(Duration::from_secs(10), setup_turn.wait_satisfied())
        .await
        .context("ordinary turn expectation timeout")?;

    harness
        .inject_keys(b"/plan present the plan\r")
        .context("submit initial Plan prompt")?;
    wait_for_plan_candidate(&mut harness, "GBT3703PLANv1", &mut plan_v1).await?;

    harness
        .wait_for_text("c comment", Duration::from_secs(10))
        .context("line comment action is available")?;
    harness
        .inject_keys(b"c")
        .context("enter first inline comment")?;
    harness
        .wait_for_text("Enter:save comment", Duration::from_secs(10))
        .context("commenting focus is active")?;
    harness
        .inject_keys(format!("{inline_comment}\r").as_bytes())
        .context("save first inline comment")?;
    harness
        .wait_for_text("c comment (1)", Duration::from_secs(10))
        .context("inline comment is attached to the Plan")?;

    harness
        .inject_keys(b"s")
        .context("focus first revision feedback")?;
    harness
        .wait_for_text("Tab:plan", Duration::from_secs(10))
        .context("revision prompt focus is active")?;
    harness
        .inject_keys(format!("{feedback_1}\r").as_bytes())
        .context("submit first revision feedback")?;
    wait_for_feedback_request(&content, &[inline_comment, feedback_1], &mut plan_v2).await?;
    wait_for_plan_candidate(&mut harness, "GBT3703PLANv2", &mut plan_v2).await?;

    harness
        .inject_keys(b"s")
        .context("focus second revision feedback")?;
    harness
        .wait_for_text("Tab:plan", Duration::from_secs(10))
        .context("second revision prompt focus is active")?;
    harness
        .inject_keys(format!("{feedback_2}\r").as_bytes())
        .context("submit second revision feedback")?;
    wait_for_feedback_request(&content, &[feedback_2], &mut plan_v3).await?;
    wait_for_plan_candidate(&mut harness, "GBT3703PLANv3", &mut plan_v3).await?;

    harness
        .inject_keys(b"a")
        .context("approve third Plan submission")?;
    tokio::time::timeout(Duration::from_secs(60), complete.wait_received())
        .await
        .context("approved Plan did not request plan_control complete")?;
    tokio::time::timeout(Duration::from_secs(10), complete.wait_satisfied())
        .await
        .context("plan_control complete request timeout")?;
    harness
        .wait_for_text("[Plan: Complete]", Duration::from_secs(15))
        .context("completed Plan status rendered")?;
    harness.wait_until(
        "durable Plan completion report",
        Duration::from_secs(15),
        |_| {
            has_durable_completed_plan_report(content.sandbox().grow_home(), COMPLETION_REPORT)
                .unwrap_or(false)
        },
    )?;
    tokio::time::timeout(Duration::from_secs(10), summary.wait_received())
        .await
        .context("normal summary request was not sent after Plan completion")?;
    let summary_request = latest_foreground_request(&content)?;
    let completion_result =
        "The approved Plan is complete. The full report is available in Plan details.";
    if !summary_request.to_string().contains(completion_result) {
        bail!(
            "normal summary request did not include the complete tool result\n{summary_request:#}"
        );
    }
    harness
        .wait_for_text(SUMMARY_SENTINEL, Duration::from_secs(60))
        .context("normal completion summary rendered")?;
    tokio::time::timeout(Duration::from_secs(10), summary.wait_satisfied())
        .await
        .context("normal summary request timeout")?;

    let screen = harness.screen_contents();
    if screen.contains("panicked") {
        bail!("pager panicked during the Plan journey\n{screen}");
    }
    let foreground_requests = foreground_request_count(&content);
    if foreground_requests != 6 {
        bail!(
            "expected setup, three submissions, completion, and one summary request; got {}",
            foreground_requests
        );
    }

    harness.quit().context("clean quit")?;
    Ok(())
}

async fn assert_plan_resume(defer_before_exit: bool) -> Result<()> {
    let content = ContentController::start()
        .await
        .context("start ContentController")?;
    content.seed_llm_config().context("seed mock LLM config")?;

    let mut setup_turn = content.expect_agent_turn(
        "initial ordinary turn",
        format!("{SETUP_SENTINEL}: ready to draft a plan."),
    );
    let mut plan_submit = expect_plan_submit(&content);
    let mut implement_turn = content.expect_agent_turn(
        "implementation after approval",
        format!("{IMPLEMENT_SENTINEL}: implementing the approved plan."),
    );

    let project = tempfile::tempdir().context("project dir")?;
    std::fs::create_dir_all(project.path().join(".git")).context("create .git")?;

    let binary = pager_binary().context("resolve pager binary")?;
    let mut first = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        Some(project.path()),
    )
    .context("spawn first pager")?;

    wait_for_welcome(&mut first).await?;
    first.inject_keys(b"go\r").context("submit ordinary turn")?;
    first
        .wait_for_text(SETUP_SENTINEL, Duration::from_secs(30))
        .context("ordinary turn rendered")?;
    tokio::time::timeout(Duration::from_secs(10), setup_turn.wait_satisfied())
        .await
        .context("ordinary turn expectation timeout")?;

    // Use the same Plan prompt and structured tool call as the focused Plan
    // PTY flow. The resulting approval is tied to this live runtime source.
    first
        .inject_keys(b"/plan present the plan\r")
        .context("submit /plan prompt")?;
    first
        .wait_for_text("request changes", Duration::from_secs(60))
        .context("real plan_control submit should open approval chrome")?;
    tokio::time::timeout(Duration::from_secs(10), plan_submit.wait_satisfied())
        .await
        .context("plan_control submit expectation timeout")?;
    first
        .wait_for_text("q abandon", Duration::from_secs(10))
        .context("plan approval chrome rendered")?;
    let screen = first.screen_contents();
    if !screen.contains("GBT3703Repro") && !screen.contains("Exercise the real plan approval flow")
    {
        bail!("expected the submitted plan body in the approval view\n{screen}");
    }
    if first.contains_text("panicked") {
        bail!("pager panicked while parking the submitted plan\n{screen}");
    }

    if defer_before_exit {
        // Defer through the actual approval view. This records the immutable
        // decision while leaving the candidate available for a later review.
        first
            .inject_keys(b"l")
            .context("defer the submitted plan")?;
        first
            .wait_for_text("Plan decision confirmed.", Duration::from_secs(15))
            .context("defer decision confirmation is released automatically")?;
        first
            .wait_for_text("Worked for", Duration::from_secs(15))
            .context("defer tool turn completed")?;
        // plan_control freezes the submitted body after trimming its edges.
        let plan_hash = blake3::hash(PLAN_BODY.trim().as_bytes())
            .to_hex()
            .to_string();
        first.wait_until(
            "durable Deferred Plan receipt",
            Duration::from_secs(15),
            |_| {
                has_durable_deferred_receipt(content.sandbox().grow_home(), &plan_hash)
                    .unwrap_or(false)
            },
        )?;

        first
            .inject_keys(b"/view-plan\r")
            .context("open retained Plan preview once after confirmation")?;
        first
            .wait_for_text("r review", Duration::from_secs(20))
            .context("retained Plan preview is reviewable")?;
        if foreground_request_count(&content) != 2 {
            bail!("deferring a Plan must not sample implementation work");
        }
        first
            .inject_keys(b"\x1b")
            .context("close the read-only retained Plan preview before global exit")?;
        first.wait_until(
            "retained Plan preview closed",
            Duration::from_secs(5),
            |h| !h.contains_text("Esc:close"),
        )?;
    }

    // The shell writes its runtime checkpoint and Control state as part of the
    // normal exit path, whether approval is live or the Plan is retained.
    first
        .inject_keys(b"\x04\x04")
        .context("suspend pager with pending plan approval")?;
    let exit = first
        .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
        .context("confirm Plan state was durably suspended")?;
    if exit != 0 {
        bail!("pending Plan suspension exited with {exit}");
    }
    if foreground_request_count(&content) != 2 {
        bail!(
            "suspension must not sample or implement the unapproved plan; foreground requests: {}",
            foreground_request_count(&content)
        );
    }
    drop(first);

    let replay_log = content.home().join("plan-resume-phases.jsonl");
    let mut resumed = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue"],
        &[
            ("GROW_INSTRUMENTATION", "log"),
            (
                "GROW_INSTRUMENTATION_LOG",
                replay_log.to_str().context("UTF-8 replay log path")?,
            ),
        ],
        Some(project.path()),
    )
    .context("spawn resumed pager")?;

    // History is visible before ordinary input is enabled. This log belongs
    // only to the resumed process; do not send commands into its replay window.
    resumed.wait_until("completed session replay", WELCOME_TIMEOUT, |_| {
        std::fs::read_to_string(&replay_log).is_ok_and(|log| log.contains("pager.replay.finalize"))
    })?;

    if defer_before_exit {
        resumed
            .wait_for_text(SETUP_SENTINEL, WELCOME_TIMEOUT)
            .context("restored transcript after --continue")?;
        resumed
            .inject_keys(b"/view-plan\r")
            .context("open the retained Plan preview")?;
        resumed
            .wait_for_text("GBT3703Repro", Duration::from_secs(20))
            .context("retained Plan artifact rendered after resume")?;
        // The resumed transcript already contains the plan title, so that
        // text alone can match before `/view-plan` has opened its preview.
        // Wait for the preview-only action footer before sending its shortcut.
        resumed
            .wait_for_text("r review", Duration::from_secs(20))
            .context("retained Plan review shortcut rendered after resume")?;
        resumed
            .inject_keys(b"r")
            .context("review the retained Plan")?;
        if let Err(error) = resumed.wait_until(
            "retained Plan review acceptance or rejection",
            Duration::from_secs(10),
            |h| {
                h.contains_text("request changes")
                    || h.contains_text("Plan review was not confirmed")
            },
        ) {
            let failure_screen = resumed.screen_contents();
            bail!(
                "retained Plan review did not resolve: {error}\n\
                 screen:\n{failure_screen}"
            );
        }
        if !resumed.contains_text("request changes") {
            bail!(
                "the retained Plan review request was rejected:\n{}",
                resumed.screen_contents()
            );
        }
    } else {
        resumed
            .wait_for_text("request changes", WELCOME_TIMEOUT)
            .context("restored approval 'request changes' after --continue")?;
    }
    resumed
        .wait_for_text("q abandon", Duration::from_secs(10))
        .context("approval chrome rendered")?;
    let screen = resumed.screen_contents();
    if !screen.contains("approve") {
        bail!("expected approval action after resume\n{screen}");
    }
    if !screen.contains("GBT3703Repro") && !screen.contains(SETUP_SENTINEL) {
        bail!("expected resumed transcript or original plan body\n{screen}");
    }
    if resumed.contains_text("panicked") {
        bail!("pager panicked after resume\n{screen}");
    }

    resumed.inject_keys(b"a").context("approve restored plan")?;
    resumed
        .wait_for_text(IMPLEMENT_SENTINEL, Duration::from_secs(60))
        .context("approval should admit the implementation turn")?;
    tokio::time::timeout(Duration::from_secs(10), implement_turn.wait_satisfied())
        .await
        .context("implementation turn expectation timeout")?;
    if foreground_request_count(&content) != 3 {
        bail!(
            "approval must continue exactly once without replaying the submitted plan; foreground requests: {}",
            foreground_request_count(&content)
        );
    }

    resumed.quit().context("quit resumed pager")?;
    Ok(())
}

fn foreground_request_count(content: &ContentController) -> usize {
    content
        .requests()
        .iter()
        .filter(|request| {
            request
                .body
                .as_ref()
                .and_then(|body| body.get("tools"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tools| tools.len() >= 2)
        })
        .count()
}

fn has_durable_deferred_receipt(grow_home: &std::path::Path, artifact_hash: &str) -> Result<bool> {
    let sessions = grow_home.join("sessions");
    let sessions = match std::fs::read_dir(sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut matches = 0;
    for cwd_entry in sessions {
        let cwd = cwd_entry?.path();
        if !cwd.is_dir() {
            continue;
        }
        for session_entry in std::fs::read_dir(cwd)? {
            let session = session_entry?.path();
            if !session.is_dir() {
                continue;
            }
            let timeline = session.join("timeline.jsonl");
            let typed_timeline = match shell::session::testkit::read_timeline_fixture(&timeline) {
                Ok(timeline) => timeline,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let mut latest_behavior = None;
            for event in typed_timeline.events() {
                let event = serde_json::to_value(event)?;
                if event.get("type").and_then(serde_json::Value::as_str) == Some("control") {
                    latest_behavior = event
                        .get("event")
                        .and_then(|event| event.get("snapshot"))
                        .and_then(|snapshot| snapshot.get("behavior"))
                        .cloned();
                }
            }
            let Some(candidate) = latest_behavior
                .as_ref()
                .and_then(|behavior| behavior.get("retained_plan"))
            else {
                continue;
            };
            let decision = candidate
                .get("decisions")
                .and_then(serde_json::Value::as_array)
                .and_then(|decisions| (decisions.len() == 1).then(|| &decisions[0]));
            if candidate
                .get("artifact_hash")
                .and_then(serde_json::Value::as_str)
                == Some(artifact_hash)
                && candidate.get("status").and_then(serde_json::Value::as_str) == Some("deferred")
                && decision
                    .and_then(|decision| decision.get("action"))
                    .and_then(serde_json::Value::as_str)
                    == Some("defer")
                && decision
                    .and_then(|decision| decision.get("decision_id"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|decision_id| !decision_id.trim().is_empty())
            {
                matches += 1;
            }
        }
    }
    if matches > 1 {
        bail!("deferred Plan artifact appeared in multiple session owners");
    }
    Ok(matches == 1)
}

fn expect_plan_submit(content: &ContentController) -> AgentTurnExpectation {
    expect_plan_submit_with_body(content, "call_plan_submit_resume", PLAN_BODY)
}

fn expect_plan_submit_with_body(
    content: &ContentController,
    call_id: &str,
    plan: &str,
) -> AgentTurnExpectation {
    let arguments = serde_json::json!({
        "action": "submit",
        "plan": plan,
    })
    .to_string();
    expect_plan_tool_call(content, call_id, arguments)
}

fn expect_plan_tool_call(
    content: &ContentController,
    call_id: &str,
    arguments: String,
) -> AgentTurnExpectation {
    content.expect_agent_turn_with_responses(
        format!("plan_control {call_id}"),
        ScriptedResponse::sse(responses_tool_call_events(
            call_id,
            "plan_control",
            &arguments,
        )),
        ScriptedResponse::sse(chat_tool_call_events(call_id, "plan_control", &arguments)),
    )
}

fn plan_body(version: &str) -> String {
    format!(
        "# GBT3703PLAN{version}\n\n## Steps\n1. Preserve current behavior\n2. Add the requested verification\n"
    )
}

async fn wait_for_plan_candidate(
    harness: &mut PtyHarness,
    marker: &str,
    expectation: &mut AgentTurnExpectation,
) -> Result<()> {
    harness
        .wait_for_text("request changes", Duration::from_secs(60))
        .with_context(|| format!("Plan candidate {marker} opened approval"))?;
    harness
        .wait_for_text(marker, Duration::from_secs(20))
        .with_context(|| format!("Plan candidate {marker} rendered"))?;
    tokio::time::timeout(Duration::from_secs(10), expectation.wait_satisfied())
        .await
        .with_context(|| format!("Plan candidate {marker} request timeout"))?;
    let screen = harness.screen_contents();
    if screen.contains("awaiting confirmation") || screen.contains("check confirmation") {
        bail!("new Plan approval remained blocked by decision confirmation\n{screen}");
    }
    Ok(())
}

async fn wait_for_feedback_request(
    content: &ContentController,
    feedback: &[&str],
    expectation: &mut AgentTurnExpectation,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), expectation.wait_received())
        .await
        .with_context(|| format!("model request after feedback timed out: {feedback:?}"))?;
    let request = latest_foreground_request(content)?;
    let request_text = request.to_string();
    for text in feedback {
        if !request_text.contains(text) {
            bail!("next model request did not include feedback `{text}`\n{request:#}");
        }
    }
    Ok(())
}

fn latest_foreground_request(content: &ContentController) -> Result<serde_json::Value> {
    content
        .requests()
        .into_iter()
        .rev()
        .filter_map(|entry| entry.body)
        .find(|body| {
            body["tools"]
                .as_array()
                .is_some_and(|tools| tools.len() >= 2)
        })
        .context("mock provider did not record a foreground model request")
}

fn has_durable_completed_plan_report(
    grow_home: &std::path::Path,
    expected_report: &str,
) -> Result<bool> {
    let expected_hash = blake3::hash(expected_report.as_bytes())
        .to_hex()
        .to_string();
    let sessions = grow_home.join("sessions");
    let sessions = match std::fs::read_dir(sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut matches = 0;
    for cwd_entry in sessions {
        let cwd = cwd_entry?.path();
        if !cwd.is_dir() {
            continue;
        }
        for session_entry in std::fs::read_dir(cwd)? {
            let session = session_entry?.path();
            if !session.is_dir() {
                continue;
            }
            let timeline = session.join("timeline.jsonl");
            let typed_timeline = match shell::session::testkit::read_timeline_fixture(&timeline) {
                Ok(timeline) => timeline,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let report_hash = typed_timeline.events().iter().rev().find_map(|event| {
                let event = serde_json::to_value(event).ok()?;
                if event.get("type")?.as_str()? != "control" {
                    return None;
                }
                event
                    .get("event")?
                    .get("snapshot")?
                    .get("behavior")?
                    .get("completed_plan")?
                    .get("report_hash")?
                    .as_str()
                    .map(str::to_owned)
            });
            if report_hash.as_deref() != Some(expected_hash.as_str()) {
                continue;
            }
            let report_path = session
                .join("artifacts")
                .join("plan")
                .join(format!("{expected_hash}.md"));
            if std::fs::read_to_string(report_path)? != expected_report {
                bail!("completed Plan report hash points to different content");
            }
            matches += 1;
        }
    }
    if matches > 1 {
        bail!("completed Plan report appeared in multiple session owners");
    }
    Ok(matches == 1)
}

fn responses_tool_call_events(call_id: &str, name: &str, arguments: &str) -> Vec<SseEvent> {
    vec![
        SseEvent::data(
            serde_json::json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": {
                    "id": format!("resp_{call_id}"),
                    "object": "response",
                    "created_at": 1234567890,
                    "model": "test-model",
                    "status": "in_progress",
                    "output": []
                }
            })
            .to_string(),
        ),
        SseEvent::data(
            serde_json::json!({
                "type": "response.function_call_arguments.delta",
                "sequence_number": 1,
                "item_id": call_id,
                "output_index": 0,
                "delta": arguments
            })
            .to_string(),
        ),
        SseEvent::data(
            serde_json::json!({
                "type": "response.completed",
                "sequence_number": 2,
                "response": {
                    "id": format!("resp_{call_id}"),
                    "object": "response",
                    "created_at": 1234567890,
                    "model": "test-model",
                    "status": "completed",
                    "output": [{
                        "type": "function_call",
                        "call_id": call_id,
                        "name": name,
                        "arguments": arguments
                    }],
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 20,
                        "total_tokens": 30,
                        "input_tokens_details": { "cached_tokens": 0 },
                        "output_tokens_details": { "reasoning_tokens": 0 }
                    }
                }
            })
            .to_string(),
        ),
        SseEvent::data("[DONE]".to_owned()),
    ]
}

fn chat_tool_call_events(call_id: &str, name: &str, arguments: &str) -> Vec<SseEvent> {
    vec![
        SseEvent::data(
            serde_json::json!({
                "id": format!("chatcmpl-{call_id}"),
                "object": "chat.completion.chunk",
                "created": 1234567890,
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "index": 0,
                            "id": call_id,
                            "type": "function",
                            "function": { "name": name, "arguments": arguments }
                        }]
                    },
                    "finish_reason": null
                }]
            })
            .to_string(),
        ),
        SseEvent::data(
            serde_json::json!({
                "id": format!("chatcmpl-{call_id}"),
                "object": "chat.completion.chunk",
                "created": 1234567890,
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 20,
                    "total_tokens": 30
                }
            })
            .to_string(),
        ),
        SseEvent::data("[DONE]".to_owned()),
    ]
}
