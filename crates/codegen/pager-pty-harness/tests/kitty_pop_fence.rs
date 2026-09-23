//! Opt-in PTY checks for the Kitty pop/DA1 teardown boundary.

#![cfg(unix)]

use std::path::Path;
use std::time::{Duration, Instant};

use pager_pty_harness::{ContentController, PtyHarness, keys, pager_binary};

const STEP_TIMEOUT: Duration = Duration::from_secs(20);
const STARTUP_PROBE: &[u8] = b"\x1b[?u\x1b[c";
const STARTUP_REPLY: &[u8] = b"\x1b[?0u\x1b[?62;c";
const KITTY_PUSH: &[u8] = b"\x1b[>3u";
const KITTY_POP: &[u8] = b"\x1b[<1u";
const DA1_QUERY: &[u8] = b"\x1b[c";
const DA1_REPLY: &[u8] = b"\x1b[?62;c";
const LEFTOVER_BEGIN: &[u8] = b"LEFTOVER-BEGIN";
const LEFTOVER_END: &[u8] = b"LEFTOVER-END EXIT=0";
const LEFTOVER_CAPTURE: &str = concat!(
    "command -v stty >/dev/null || exit 99; ",
    "\"$0\" \"$@\"; rc=$?; ",
    "stty -icanon -echo min 0 time 20 || exit 98; ",
    "printf LEFTOVER-BEGIN; cat; ",
    "printf \"LEFTOVER-END EXIT=%s\" \"$rc\"",
);

fn raw_position_after(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(needle.len())
        .position(|part| part == needle)
        .map(|offset| start + offset)
}

fn wait_raw(harness: &mut PtyHarness, start: usize, needle: &[u8]) -> usize {
    let deadline = Instant::now() + STEP_TIMEOUT;
    loop {
        if let Some(at) = raw_position_after(harness.raw_output(), start, needle) {
            return at;
        }
        if Instant::now() >= deadline {
            let raw = harness.raw_output();
            panic!(
                "missing {needle:?} in PTY output; raw tail: {:?}; screen: {}",
                String::from_utf8_lossy(&raw[raw.len().saturating_sub(4096)..]),
                harness.screen_contents()
            );
        }
        harness.update(Duration::from_millis(20));
    }
}

async fn spawn_kitty() -> (ContentController, PtyHarness) {
    let content = ContentController::start()
        .await
        .expect("start content server");
    content.seed_llm_config().expect("seed mock LLM config");
    let binary = pager_binary().expect("grow binary");
    let pager = binary.to_str().expect("pager path UTF-8");
    let mut harness = PtyHarness::spawn_with_content_env(
        Path::new("/bin/sh"),
        50,
        120,
        &content,
        &["-c", LEFTOVER_CAPTURE, pager],
        &[("TERM_PROGRAM", "WezTerm")],
    )
    .expect("spawn grow inside capture shell");
    wait_kitty_startup(&mut harness);
    (content, harness)
}

async fn spawn_kitty_direct() -> (ContentController, PtyHarness) {
    let content = ContentController::start()
        .await
        .expect("start content server");
    content.seed_llm_config().expect("seed mock LLM config");
    let binary = pager_binary().expect("grow binary");
    let mut harness = PtyHarness::spawn_with_content_env(
        &binary,
        50,
        120,
        &content,
        &[],
        &[("TERM_PROGRAM", "WezTerm")],
    )
    .expect("spawn grow directly");
    wait_kitty_startup(&mut harness);
    (content, harness)
}

fn wait_kitty_startup(harness: &mut PtyHarness) {
    let probe = wait_raw(harness, 0, STARTUP_PROBE);
    harness
        .inject_keys(STARTUP_REPLY)
        .expect("answer startup probe");
    let _ = wait_raw(harness, probe, KITTY_PUSH);
    harness
        .wait_for_text("Quit", STEP_TIMEOUT)
        .expect("welcome screen");
}

fn quit(harness: &mut PtyHarness) -> usize {
    let before = harness.raw_output().len();
    harness.inject_keys(keys::CTRL_C).expect("arm quit");
    harness.update(Duration::from_millis(250));
    if harness
        .is_running()
        .expect("poll grow before confirming quit")
    {
        harness.inject_keys(keys::CTRL_C).expect("confirm quit");
    }
    before
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns grow and a scripted PTY"]
async fn late_release_is_consumed_before_shell_resumes() {
    let (_content, mut harness) = spawn_kitty().await;
    let before = quit(&mut harness);
    let query = wait_raw(&mut harness, before, DA1_QUERY);
    let pop = raw_position_after(harness.raw_output(), before, KITTY_POP).expect("kitty pop");
    assert!(pop < query, "pop must precede DA1 query");

    harness.update(Duration::from_millis(100));
    harness.inject_keys(b"\x1b[99;5:3u").expect("late release");
    harness.update(Duration::from_millis(50));
    harness.inject_keys(DA1_REPLY).expect("DA1 reply");

    let begin = wait_raw(&mut harness, query, LEFTOVER_BEGIN);
    harness.inject_keys(b"PROBE-OK").expect("positive control");
    let end = wait_raw(&mut harness, begin, LEFTOVER_END);
    let leftovers = &harness.raw_output()[begin + LEFTOVER_BEGIN.len()..end];
    assert!(raw_position_after(leftovers, 0, b"PROBE-OK").is_some());
    assert!(raw_position_after(&harness.raw_output()[query..end], 0, b":3u").is_none());
    assert_eq!(
        harness
            .wait_for_exit_and_drain(Duration::from_secs(8), Duration::from_secs(2))
            .expect("shell exit"),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns grow and a scripted PTY"]
async fn silent_terminal_does_not_hold_quit() {
    let (_content, mut harness) = spawn_kitty().await;
    let before = quit(&mut harness);
    let query = wait_raw(&mut harness, before, DA1_QUERY);
    let _ = wait_raw(&mut harness, query, LEFTOVER_END);
    assert_eq!(
        harness
            .wait_for_exit_and_drain(Duration::from_secs(8), Duration::from_secs(2))
            .expect("shell exit"),
        0
    );
    assert!(raw_position_after(harness.raw_output(), before, b"\x1b[?25h").is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns grow and a scripted PTY"]
async fn no_kitty_flags_sends_no_teardown_query() {
    let content = ContentController::start()
        .await
        .expect("start content server");
    content.seed_llm_config().expect("seed mock LLM config");
    let binary = pager_binary().expect("grow binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, 50, 120, &content, &[]).expect("spawn grow");
    harness
        .wait_for_text("Quit", STEP_TIMEOUT)
        .expect("welcome screen");
    assert!(raw_position_after(harness.raw_output(), 0, KITTY_PUSH).is_none());
    let before = quit(&mut harness);
    assert_eq!(
        harness
            .wait_for_exit_and_drain(Duration::from_secs(10), Duration::from_secs(2))
            .expect("grow exit"),
        0
    );
    assert!(raw_position_after(harness.raw_output(), before, KITTY_POP).is_none());
    assert!(raw_position_after(harness.raw_output(), before, DA1_QUERY).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns grow and sends a real OS signal"]
async fn first_sigint_uses_normal_fence() {
    let (_content, mut harness) = spawn_kitty_direct().await;
    let before = harness.raw_output().len();
    harness.send_signal(libc::SIGINT).expect("first SIGINT");
    let query = wait_raw(&mut harness, before, DA1_QUERY);
    let pop = raw_position_after(harness.raw_output(), before, KITTY_POP).expect("kitty pop");
    assert!(pop < query, "pop must precede DA1 after SIGINT");
    harness.inject_keys(DA1_REPLY).expect("DA1 reply");
    assert_eq!(
        harness
            .wait_for_exit_and_drain(Duration::from_secs(8), Duration::from_secs(2))
            .expect("grow exit"),
        0
    );
    assert!(raw_position_after(harness.raw_output(), before, b"\x1b[?25h").is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns grow and sends two real OS signals"]
async fn second_sigint_forces_exit_without_another_fence() {
    let (_content, mut harness) = spawn_kitty_direct().await;
    let before = harness.raw_output().len();
    harness.send_signal(libc::SIGINT).expect("first SIGINT");
    let query = wait_raw(&mut harness, before, DA1_QUERY);
    harness.send_signal(libc::SIGINT).expect("second SIGINT");
    assert_eq!(
        harness
            .wait_for_exit_and_drain(Duration::from_secs(8), Duration::from_secs(2))
            .expect("forced grow exit"),
        130
    );
    assert!(raw_position_after(harness.raw_output(), query + DA1_QUERY.len(), DA1_QUERY).is_none());
    assert!(raw_position_after(harness.raw_output(), before, b"\x1b[?25h").is_some());
}
