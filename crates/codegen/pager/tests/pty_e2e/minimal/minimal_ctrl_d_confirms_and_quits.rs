// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use crate::common::*;

/// Ctrl+C stays local at idle. Ctrl+D arms a visible one-second confirmation
/// in minimal mode; the second press requests suspension and exits normally.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn minimal_ctrl_d_confirms_and_quits() {
    let content = ContentController::start().await.expect("start content");
    git2::Repository::init(content.home()).expect("initialize isolated project");
    content.seed_llm_config().expect("seed mock LLM config");
    let mut harness = spawn_minimal(&content);
    wait_minimal_ready(&mut harness);

    harness
        .inject_keys(b"\x03\x03")
        .expect("Ctrl+C must stay local");
    harness.update(Duration::from_millis(100));
    assert!(!harness.screen_contents().contains("again to quit"));

    harness.inject_keys(b"\x04").expect("arm Ctrl+D quit");
    harness
        .wait_for_text("again to quit", Duration::from_secs(5))
        .unwrap_or_else(|e| {
            panic!(
                "quit-confirmation hint expected after first Ctrl+D: {e}\nscreen:\n{}",
                harness.screen_contents()
            )
        });

    // The screen wait pumps PTY output and may itself outlive Ctrl+D's
    // one-second confirmation window on a loaded CI runner. Ctrl+C clears
    // that old arm; a single PTY write then keeps the confirming pair within
    // the product's existing window.
    harness
        .inject_keys(b"\x03")
        .expect("Ctrl+C must clear the prior quit confirmation");
    harness
        .wait_for_text_absent("again to quit", Duration::from_secs(2))
        .expect("Ctrl+C clears the old quit confirmation");
    harness
        .inject_keys(b"\x04\x04")
        .expect("send Ctrl+D confirmation pair together");
    let exit = harness
        .wait_exit_code(Duration::from_secs(5))
        .expect("wait for minimal pager exit");
    assert!(
        matches!(exit, PtyExitPoll::Exited(_) | PtyExitPoll::PendingStatus),
        "second Ctrl+D should quit minimal, got {exit:?}\nscreen:\n{}",
        harness.screen_contents()
    );
}
