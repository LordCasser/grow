// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// 1b. **Welcome home page renders the small logo at the default terminal size.**
///
/// The borderless hero tiers the logo by the content area: at 120 cols the
/// side-by-side left column is `max(60, 34)` = 60 wide (≥ the 34-col small
/// gate) and 50 rows clears the 19-row gate, so the small art
/// (`grow-small.txt`, 30×15) is selected — the big art (80×35) must not
/// appear.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen_logo_renders_correctly() {
    let content = ContentController::start().await.expect("start content");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    let screen = harness.screen_contents();
    assert!(
        screen.contains(WELCOME_SMALL_LOGO_MOTIF),
        "small-logo motif {WELCOME_SMALL_LOGO_MOTIF:?} not found in the welcome screen\n\
         Screen contents:\n{screen}"
    );
    assert!(
        !screen.contains(WELCOME_BIG_LOGO_MOTIF),
        "big-logo motif {WELCOME_BIG_LOGO_MOTIF:?} must not render at 120x50\n\
         Screen contents:\n{screen}"
    );

    harness.quit().expect("clean quit");
}

/// 1c. **Welcome home page renders the big logo on a wide terminal.**
///
/// At 180 cols the side-by-side left column is `max(90, 84)` = 90 wide
/// (≥ the 84-col big gate) and 50 rows clears the 39-row gate, so the big art
/// (`grow-big.txt`, 80×35) is selected — the small art must not appear
/// alongside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen_big_logo_renders_on_wide_terminal() {
    let content = ContentController::start().await.expect("start content");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, 180, &content, &[])
        .expect("spawn pager at 180x50");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    let screen = harness.screen_contents();
    assert!(
        screen.contains(WELCOME_BIG_LOGO_MOTIF),
        "big-logo motif {WELCOME_BIG_LOGO_MOTIF:?} not found in the welcome screen at 180x50\n\
         Screen contents:\n{screen}"
    );
    assert!(
        !screen.contains(WELCOME_SMALL_LOGO_MOTIF),
        "small-logo motif {WELCOME_SMALL_LOGO_MOTIF:?} must not render when the big logo fits\n\
         Screen contents:\n{screen}"
    );

    harness.quit().expect("clean quit");
}
