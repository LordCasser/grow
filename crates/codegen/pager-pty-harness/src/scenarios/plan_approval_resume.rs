//! Plan-approval chrome restored by the shell after quit + resume.
//!
//! When Plan approval is parked and the user quits, the shell persists
//! `approval_pending = true` as a Timeline Control event. On `--continue` the
//! shell re-issues the `grow/plan_approval` reverse-request — a real live ACP
//! waiter — so the pager re-shows approval chrome through its normal path with
//! no pager-side disk logic. Approving then enters the Plan Executing phase and
//! starts the durable handoff turn because no live Plan turn survived restart.
//!
//! This FAILS without the shell re-park (PR2 product change): no reverse-request
//! reaches the resumed pager, so no approval chrome appears.

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::wait_for_welcome;
use crate::{ContentController, PtyHarness, pager_binary};

const DEFAULT_ROWS: u16 = 50;
const DEFAULT_COLS: u16 = 120;
const WELCOME_TIMEOUT: Duration = Duration::from_secs(20);
/// Distinct per-turn sentinels: turn 1 seeds the session before quit; turn 2 is
/// the implement turn the shell injects after the resumed approval is approved.
const SETUP_SENTINEL: &str = "GBT3703SETUP";
const IMPLEMENT_SENTINEL: &str = "GBT3703IMPLEMENTED";

const PLAN_BODY: &str = "\
# Plan GBT3703Repro

## Steps
1. Seed plan file on disk
2. Quit pager with the approval parked
3. Resume and expect restored approval chrome
";

/// Regression: the shell re-parks Plan approval on resume; pressing approve
/// enters Plan execution and starts the durable handoff turn.
pub async fn assert_plan_approval_restored_after_resume() -> Result<()> {
    let content = ContentController::start()
        .await
        .context("start ContentController")?;

    // BYOK gate: the pager refuses to start without a configured LLM
    // ([models].default + a provider base_url). Point it at the mock server.
    content.seed_llm_config().context("seed mock LLM config")?;

    let mut setup_turn = content.expect_agent_turn(
        "initial plan-drafting turn",
        format!("{SETUP_SENTINEL}: drafted a plan for the user to review."),
    );
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

    first.inject_keys(b"go\r").context("submit setup turn")?;
    first
        .wait_for_text(SETUP_SENTINEL, Duration::from_secs(30))
        .context("setup turn rendered")?;
    tokio::time::timeout(Duration::from_secs(10), setup_turn.wait_satisfied())
        .await
        .context("setup turn expectation timeout")?;

    // Quit and reap BEFORE seeding so the still-live shell cannot re-persist
    // and clobber the seeded state.
    first.inject_keys(b"\x11").context("ctrl-q once")?;
    first.update(Duration::from_millis(200));
    first.inject_keys(b"\x11").context("ctrl-q confirm")?;
    first.quit().context("reap first pager")?;

    let seeded = seed_parked_approval(content.home()).context("seed parked approval")?;
    assert!(seeded > 0, "no session dir seeded");

    let mut resumed = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue"],
        Some(project.path()),
    )
    .context("spawn resumed pager")?;

    // The shell re-parks Plan approval on resume, so approval chrome can open
    // immediately and cover chat history. Prefer the chrome markers (product
    // signal) over SETUP_SENTINEL, which may not be visible under the plan viewer.
    // Without the shell re-park this times out.
    resumed
        .wait_for_text("request changes", WELCOME_TIMEOUT)
        .context("restored approval 'request changes' after --continue")?;
    resumed
        .wait_for_text("quit plan", Duration::from_secs(5))
        .context("restored approval 'quit plan' after resume")?;
    let screen = resumed.screen_contents();
    if !screen.contains("approve") {
        bail!("expected approval primary action after resume\n{screen}");
    }
    // History was seeded before quit; plan body from disk is a stronger signal
    // that the session was restored when chrome already covers the transcript.
    if !screen.contains("GBT3703Repro")
        && !screen.contains(SETUP_SENTINEL)
        && !screen.contains("Seed plan file on disk")
    {
        bail!("expected resumed session content (plan body or setup sentinel)\n{screen}");
    }
    if resumed.contains_text("panicked") {
        bail!("pager panicked\n{screen}");
    }

    // Approve: the shell enters Plan execution and admits the durable handoff turn.
    resumed.inject_keys(b"a").context("press 'a' to approve")?;
    resumed
        .wait_for_text(IMPLEMENT_SENTINEL, Duration::from_secs(30))
        .context("approve must enter Plan execution and start the handoff turn")?;
    tokio::time::timeout(Duration::from_secs(10), implement_turn.wait_satisfied())
        .await
        .context("implement turn expectation timeout")?;

    resumed.quit().context("quit resumed pager")?;
    Ok(())
}

/// Mark the persisted session as having a parked plan approval: write the
/// immutable content-addressed Plan artifact and append a Control event to
/// `timeline.jsonl` for every session directory under the sandbox home.
fn seed_parked_approval(home: &Path) -> Result<usize> {
    let sessions_root = home.join(".grow").join("sessions");
    if !sessions_root.is_dir() {
        bail!(
            "expected sessions under {} after first turn",
            sessions_root.display()
        );
    }
    let mut seeded = 0usize;
    for cwd_ent in std::fs::read_dir(&sessions_root).context("read sessions root")? {
        let cwd_ent = cwd_ent.context("cwd entry")?;
        if !cwd_ent.file_type().context("ft")?.is_dir() {
            continue;
        }
        for sess_ent in std::fs::read_dir(cwd_ent.path()).context("read cwd sessions")? {
            let sess_ent = sess_ent.context("session entry")?;
            if !sess_ent.file_type().context("ft")?.is_dir() {
                continue;
            }
            let dir = sess_ent.path();
            let plan_hash = blake3::hash(PLAN_BODY.as_bytes()).to_hex().to_string();
            let artifact_dir = dir.join("artifacts").join("plan");
            std::fs::create_dir_all(&artifact_dir).context("create Plan artifact directory")?;
            std::fs::write(artifact_dir.join(format!("{plan_hash}.md")), PLAN_BODY)
                .context("write immutable Plan artifact")?;
            append_awaiting_plan_control(&dir.join("timeline.jsonl"))?;
            seeded += 1;
        }
    }
    if seeded == 0 {
        bail!(
            "expected at least one session dir under {}",
            sessions_root.display()
        );
    }
    Ok(seeded)
}

/// Round-trip the latest shell-written Control snapshot and append the next
/// monotonic event, preserving Goal and every unrelated field. The harness
/// mirrors only the public Timeline JSON shape instead of depending on shell.
fn append_awaiting_plan_control(path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(path).context("read timeline.jsonl")?;
    let mut expected_seq = 0_u64;
    let mut timeline_version = None;
    let mut latest_control = None;
    let mut latest_revision = 0_u64;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let value: serde_json::Value =
            serde_json::from_str(line).context("parse Timeline event")?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .context("Timeline event version must be an integer")?;
        if timeline_version
            .replace(version)
            .is_some_and(|seen| seen != version)
        {
            bail!("Timeline schema version changed within one ledger");
        }
        let seq = value
            .get("seq")
            .and_then(serde_json::Value::as_u64)
            .context("Timeline event seq must be an integer")?;
        if seq != expected_seq {
            bail!("Timeline seq {seq} is not contiguous; expected {expected_seq}");
        }
        expected_seq = expected_seq.saturating_add(1);
        if value.get("type").and_then(serde_json::Value::as_str) == Some("control") {
            let control = value
                .get("event")
                .and_then(serde_json::Value::as_object)
                .context("Timeline control event must be an object")?;
            latest_revision = control
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .context("Timeline control revision must be an integer")?;
            latest_control = Some(
                control
                    .get("snapshot")
                    .cloned()
                    .context("Timeline control event must carry a snapshot")?,
            );
        }
    }
    let revision = latest_revision.saturating_add(1).max(1);
    let mut snapshot = latest_control.context(
        "new sessions must durably seed their current Control snapshot before the harness mutates it",
    )?;
    let obj = snapshot
        .as_object_mut()
        .context("Timeline control snapshot must be a JSON object")?;
    obj.insert("control_revision".into(), serde_json::json!(revision));
    let behavior = obj
        .entry("behavior")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("Timeline control behavior must be an object")?;
    // The Plan phase and transport flag must agree for a valid re-park.
    behavior.insert(
        "state".into(),
        serde_json::json!({ "Plan": "AwaitingApproval" }),
    );
    behavior.insert("approval_pending".into(), serde_json::Value::Bool(true));
    behavior.insert("plan_artifact_revision".into(), serde_json::json!(1));
    behavior.insert(
        "plan_artifact_hash".into(),
        serde_json::json!(blake3::hash(PLAN_BODY.as_bytes()).to_hex().to_string()),
    );
    behavior.insert("last_plan_handoff".into(), serde_json::Value::Null);
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64;
    let event = serde_json::json!({
        "version": timeline_version.context("Timeline must contain a seeded event")?,
        "seq": expected_seq,
        "at_ms": at_ms,
        "type": "control",
        "event": {
            "revision": revision,
            "snapshot": snapshot,
            "retired_context_layers": ["goal_definition"],
            "model_contexts": [
                {
                    "layer": "behavior",
                    "activation": "transition",
                    "item": system_reminder_json(
                        "Plan behavior is active. The Plan phase contract is authoritative."
                    ),
                },
                {
                    "layer": "plan_phase",
                    "activation": "transition",
                    "item": system_reminder_json(format!(
                        "Plan phase: AwaitingApproval. The following plan is frozen and awaiting a human decision. Do not execute it or modify the workspace.\n\n{PLAN_BODY}"
                    )),
                },
            ],
        },
    });
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .context("open timeline.jsonl for append")?;
    if !raw.is_empty() && !raw.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    serde_json::to_writer(&mut file, &event)?;
    file.write_all(b"\n")?;
    file.sync_all()
        .context("durably append Timeline control event")?;
    Ok(())
}

fn system_reminder_json(text: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "type": "user",
        "content": [{ "type": "text", "text": text.into() }],
        "synthetic_reason": "system_reminder",
    })
}
