//! A normal Pager quit checkpoints an unsent cwd draft for the next run.
#[allow(unused_imports)]
use super::common::*;

const DRAFT: &str = "local-draft-cwd-sentinel";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn local_draft_recovers_after_quit() {
    let content = ContentController::start().await.expect("start content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");

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
        .inject_keys(DRAFT.as_bytes())
        .expect("type unsent draft");
    first
        .wait_for_text(DRAFT, WELCOME_TIMEOUT)
        .expect("draft rendered in composer");
    // Ctrl+Q is a normal, checkpointed quit from the prompt.
    first.update(Duration::from_millis(500));
    first.inject_keys(b"\x11").expect("ctrl-q once");
    first.update(Duration::from_millis(200));
    first.inject_keys(b"\x11").expect("ctrl-q confirm");
    assert_eq!(
        first
            .wait_for_exit_and_drain(Duration::from_secs(10), Duration::from_secs(2))
            .expect("first pager exits after checkpoint"),
        0
    );
    let draft_dir = content.home().join(".grow/pager-drafts-v1");
    let saved = std::fs::read_dir(&draft_dir)
        .expect("draft directory after checkpoint")
        .filter_map(Result::ok)
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap_or_default())
        .any(|record| record.contains(DRAFT));
    assert!(saved, "normal quit must persist the unsent draft");

    let mut resumed = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue"],
        Some(project.path()),
    )
    .expect("spawn pager again in the same cwd");
    resumed
        .wait_for_text(DRAFT, WELCOME_TIMEOUT)
        .expect("unsent draft recovered");
    assert_eq!(
        content.request_count(),
        0,
        "recovering a local draft must not submit it"
    );
    assert!(
        !resumed.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        resumed.screen_contents()
    );
    resumed.quit().expect("quit resumed pager");
}
