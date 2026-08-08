// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// A `!` row queued mid-turn is not valid steering input. Empty Enter leaves
/// it in FIFO without cancelling the regular turn; after that turn ends the
/// row drains as real bash, never model prompt/interjection text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
#[cfg(unix)]
async fn bash_queued_mid_turn_drains_as_bash() {
    let content = ContentController::start().await.expect("start content");
    content.seed_llm_config().expect("seed mock LLM config");
    let mut turn_one = content.expect_agent_turn_blocked(
        "running turn before queued bash drain",
        "STEPONE still owns the foreground.",
    );

    let project = tempfile::tempdir().expect("create project dir");
    std::fs::create_dir_all(project.path().join(".git")).expect("create .git");
    let cwd = dunce::canonicalize(project.path()).expect("canonicalize project");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        Some(cwd.as_path()),
    )
    .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text("STEPONE", Duration::from_secs(30))
        .expect("turn 1 streaming");
    tokio::time::timeout(Duration::from_secs(10), turn_one.wait_blocked())
        .await
        .expect("turn 1 reached terminal barrier");

    // `QBASH_%s_OK` keeps the output sentinel out of the queue-row text.
    harness
        .inject_keys(b"!printf 'QBASH_%s_OK\\n' MIDTURN\r")
        .expect("submit bash-mode command mid-turn");
    harness
        .wait_for_text("QBASH_%s_OK", Duration::from_secs(10))
        .expect("bash command visible as a queued row");

    // Empty Enter can only steer prompt-like work. Bash remains queued until
    // the current foreground owner reaches its real terminal.
    assert!(
        !harness.contains_text("Enter:send now"),
        "non-prompt FIFO row must not advertise prompt steering\nscreen:\n{}",
        harness.screen_contents()
    );
    harness
        .inject_keys(b"\r")
        .expect("empty Enter on queued bash");
    harness.update(Duration::from_millis(500));
    assert!(
        !harness.contains_text("QBASH_MIDTURN_OK"),
        "queued bash ran before its foreground predecessor completed\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        !harness.contains_text("Turn cancelled by user"),
        "attempting to steer a bash row must not cancel the turn\nscreen:\n{}",
        harness.screen_contents()
    );

    turn_one.release();
    harness
        .wait_for_text("QBASH_MIDTURN_OK", Duration::from_secs(30))
        .expect("queued bash command executed after FIFO promotion");
    harness
        .wait_for_text("Run (user)", Duration::from_secs(15))
        .expect("Run (user) chrome for the promoted bash turn");

    // Bash rows never render a user-prompt block (the execute block IS the
    // visual entry) — a "❯ !printf…" block would mean the row went to the
    // model as text instead of executing.
    assert!(
        !harness.contains_text("\u{276F} !printf"),
        "bash row must not render a user-prompt block\nscreen:\n{}",
        harness.screen_contents()
    );
    let users = all_user_message_blobs(&content);
    assert!(
        !users.iter().any(|u| u.contains("QBASH")),
        "bash command leaked to the model as a prompt/interjection: {users:#?}"
    );

    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );
    harness.quit().expect("clean quit");
}
