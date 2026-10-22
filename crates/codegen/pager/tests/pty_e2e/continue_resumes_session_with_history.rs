// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// 16. **`--continue` resumes the latest session.**
/// History must render exactly once (duplicate replay and empty pane both
/// fail) and the resumed session must accept a follow-up turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn continue_resumes_session_with_history() {
    let content = ContentController::start().await.expect("start content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    content.set_response(format!("{} first session payload.", turn_sentinel(1)));

    // Sessions are keyed by cwd: both runs must share a stable project dir.
    let project = tempfile::tempdir().expect("create project dir");
    std::fs::create_dir_all(project.path().join(".git")).expect("create .git");

    let binary = pager_binary().expect("resolve pager binary");
    let mut first = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        Some(project.path()),
    )
    .expect("spawn first pager");

    first
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    first
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit turn 1");
    first
        .wait_for_text(&turn_sentinel(1), Duration::from_secs(30))
        .expect("turn 1 rendered");
    // Use the actual global quit path, including its persistence barrier.
    first.update(Duration::from_millis(500));
    first.inject_keys(b"\x04\x04").expect("confirm Ctrl+D");
    assert_eq!(
        first
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .unwrap(),
        0
    );

    let mut resumed = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue"],
        Some(project.path()),
    )
    .expect("spawn resumed pager");

    // Historical text may be visible before the load barrier enables input.
    resumed
        .wait_until_stable(
            "ready resumed history",
            WELCOME_TIMEOUT,
            Duration::from_millis(500),
            |h| {
                h.contains_text(&turn_sentinel(1))
                    && h.contains_text("grow · Mock")
                    && !h.contains_text("Loading session")
            },
        )
        .expect("history replayed and input ready after --continue");
    let screen = resumed.screen_contents();
    assert_eq!(
        screen.matches(&turn_sentinel(1)).count(),
        1,
        "turn 1 must appear exactly once after resume\nscreen:\n{screen}"
    );

    content.set_response(format!("{} resumed session payload.", turn_sentinel(2)));
    resumed
        .inject_keys(b"again\r")
        .expect("submit turn 2 after resume");
    resumed
        .wait_for_text(&turn_sentinel(2), Duration::from_secs(30))
        .expect("turn 2 rendered in resumed session");

    assert!(
        !resumed.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        resumed.screen_contents()
    );

    resumed.inject_keys(b"\x04\x04").expect("confirm Ctrl+D");
    assert_eq!(
        resumed
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .unwrap(),
        0
    );
}
