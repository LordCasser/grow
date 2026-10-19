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
        .wait_for_text("quit plan", Duration::from_secs(10))
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

        // The first status query is sent with the reverse response and may
        // arrive before Control commits. The exact pending decision remains
        // available through ShowPlan; retry it, then read the retained artifact.
        first
            .inject_keys(b"/view-plan\r")
            .context("recheck the deferred decision receipt")?;
        first.wait_until(
            "durable receipt acknowledgement or Plan preview",
            Duration::from_secs(15),
            |h| h.contains_text("Plan decision confirmed.") || h.contains_text("GBT3703Repro"),
        )?;
        if !first.contains_text("GBT3703Repro") {
            first
                .inject_keys(b"/view-plan\r")
                .context("open the retained Plan after receipt confirmation")?;
            first
                .wait_for_text("GBT3703Repro", Duration::from_secs(20))
                .context("retained Plan preview rendered")?;
        }
        if !first.screen_contents().to_lowercase().contains("review") {
            bail!(
                "expected the retained Plan preview to offer Review\n{}",
                first.screen_contents()
            );
        }
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
        .inject_keys(b"\x03")
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

    let mut resumed = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue"],
        Some(project.path()),
    )
    .context("spawn resumed pager")?;

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
        .wait_for_text("quit plan", Duration::from_secs(10))
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
    use std::io::BufRead;

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
            let file = match std::fs::File::open(timeline) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let mut latest_behavior = None;
            for line in std::io::BufReader::new(file).lines() {
                let line = line?;
                let event: serde_json::Value = serde_json::from_str(&line)?;
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
    let call_id = "call_plan_submit_resume";
    let arguments = serde_json::json!({
        "action": "submit",
        "plan": PLAN_BODY,
    })
    .to_string();
    content.expect_agent_turn_with_responses(
        "plan_control submit",
        ScriptedResponse::sse(responses_tool_call_events(
            call_id,
            "plan_control",
            &arguments,
        )),
        ScriptedResponse::sse(chat_tool_call_events(call_id, "plan_control", &arguments)),
    )
}

fn responses_tool_call_events(call_id: &str, name: &str, arguments: &str) -> Vec<SseEvent> {
    vec![
        SseEvent::data(
            serde_json::json!({
                "type": "response.created",
                "sequence_number": 0,
                "response": {
                    "id": "resp_plan_submit_resume",
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
                    "id": "resp_plan_submit_resume",
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
                "id": "chatcmpl-plan-submit-resume",
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
                "id": "chatcmpl-plan-submit-resume",
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
