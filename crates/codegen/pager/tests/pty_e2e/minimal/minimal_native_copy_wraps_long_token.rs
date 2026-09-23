// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use crate::common::*;

/// The headless terminal's WRAPLINE flags model native selection: a long
/// unbroken answer token must copy intact after its rendered rows enter real
/// scrollback. A physical-row text dump would always insert newlines and miss
/// this regression.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn minimal_native_copy_wraps_long_token() {
    let token = format!("WRAPSTART{}WRAPEND", "q".repeat(180));
    let mut response = format!("{token}\n\n```text\n");
    for i in 0..90 {
        response.push_str(&format!("copy_probe_row_{i:02}\n"));
    }
    response.push_str("```\n\n[LINKCHECK](https://example.test/minimal-native-link-target)\n");

    let content = ContentController::start().await.expect("start content");
    git2::Repository::init(content.home()).expect("initialize isolated project");
    content.seed_llm_config().expect("seed mock LLM config");
    content.set_response(response);
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_env(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        MINIMAL_ARGS,
        &[("TERM_PROGRAM", "WezTerm")],
    )
    .expect("spawn minimal pager with native OSC 8");
    harness.set_respond_to_queries(true);
    wait_minimal_ready(&mut harness);
    // The generic minimal sentinel can appear before the async session has
    // finished loading; wait for the configured context meter before typing.
    harness
        .wait_for_text("128K (", Duration::from_secs(30))
        .expect("mock session fully loaded");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");

    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && !harness.scrollback_text().contains("WRAPSTART") {
        harness.update(Duration::from_millis(100));
    }
    assert!(
        harness.scrollback_text().contains("WRAPSTART"),
        "head did not reach native scrollback; running={:?}, requests={:?}, chat_completion={}; screen:\n{}\nfull:\n{}\nraw-tail:\n{}",
        harness.is_running(),
        content.requests(),
        content.has_chat_completion(),
        harness.screen_contents(),
        harness.full_text(),
        String::from_utf8_lossy(harness.raw_output())
            .chars()
            .rev()
            .take(2500)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
    let copied = harness.scrollback_copy_text();
    assert!(
        copied.contains(&token),
        "native selection inserted a newline or padding in the token:\n{copied}"
    );
    harness
        .wait_for_full_text("LINKCHECK", Duration::from_secs(30))
        .expect("linked line streamed before OSC 8 assertion");
    let raw = String::from_utf8_lossy(harness.raw_output());
    assert!(
        raw.contains("\x1b]8;") && raw.contains("https://example.test/minimal-native-link-target"),
        "native OSC 8 link target was not emitted: {}",
        osc8_snippets(&raw)
    );
    assert!(!harness.contains_text("panicked"));
    quit_minimal(&mut harness);
}
