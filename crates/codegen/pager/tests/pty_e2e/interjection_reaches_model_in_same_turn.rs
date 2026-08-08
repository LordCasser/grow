// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Ctrl+Enter steers the current regular turn. The sampler may start another
/// request so the model can consume the new user message, but the pager keeps
/// one foreground identity and the shell emits only that turn's terminal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn interjection_reaches_model_in_same_turn() {
    let content = ContentController::start().await.expect("start content");
    content.seed_llm_config().expect("seed mock LLM config");
    // Gate turn 1's terminal event so the typed text + chord provably land
    // mid-turn regardless of suite load. Chunk delay widens the mid-stream
    // window under remote CI load (same shape as cancel_discards_*).
    let mut turn_one = content
        .expect_agent_turn_blocked("running turn before steering", slow_turn_text("TURNONE"));
    content.set_chunk_delay(Some(Duration::from_millis(100)));
    let _turn_two =
        content.expect_agent_turn("steered message", "TURNTWO reply to the steered message.");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text("TURNONE", Duration::from_secs(30))
        .expect("turn 1 streaming");
    tokio::time::timeout(Duration::from_secs(10), turn_one.wait_blocked())
        .await
        .expect("turn 1 reached completion barrier");
    // Still mid-stream (hold gates completion) — not "Worked for".
    assert!(
        !harness.contains_text("Worked for"),
        "turn must still be open before steering\nscreen:\n{}",
        harness.screen_contents()
    );

    harness
        .inject_keys(b"please also check the logs")
        .expect("type message");
    harness
        .wait_for_text("please also check the logs", Duration::from_secs(5))
        .expect("draft visible in composer");
    harness.inject_keys(CTRL_ENTER).expect("steer chord");
    turn_one.release();

    // Cancel-and-send: message leaves the composer and commits as a scrollback
    // user block (not just the draft line that also carries ❯).
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.update(Duration::from_millis(100));
        if !composer_holds(&harness, "please also check the logs")
            && block_lines_containing(&harness, "please also check the logs") >= 1
        {
            break;
        }
        if Instant::now() >= deadline {
            panic!(
                "steering did not commit draft to scrollback\nscreen:\n{}",
                harness.screen_contents()
            );
        }
    }
    harness
        .wait_for_text("TURNTWO", Duration::from_secs(40))
        .expect("steered message reached the next sampling cycle");

    // Steering never cancels the foreground turn.
    assert!(
        !harness.contains_text("Turn cancelled by user"),
        "steering must not render a cancelled marker\nscreen:\n{}",
        harness.screen_contents()
    );

    let users = all_user_messages(&content);
    let sent = users
        .iter()
        .find(|u| u.contains("please also check the logs"))
        .unwrap_or_else(|| panic!("steered message never reached the wire: {users:#?}"));
    assert!(
        sent.contains(INTERJECTION_WIRE_PREFIX),
        "same-turn steering must use the interjection envelope: {sent}"
    );
    assert!(
        sent.contains("<user_query>"),
        "steering must retain the user_query envelope: {sent}"
    );

    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );
    harness.quit().expect("clean quit");
}
