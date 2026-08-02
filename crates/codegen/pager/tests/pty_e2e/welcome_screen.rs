// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// 1. **Welcome screen.**
/// The pager boots and draws its welcome screen within the timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen() {
    let content = ContentController::start().await.expect("start content");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    // The home page has no input box: the prompt placeholder belongs to agent
    // sessions only. The harness runs in API-key auth mode, so this screen is
    // the home page — not a login gate that would render a prompt.
    assert!(
        !harness.contains_text("Type a message"),
        "home page must not render a prompt placeholder\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}

/// 1d. **Any uncaught key starts a new session.**
/// The home page has no input box, so typing a plain character leaves the
/// welcome screen, starts a new agent session, and forwards the key into the
/// session prompt's draft.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn typing_any_character_starts_session_and_leaves_home() {
    let content = ContentController::start().await.expect("start content");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    // The first char starts the session; the rest accumulate into the prompt
    // draft (no Enter yet, so no turn fires and the mock stays quiet).
    harness.inject_keys(b"hello").expect("type draft");

    harness
        .wait_for_text("hello", Duration::from_secs(10))
        .unwrap_or_else(|_| {
            panic!(
                "typed draft never rendered in the session prompt\nscreen:\n{}",
                harness.screen_contents()
            )
        });
    // Leaving home: the welcome menu (and its "Quit" row) is gone.
    harness
        .wait_for_text_absent(WELCOME_SCREEN_SENTINEL, Duration::from_secs(10))
        .unwrap_or_else(|_| {
            panic!(
                "welcome menu still visible after typing (home → session never happened)\n\
                 screen:\n{}",
                harness.screen_contents()
            )
        });

    // Graceful quit: focus is in the prompt with a draft, so 'q' would type —
    // use the Ctrl+Q double-press chord instead (see `rename_title_shows_in_prompt_border`).
    harness.update(Duration::from_millis(500));
    harness.inject_keys(b"\x11").expect("ctrl-q arm");
    harness.update(Duration::from_millis(200));
    harness.inject_keys(b"\x11").expect("ctrl-q confirm");
    harness.quit().expect("reap pager");
}

/// 1e. **Agent empty-state logo.**
/// A fresh session has an empty scrollback: the agent view shows only the
/// centered Grow logo (big tier at the default size — the agent pane is 39+
/// rows tall). Once the mock response streams into the scrollback, the logo
/// disappears.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn agent_empty_state_logo_shows_until_content_streams() {
    let content = ContentController::start().await.expect("start content");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} empty-state reply."));

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    // Start a session with a draft; the scrollback is still empty here.
    harness.inject_keys(b"hello").expect("type draft");
    harness
        .wait_for_text_absent(WELCOME_SCREEN_SENTINEL, Duration::from_secs(10))
        .expect("left the welcome screen");

    // The empty session shows the logo centered in the scrollback (~col 56 at
    // 120 cols — the agent pane is 39+ rows tall, so the BIG tier renders;
    // the home hero's side-by-side slot is capped at the 113-col small gate).
    let screen = harness.screen_contents();
    let (row, col) = locate_screen_text(&screen, WELCOME_BIG_LOGO_MOTIF)
        .unwrap_or_else(|| panic!("empty-state logo not found\nscreen:\n{screen}"));
    assert!(
        col >= 40,
        "empty-state logo must be centered, not pinned left: motif at col {col} \
         (row {row})\nscreen:\n{screen}"
    );

    // Submit the draft: the response streams into the scrollback, and the logo
    // must be gone once any content is on screen.
    harness.inject_keys(b"\r").expect("submit draft");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("response streamed");
    assert!(
        !harness.contains_text(WELCOME_BIG_LOGO_MOTIF),
        "empty-state logo must disappear once the scrollback has content\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}
