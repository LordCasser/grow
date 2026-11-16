//! Minimal PageUp browses loaded history on a temporary alternate screen.
//!
//! The recent page is already in the terminal's native scrollback. Older
//! markers must show up between Enter/Leave, not by being inserted back into
//! that native history, and Esc must not print them again.
#[allow(unused_imports)]
use crate::common::*;
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};

const HISTORY_TURNS: usize = 300;
const ARTIFACT_DIR: &str = "/tmp/grow-repair-session-resume-presentation";
const CAST_NAME: &str = "minimal-history-resume-browser.cast";
const ENTER_ALT: &[u8] = b"\x1b[?1049h";
const LEAVE_ALT: &[u8] = b"\x1b[?1049l";
const ONCE_MARKER: &str = "MINHIST_ONCE";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run pty_e2e_minimal with --ignored --test-threads=1"]
async fn minimal_history_resume_browser() {
    let content = ContentController::start()
        .await
        .expect("start isolated mock provider");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    let project = tempfile::tempdir().expect("create project");
    git2::Repository::init(project.path()).expect("init isolated project git");
    let project_cwd = dunce::canonicalize(project.path()).expect("canonical project cwd");
    let grow_home = content.home().join(".grow");
    let spec = SessionSpec {
        turns: HISTORY_TURNS,
        acu_per_turn: 1,
        catalog_commands: 1,
        catalog_desc_len: 16,
        agent_chunks_per_turn: 1,
        agent_chunk_len: 24,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    };
    let (info, _directory) =
        synth::replay::prepare_legacy_session(&grow_home, &project_cwd, &spec).await;
    let route = sampling_types::model_image_input_key_from_parts(
        "mock",
        &sampling_types::ApiBackend::ChatCompletions,
        &content.url(),
        &Default::default(),
    );
    synth::write_admitted_history(
        &info,
        &_directory,
        &spec,
        "mock/mock",
        0,
        Some(("mock", &route)),
    );
    JsonlStorageAdapter::with_root(grow_home.clone())
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("mock/mock"),
            None,
            None,
        )
        .await
        .expect("select mock model for fixture");

    let early = sequence_marker(1);
    let middle = sequence_marker(HISTORY_TURNS / 2);
    let latest = sequence_marker(HISTORY_TURNS);
    let binary = pager_binary().expect("resolve pager binary");
    let mut session = spawn_minimal_resume(&binary, &content, &project_cwd);
    wait_recent(&mut session, &latest);
    expect_pty(
        &session,
        !session.full_text().contains(&early) && !session.full_text().contains(&middle),
        "recent page already printed older fixture markers into native history",
    );
    let requests_before_browse = content.request_count();
    let usage_before_browse = stable_ledger(&mut session, &grow_home, &info).await;
    let recent_native_copies = native_count(session.raw_output(), &latest);
    expect_pty(
        &session,
        recent_native_copies >= 1,
        "recent marker was not printed in the native phase",
    );

    // PageUp from the prompt. Tab is the fullscreen scrollback focus chord and
    // must not be required here.
    browse_to_earliest(&mut session, &early, &middle, true);
    leave_browser(&mut session, &latest);
    assert_native_history_once(&session, &early, &middle, &latest, recent_native_copies);
    expect_pty(
        &session,
        !session.full_text().contains(&early) && !session.full_text().contains(&middle),
        "Esc put older history back into native scrollback",
    );
    let usage_after_browse = stable_ledger(&mut session, &grow_home, &info).await;
    expect_pty(
        &session,
        usage_after_browse == usage_before_browse,
        "browsing recomputed settled usage before any new prompt",
    );
    expect_pty(
        &session,
        content.request_count() == requests_before_browse,
        "browsing issued a model request",
    );

    let mut live_turn = content.expect_agent_turn("one native prompt after browsing", ONCE_MARKER);
    if let Err(error) = session.inject_keys(b"browse once\r") {
        fail(&session, &format!("prompt injection failed: {error}"));
    }
    let rendered = session.wait_until_stable(
        "one native mock marker",
        Duration::from_secs(30),
        Duration::from_millis(250),
        |harness| {
            harness.contains_full_text(ONCE_MARKER)
                && harness.contains_text(MINIMAL_IDLE_SENTINEL)
                && !in_alt(harness.raw_output())
        },
    );
    if let Err(error) = rendered {
        fail(
            &session,
            &format!("mock marker was not committed on the native screen: {error}"),
        );
    }
    expect_pty(
        &session,
        session.full_text().matches(ONCE_MARKER).count() == 1,
        "mock marker was printed more than once on the native screen",
    );
    // Live preview and final native commit may paint the same bytes. The
    // terminal projection above must contain one logical copy; subsequent
    // browsing must not add any more native writes of that committed marker.
    let live_native_copies = native_count(session.raw_output(), ONCE_MARKER);
    expect_pty(
        &session,
        live_native_copies >= 1,
        "mock marker never reached native output",
    );
    assert_native_history_once(&session, &early, &middle, &latest, recent_native_copies);
    live_turn.wait_satisfied().await;
    let usage_after_prompt = stable_ledger(&mut session, &grow_home, &info).await;
    expect_pty(
        &session,
        usage_after_prompt
            .lines()
            .filter(|line| !line.is_empty())
            .count()
            == usage_after_browse
                .lines()
                .filter(|line| !line.is_empty())
                .count()
                + 1,
        "the live prompt must settle exactly one foreground model attempt",
    );
    let requests_after_prompt = content.request_count();

    browse_to_earliest(&mut session, &early, &middle, false);
    expect_pty(
        &session,
        content.request_count() == requests_after_prompt,
        "re-entering the loaded browser issued a model request",
    );
    if let Err(error) = session.inject_keys(b"\x04\x04") {
        fail(&session, &format!("Ctrl+D injection failed: {error}"));
    }
    let exit =
        match session.wait_for_exit_and_drain(Duration::from_secs(20), Duration::from_secs(1)) {
            Ok(code) => code,
            Err(error) => fail(
                &session,
                &format!("Ctrl+D from the history browser did not exit: {error}"),
            ),
        };
    expect_pty(
        &session,
        exit == 0,
        "Ctrl+D from the history browser exited non-zero",
    );
    expect_pty(
        &session,
        contains_sequence(session.raw_output(), LEAVE_ALT) && !ends_on_alt(session.raw_output()),
        "quit left the terminal on the alternate screen",
    );
    expect_pty(
        &session,
        session.full_text().contains(&latest) && session.full_text().contains(ONCE_MARKER),
        "quit dropped the native tail that was already printed",
    );
    expect_pty(
        &session,
        !session.full_text().contains(&early),
        "quit left the earliest marker in native scrollback",
    );
    assert_native_history_once(&session, &early, &middle, &latest, recent_native_copies);
    expect_pty(
        &session,
        native_count(session.raw_output(), ONCE_MARKER) == live_native_copies,
        "quit reprinted the live marker into native output",
    );
    drop(session);

    let mut resumed = spawn_minimal_resume(&binary, &content, &project_cwd);
    let resumed_ready = resumed.wait_until_stable(
        "resumed native tail",
        RESUME_TIMEOUT,
        Duration::from_millis(400),
        |harness| {
            harness.contains_full_text(&latest)
                && harness.contains_full_text(ONCE_MARKER)
                && harness.contains_text(MINIMAL_IDLE_SENTINEL)
                && !harness.contains_text("Loading session")
                && !in_alt(harness.raw_output())
        },
    );
    if let Err(error) = resumed_ready {
        fail(&resumed, &format!("resume lost the session tail: {error}"));
    }
    expect_pty(
        &resumed,
        !resumed.full_text().contains(&early) && !resumed.full_text().contains(&middle),
        "resume printed older history into the native page",
    );
    let requests_before_rebrowse = content.request_count();
    let usage_on_resume = stable_ledger(&mut resumed, &grow_home, &info).await;
    expect_pty(
        &resumed,
        usage_on_resume == usage_after_prompt,
        "resume recomputed settled usage",
    );
    browse_to_earliest(&mut resumed, &early, &middle, true);
    leave_browser(&mut resumed, &latest);
    expect_pty(
        &resumed,
        resumed.contains_full_text(ONCE_MARKER) && !resumed.full_text().contains(&early),
        "the second browse changed the native tail",
    );
    expect_pty(
        &resumed,
        content.request_count() == requests_before_rebrowse,
        "browsing after resume issued a model request",
    );
    let usage_after_rebrowse = stable_ledger(&mut resumed, &grow_home, &info).await;
    expect_pty(
        &resumed,
        usage_after_rebrowse == usage_on_resume,
        "PageUp after resume recomputed settled usage",
    );
    if let Err(error) = resumed.inject_keys(b"\x04\x04") {
        fail(
            &resumed,
            &format!("resume Ctrl+D injection failed: {error}"),
        );
    }
    let exit =
        match resumed.wait_for_exit_and_drain(Duration::from_secs(20), Duration::from_secs(1)) {
            Ok(code) => code,
            Err(error) => fail(&resumed, &format!("resumed session did not exit: {error}")),
        };
    expect_pty(
        &resumed,
        exit == 0,
        "resumed session Ctrl+D exited non-zero",
    );
}

fn spawn_minimal_resume(binary: &Path, content: &ContentController, cwd: &Path) -> PtyHarness {
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        content,
        &["--continue", "--minimal", "--no-leader", "--trust"],
        Some(cwd),
    )
    .expect("spawn minimal resume");
    // The inline viewport's cursor query has to be answered or `--minimal`
    // downgrades before this scenario starts.
    harness.set_respond_to_queries(true);
    harness
}

fn wait_recent(harness: &mut PtyHarness, latest: &str) {
    let ready = harness.wait_until_stable(
        "recent minimal page",
        RESUME_TIMEOUT,
        Duration::from_millis(400),
        |session| {
            session.contains_text(latest)
                && session.contains_text(MINIMAL_IDLE_SENTINEL)
                && !session.contains_text("Loading session")
        },
    );
    if let Err(error) = ready {
        fail(
            harness,
            &format!("recent fixture marker did not settle: {error}"),
        );
    }
}

fn browse_to_earliest(harness: &mut PtyHarness, early: &str, middle: &str, require_middle: bool) {
    if !in_alt(harness.raw_output()) {
        if let Err(error) = harness.inject_keys(keys::PGUP) {
            fail(harness, &format!("PageUp injection failed: {error}"));
        }
        let entered =
            harness.wait_until("temporary alternate screen", WELCOME_TIMEOUT, |session| {
                in_alt(session.raw_output())
            });
        if let Err(error) = entered {
            fail(
                harness,
                &format!("PageUp did not enter the temporary alternate screen: {error}"),
            );
        }
    }
    let deadline = Instant::now() + RESUME_TIMEOUT;
    let mut saw_middle = false;
    while Instant::now() < deadline {
        saw_middle |= harness.contains_text(middle);
        if harness.contains_text(early) {
            // A differential paint may only emit the changed digits of a
            // marker. The terminal's current alternate-screen projection is
            // the content evidence; raw substring presence is not.
            if in_alt(harness.raw_output()) && harness.contains_text("History") {
                expect_pty(
                    harness,
                    !require_middle || saw_middle,
                    "fresh history browsing never displayed the middle marker",
                );
                return;
            }
            fail(
                harness,
                "the earliest marker became visible outside the temporary alternate screen",
            );
        }
        if !in_alt(harness.raw_output()) {
            fail(
                harness,
                "the history browser left the alternate screen before the earliest marker",
            );
        }
        if let Err(error) = harness.inject_keys(keys::PGUP) {
            fail(harness, &format!("PageUp injection failed: {error}"));
        }
        harness.update(Duration::from_millis(40));
    }
    fail(
        harness,
        "PageUp never reached the earliest marker on the alternate screen",
    );
}

fn leave_browser(harness: &mut PtyHarness, latest: &str) {
    if let Err(error) = harness.inject_keys(keys::ESC) {
        fail(harness, &format!("Esc injection failed: {error}"));
    }
    let left = harness.wait_until(
        "native prompt after history Esc",
        WELCOME_TIMEOUT,
        |session| {
            !in_alt(session.raw_output())
                && session.contains_text(MINIMAL_IDLE_SENTINEL)
                && session.contains_full_text(latest)
        },
    );
    if let Err(error) = left {
        fail(
            harness,
            &format!("Esc did not restore the native prompt: {error}"),
        );
    }
}

fn assert_native_history_once(
    harness: &PtyHarness,
    early: &str,
    middle: &str,
    latest: &str,
    recent_copies: usize,
) {
    let raw = harness.raw_output();
    expect_pty(
        harness,
        native_count(raw, early) == 0 && native_count(raw, middle) == 0,
        "native output reprinted older history that belongs only on the alternate screen",
    );
    expect_pty(
        harness,
        native_count(raw, latest) == recent_copies,
        "native output reprinted the recent page after entering or leaving history",
    );
}

fn sequence_marker(turn: usize) -> String {
    format!("GROW_SEQ_{turn:06}")
}

fn in_alt(raw: &[u8]) -> bool {
    ends_on_alt(raw)
}

fn ends_on_alt(raw: &[u8]) -> bool {
    match (last_sequence(raw, ENTER_ALT), last_sequence(raw, LEAVE_ALT)) {
        (Some(enter), Some(leave)) => enter > leave,
        (Some(_), None) => true,
        _ => false,
    }
}

fn contains_sequence(raw: &[u8], needle: &[u8]) -> bool {
    raw.windows(needle.len()).any(|window| window == needle)
}

fn last_sequence(raw: &[u8], needle: &[u8]) -> Option<usize> {
    raw.windows(needle.len())
        .rposition(|window| window == needle)
}

fn native_count(raw: &[u8], marker: &str) -> usize {
    segments(raw, false)
        .iter()
        .map(|segment| String::from_utf8_lossy(segment).matches(marker).count())
        .sum()
}

fn segments(raw: &[u8], want_alt: bool) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut alt = false;
    let mut start = 0;
    let mut index = 0;
    while index < raw.len() {
        if raw[index..].starts_with(ENTER_ALT) {
            if alt == want_alt {
                found.push(&raw[start..index]);
            }
            index += ENTER_ALT.len();
            start = index;
            alt = true;
        } else if raw[index..].starts_with(LEAVE_ALT) {
            if alt == want_alt {
                found.push(&raw[start..index]);
            }
            index += LEAVE_ALT.len();
            start = index;
            alt = false;
        } else {
            index += 1;
        }
    }
    if alt == want_alt {
        found.push(&raw[start..]);
    }
    found
}

async fn stable_ledger(
    harness: &mut PtyHarness,
    grow_home: &Path,
    info: &shell::session::info::Info,
) -> String {
    let deadline = Instant::now() + WELCOME_TIMEOUT;
    let mut previous = None;
    loop {
        let ledger = usage_ledger(grow_home, info).await;
        if previous.as_ref() == Some(&ledger) {
            return ledger;
        }
        previous = Some(ledger);
        if Instant::now() >= deadline {
            fail(harness, "settled usage did not settle");
        }
        harness.update(Duration::from_millis(100));
    }
}

async fn usage_ledger(grow_home: &Path, info: &shell::session::info::Info) -> String {
    let data = JsonlStorageAdapter::with_root(grow_home.to_path_buf())
        .load_session(info)
        .await
        .expect("load verified session");
    data.timeline_events
        .iter()
        .filter_map(|event| {
            let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
                return None;
            };
            if observation.scope != "sampling_usage" || observation.name != "attempt_settled" {
                return None;
            }
            Some(
                observation
                    .data
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn artifact_path(name: &str) -> PathBuf {
    std::fs::create_dir_all(ARTIFACT_DIR).expect("create artifact dir");
    Path::new(ARTIFACT_DIR).join(name)
}

fn expect_pty(harness: &PtyHarness, cond: bool, message: &str) {
    if !cond {
        fail(harness, message);
    }
}

fn fail(harness: &PtyHarness, message: &str) -> ! {
    let path = artifact_path(CAST_NAME);
    let write_error = harness
        .write_cast(&path)
        .err()
        .map(|error| format!(" (cast write failed: {error})"));
    panic!(
        "{message}\ncast: {}{}\nscreen:\n{}\nfull:\n{}",
        path.display(),
        write_error.unwrap_or_default(),
        harness.screen_contents(),
        harness.full_text()
    );
}
