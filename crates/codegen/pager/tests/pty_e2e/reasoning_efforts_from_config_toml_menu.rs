// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// The provider model's canonical `reasoning_efforts` contract renders in
/// `/effort`, independent of the inference server's `/models` payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn reasoning_efforts_from_config_toml_menu() {
    let content = ContentController::start_with_models(vec![MockModel::new("grow-4.5")])
        .await
        .expect("start content");
    content.set_response(format!("{MOCK_RESPONSE_SENTINEL} turn."));

    // Seed `~/.grow/config.toml` with a complete provider-model definition.
    let grow_home = content.home().join(".grow");
    std::fs::create_dir_all(&grow_home).expect("create .grow");
    std::fs::write(
        grow_home.join("config.toml"),
        format!(
            r#"[models]
default = "mock/grow-4.5"

[provider.mock]
api_backend = "chat_completions"

[provider.mock.options]
base_url = "{}"

[provider.mock.models.grow-4.5]
context_window = 128000
reasoning_efforts = [{{ value = "high", label = "ConfigHigh" }}]
"#,
            content.url()
        ),
    )
    .expect("write config.toml");

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .expect("turn rendered");

    inject_keys_paced(&mut harness, b"/effort ");
    harness
        .wait_for_text("ConfigHigh", Duration::from_secs(10))
        .expect("config-driven label in /effort dropdown");
    assert!(
        !harness.contains_text("Extended reasoning"),
        "the client must not synthesize undeclared effort rows\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}
