// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// 1b. **Welcome screen renders the Grow world-tree emblem correctly.**
///
/// The full logo is selected on a tall terminal. Its canopy, trunk, and roots
/// are all rendered from the embedded Braille asset.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen_world_tree_logo_renders_correctly() {
    let content = ContentController::start().await.expect("start content");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    let screen = harness.screen_contents();
    for motif in ["⣹⣷⣶⣼⣿⣿⣷", "⢻⣿⣿⣿⡟", "⣿⣿⣿⣿⣿"] {
        assert!(
            screen.contains(motif),
            "world-tree motif {motif:?} not found in welcome screen\n\
             Screen contents:\n{screen}"
        );
    }

    harness.quit().expect("clean quit");
}
