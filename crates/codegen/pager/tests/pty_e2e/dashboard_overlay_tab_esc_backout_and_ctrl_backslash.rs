// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Ctrl+\ (OpenDashboard). crossterm maps the raw 0x1c byte to Ctrl+4, so the
/// universal dashboard chord must be sent as the kitty CSI-u form: code 92
/// (`\`), modifier 5 (Ctrl). Mirrors `CTRL_ENTER` / `CTRL_SEMICOLON` in common.
const CTRL_BACKSLASH: &[u8] = b"\x1b[92;5u";

/// Attach the (only) agent row as a session overlay from the dashboard list.
/// Down clamps at the last focusable, so three Downs land on the row regardless
/// of the current cursor (fresh open = New session button; after a back-out =
/// previously-selected row); Enter then attaches the peeked row. Waits until the
/// overlay is up: the dashboard list ("+ New session") is gone and the agent's
/// transcript (MOCKRESPONSE) is shown.
fn attach_overlay(h: &mut PtyHarness) {
    for _ in 0..3 {
        h.inject_keys(keys::DOWN).expect("down to row");
        h.update(Duration::from_millis(200));
    }
    h.inject_keys(keys::ENTER).expect("attach row");
    wait_for_labels_absent(h, &["+ New session"], Duration::from_secs(10));
    h.wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(10))
        .expect("attached the original agent as an overlay");
    assert!(
        !h.contains_text("+ New session"),
        "attach must leave the dashboard list for the agent overlay\nscreen:\n{}",
        h.screen_contents()
    );
}

fn saved_session_directories(
    parent: &std::path::Path,
) -> std::collections::BTreeSet<std::path::PathBuf> {
    std::fs::read_dir(parent)
        .expect("session history directory")
        .map(|entry| entry.expect("session directory entry").path())
        .filter(|path| path.join("summary.json").is_file())
        .collect()
}

/// Dashboard-overlay navigation from the default Prompt focus:
///   - **Ctrl+\** opens the dashboard from a session;
///   - **empty-prompt Esc** stays in detail;
///   - **Left on an empty prompt** stays in detail;
///   - **a drafted-prompt Esc** does NOT back out — it arms "press again to clear";
///   - **Tab then Esc** stays in detail;
///   - **Ctrl+\\** explicitly returns to the dashboard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn dashboard_overlay_tab_esc_backout_and_ctrl_backslash() {
    let content = ContentController::start().await.expect("start content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} dashboard overlay turn."));

    let project = tempfile::tempdir().expect("isolated project");
    std::fs::create_dir(project.path().join(".git")).expect("project marker");
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--trust"],
        Some(project.path()),
    )
    .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("turn rendered (idle session)");

    // Ctrl+\ opens the dashboard from inside a session (universal back-out).
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("ctrl+\\ open dashboard");
    harness
        .wait_for_text("+ New session", Duration::from_secs(10))
        .expect("Ctrl+\\ opens the dashboard");

    // Drafted-prompt Esc and empty-prompt Esc both stay in detail.
    attach_overlay(&mut harness);
    let draft = "OVLDRAFT";
    harness.inject_keys(draft.as_bytes()).expect("type draft");
    harness
        .wait_for_text(draft, Duration::from_secs(10))
        .expect("draft renders in the overlay prompt");
    harness.inject_keys(keys::ESC).expect("esc with draft");
    harness.update(Duration::from_millis(300));
    assert!(
        harness.contains_text("press again to clear"),
        "a drafted overlay prompt Esc must arm clear, not back out\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        !harness.contains_text("+ New session"),
        "a drafted overlay prompt Esc must NOT return to the dashboard\nscreen:\n{}",
        harness.screen_contents()
    );
    // Wipe the draft (Ctrl+U clears the line).
    harness.inject_keys(b"\x15").expect("ctrl+u clear draft");
    wait_for_labels_absent(&mut harness, &[draft], Duration::from_secs(5));
    // Empty-prompt Esc remains within detail.
    harness.inject_keys(keys::ESC).expect("empty-prompt esc");
    harness.update(Duration::from_millis(300));
    assert!(!harness.contains_text("+ New session"));
    assert!(harness.contains_text(MOCK_RESPONSE_SENTINEL));

    // Left on an empty prompt is an editing/navigation key, not a back-out.
    harness
        .inject_keys(b"\x1b[D")
        .expect("left on empty prompt");
    harness.update(Duration::from_millis(300));
    assert!(!harness.contains_text("+ New session"));
    assert!(harness.contains_text(MOCK_RESPONSE_SENTINEL));

    // Tab then Esc switches detail focus without returning to the dashboard.
    harness.inject_keys(b"\t").expect("tab to scrollback");
    harness.update(Duration::from_millis(400));
    harness.inject_keys(keys::ESC).expect("tab-pane esc");
    harness.update(Duration::from_millis(300));
    assert!(!harness.contains_text("+ New session"));
    assert!(harness.contains_text(MOCK_RESPONSE_SENTINEL));

    // Ctrl+\ is the explicit dashboard back-out.
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("ctrl+\\ from detail");
    harness
        .wait_for_text("+ New session", Duration::from_secs(10))
        .expect("Ctrl+\\ returns to the dashboard");

    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}

/// Exercise the application input path and real runtime together: command
/// prefix, independent fork, peer cycling, and acknowledged local close.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn dashboard_command_prefix_fork_cycle_and_close() {
    let content = ContentController::start().await.expect("start content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    let project = tempfile::tempdir().expect("isolated project");
    std::fs::create_dir(project.path().join(".git")).expect("project marker");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} original journey turn."));
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        160,
        &content,
        &["--trust"],
        Some(project.path()),
    )
    .expect("spawn pager");
    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("initial prompt");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("initial response");
    harness.update(Duration::from_millis(400));
    let source_history = session_dir(&content, &mut harness);
    let history_root = source_history
        .parent()
        .expect("encoded cwd history directory")
        .to_path_buf();
    let before_fork = saved_session_directories(&history_root);
    harness.inject_keys(CTRL_BACKSLASH).expect("dashboard");
    harness
        .wait_for_text("+ New session", Duration::from_secs(10))
        .expect("dashboard list");
    attach_overlay(&mut harness);
    for (suffix, title) in [
        (b'm', "Pick model"),
        (b'p', "Pick Permission"),
        (b'b', "Pick Behavior"),
    ] {
        harness
            .inject_keys(&[0x18, suffix])
            .expect("command prefix then selector");
        harness
            .wait_for_text(title, Duration::from_secs(10))
            .expect("selector opens inside attached session");
        harness.inject_keys(keys::ESC).expect("close selector");
        wait_for_labels_absent(&mut harness, &[title], Duration::from_secs(5));
        assert!(
            !harness.contains_text("+ New session"),
            "selector must keep attached session"
        );
    }
    inject_keys_paced(&mut harness, b"/fork --no-worktree\r");
    harness
        .wait_for_text("forked from", Duration::from_secs(20))
        .expect("fork finishes replay");
    let first_fork_histories = saved_session_directories(&history_root);
    let closed_fork_history = first_fork_histories
        .difference(&before_fork)
        .next()
        .expect("independent fork history")
        .clone();
    assert_eq!(first_fork_histories.len(), before_fork.len() + 1);
    let fork_position = if harness.contains_text("1/2") {
        "1/2"
    } else {
        "2/2"
    };
    harness
        .wait_for_text(fork_position, Duration::from_secs(10))
        .expect("two independently selectable sessions");
    harness
        .inject_keys(&[0x18, 0x18])
        .expect("repeat Ctrl+X on idle fork");
    harness.update(Duration::from_millis(400));
    assert!(
        harness.contains_text("forked from") && harness.contains_text(fork_position),
        "repeating Ctrl+X leaves an idle fork open\nscreen:\n{}",
        harness.screen_contents()
    );
    let source_position = if fork_position == "1/2" { "2/2" } else { "1/2" };
    harness
        .inject_keys(b"\x1b[91;5u")
        .expect("previous session");
    harness
        .wait_for_text(source_position, Duration::from_secs(10))
        .expect("cycle reaches original");
    harness
        .inject_keys(&[0x18, b'p'])
        .expect("prefix after cycle");
    harness
        .wait_for_text("Pick Permission", Duration::from_secs(10))
        .expect("cycled session accepts prefix");
    harness.inject_keys(keys::ESC).expect("close selector");
    wait_for_labels_absent(&mut harness, &["Pick Permission"], Duration::from_secs(5));
    harness.inject_keys(b"\x1b[93;5u").expect("next session");
    harness
        .wait_for_text(fork_position, Duration::from_secs(10))
        .expect("cycle returns to fork");

    // A hidden selected row remains attachable after Dashboard filtering;
    // Dashboard composer text survives the search and detail round trip.
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("return to Dashboard from fork");
    harness
        .wait_for_text("+ New session", Duration::from_secs(10))
        .expect("Dashboard after fork");
    harness
        .inject_keys(b"\t")
        .expect("focus New session composer");
    let preserved_draft = "PRESERVEDNEWDRAFT";
    inject_keys_paced(&mut harness, preserved_draft.as_bytes());
    harness
        .wait_for_text(preserved_draft, Duration::from_secs(10))
        .expect("Dashboard composer draft");
    harness
        .inject_keys(b"\x1b[47;5u")
        .expect("Ctrl+/ search mode");
    inject_keys_paced(&mut harness, b"s:working");
    harness
        .inject_keys(keys::ENTER)
        .expect("apply state filter");
    harness
        .wait_for_text("No agents in state `Working`", Duration::from_secs(10))
        .expect("state filter hides the idle fork/source rows");
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("reattach selected fork despite filter");
    harness
        .wait_for_text(fork_position, Duration::from_secs(10))
        .expect("filtered-out fork remains the attached session");
    harness
        .inject_keys(b"\x1b[91;5u")
        .expect("cycle from filtered Dashboard selection");
    harness
        .wait_for_text(source_position, Duration::from_secs(10))
        .expect("detail cycle reaches source independent of Dashboard filter");
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("return to Dashboard from source");
    harness
        .wait_for_text("+ New session", Duration::from_secs(10))
        .expect("Dashboard draft remains available");
    assert!(
        harness.contains_text(preserved_draft),
        "Dashboard composer draft must survive search and detail transitions\nscreen:\n{}",
        harness.screen_contents()
    );

    // Return to fork and close it with the explicit Ctrl+X, C command.
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("reattach source");
    harness
        .wait_for_text(source_position, Duration::from_secs(10))
        .expect("source detail");
    harness.inject_keys(b"\x1b[93;5u").expect("cycle to fork");
    harness
        .wait_for_text(fork_position, Duration::from_secs(10))
        .expect("fork detail");
    harness
        .inject_keys(&[0x18, b'c'])
        .expect("Ctrl+X, C closes fork");
    harness
        .wait_for_text("+ New session", Duration::from_secs(10))
        .expect("runtime close returns to Dashboard");
    assert!(
        closed_fork_history.exists(),
        "close preserves saved history"
    );
    assert!(source_history.exists());

    // The overview filter still hides rows. Explicit return reaches the remembered source.
    harness
        .inject_keys(CTRL_BACKSLASH)
        .expect("return to surviving source despite the overview filter");
    wait_for_labels_absent(&mut harness, &["+ New session"], Duration::from_secs(10));
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(10))
        .expect("source survives fork close");
    assert!(
        !harness.contains_text("1/2") && !harness.contains_text("2/2"),
        "closed fork must leave peer navigation\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        !harness.contains_text("forked from"),
        "the attached surviving source is not the retired fork\nscreen:\n{}",
        harness.screen_contents()
    );

    // Create a second independent fork, cancel its delete modal, then confirm.
    inject_keys_paced(&mut harness, b"/fork --no-worktree\r");
    harness
        .wait_for_text("forked from", Duration::from_secs(20))
        .expect("second fork finishes replay");
    let second_fork_histories = saved_session_directories(&history_root);
    let deleted_fork_history = second_fork_histories
        .difference(&first_fork_histories)
        .next()
        .expect("second independent fork history")
        .clone();
    assert_eq!(second_fork_histories.len(), first_fork_histories.len() + 1);
    harness
        .inject_keys(&[0x18, b'd'])
        .expect("Ctrl+X, D opens permanent-delete confirmation");
    harness
        .wait_for_text("Delete session history?", Duration::from_secs(10))
        .expect("delete confirmation modal");
    harness.inject_keys(keys::ESC).expect("cancel delete modal");
    wait_for_labels_absent(
        &mut harness,
        &["Delete session history?"],
        Duration::from_secs(5),
    );
    harness
        .wait_for_text("forked from", Duration::from_secs(10))
        .expect("cancelled fork remains open");
    assert!(
        deleted_fork_history.exists(),
        "cancel does not remove history"
    );
    harness
        .inject_keys(&[0x18, b'd'])
        .expect("reopen permanent-delete confirmation");
    harness
        .wait_for_text("Delete session history?", Duration::from_secs(10))
        .expect("delete confirmation reopened");
    harness.inject_keys(b"y").expect("confirm permanent delete");
    wait_for_labels_absent(
        &mut harness,
        &["Delete session history?", "forked from"],
        Duration::from_secs(10),
    );
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(10))
        .expect("source survives deletion of independent fork");
    assert!(
        !deleted_fork_history.exists(),
        "confirmed deletion removes exactly the target history"
    );
    assert!(source_history.exists(), "source history is independent");
    assert!(
        closed_fork_history.exists(),
        "the earlier closed fork keeps its history"
    );
    assert!(
        !harness.contains_text("1/2") && !harness.contains_text("2/2"),
        "deleted fork must leave peer navigation\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        !harness.contains_text("forked from"),
        "source history must remain after fork deletion\nscreen:\n{}",
        harness.screen_contents()
    );
    harness.quit().expect("clean quit");
}
