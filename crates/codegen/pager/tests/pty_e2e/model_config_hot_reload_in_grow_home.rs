// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Regression for the in-process startup path: editing the active
/// `GROW_HOME/config.toml` while the pager cwd is that same directory must
/// refresh `/model` without restarting Grow.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn model_config_hot_reload_in_grow_home() {
    let content = ContentController::start()
        .await
        .expect("start content controller");
    content
        .seed_llm_config()
        .expect("seed initial model config");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} turn."));

    let grow_home = content.home().join(".grow");
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        Some(&grow_home),
    )
    .expect("spawn pager in GROW_HOME");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("turn rendered");

    let config_path = grow_home.join("config.toml");
    let mut config = std::fs::read_to_string(&config_path).expect("read initial config");
    config.push_str(
        "\n[provider.mock.models.hot-added]\nname = \"Hot Added\"\ncontext_window = 128000\n",
    );
    std::fs::write(&config_path, config).expect("append model config");

    // The production watcher deliberately debounces editor write/rename
    // bursts for one second; leave enough headroom for the config reload and
    // grow/models/update round-trip before opening the selector.
    tokio::time::sleep(Duration::from_millis(1800)).await;
    inject_keys_paced(&mut harness, b"/model ");
    harness
        .wait_for_text("mock/hot-added", Duration::from_secs(10))
        .expect("hot-added model appears without restarting");

    harness.quit().expect("clean quit");
}
