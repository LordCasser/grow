// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// `/usage`, `/context`, `/session-info` open a tabbed modal instead of
/// writing scrollback blocks (fullscreen/inline). Covers: modal opens on the
/// requested tab, Tab switches tabs, Esc closes, and — the transcript
/// invariant — none of the modal content lands in scrollback.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn usage_modal_opens_switches_and_closes_pty() {
    let content = ContentController::start().await.expect("start content");
    // Spawn-based tests must seed the mock provider config before the pager
    // starts, or the BYOK gate shows the config wizard instead of the TUI.
    content
        .seed_llm_config()
        .expect("seed mock provider config");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} usage modal turn."));

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
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("turn rendered");
    harness.update(Duration::from_millis(400));

    // `/usage` opens the modal on the Usage tab; all three tab labels render.
    harness.inject_keys(b"/usage\r").expect("open usage modal");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if harness.contains_text("Session usage")
            && harness.contains_text("Context")
            && harness.contains_text("Session Info")
        {
            break;
        }
        harness.update(Duration::from_millis(250));
    }
    assert!(
        harness.contains_text("Session usage"),
        "Usage tab must show the local session ledger\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        harness.contains_text("Context") && harness.contains_text("Session Info"),
        "tab bar must render all three tabs\nscreen:\n{}",
        harness.screen_contents()
    );

    // Tab switches to the Context tab (local context-window snapshot).
    harness.inject_keys(b"\t").expect("switch to Context tab");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if harness.contains_text("Auto-compact") || harness.contains_text("System prompt") {
            break;
        }
        harness.update(Duration::from_millis(250));
    }
    assert!(
        harness.contains_text("Auto-compact") || harness.contains_text("System prompt"),
        "Context tab must show the context-window breakdown\nscreen:\n{}",
        harness.screen_contents()
    );

    // Tab again → Session Info tab; the Session ID row renders.
    harness
        .inject_keys(b"\t")
        .expect("switch to Session Info tab");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if harness.contains_text("Session ID") && harness.contains_text("Working directory") {
            break;
        }
        harness.update(Duration::from_millis(250));
    }
    assert!(
        harness.contains_text("Session ID") && harness.contains_text("Working directory"),
        "Session Info tab must show the copyable rows\nscreen:\n{}",
        harness.screen_contents()
    );

    // Esc closes the modal and its content leaves the screen entirely —
    // nothing was written into the transcript.
    harness.inject_keys(keys::ESC).expect("close usage modal");
    harness.update(Duration::from_millis(500));
    assert!(
        !harness.contains_text("Session ID"),
        "modal content must disappear on Esc and never enter the transcript\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        !harness.contains_text("Auto-compact") && !harness.contains_text("Session usage"),
        "no tab content may remain after Esc\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}
