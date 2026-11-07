//! Built-binary e2e: SessionEnd hooks fire on headless process exit.
//!
//! Regression for the non-leader quit path that used to cancel the agent
//! without flushing session actors, so SessionEnd never ran on `/exit` /
//! `grow -p` exit.
//!
//! `#[ignore]`d by default — needs the grow binary (`GROW_BINARY` or a local
//! debug build):
//! ```bash
//! cargo test -p shell --test test_session_end_hook_e2e -- --ignored
//! ```
//!
//! CI coverage of the same machinery without a built binary lives in
//! `shell::agent::activity` tests (the flush quiesce loop and its
//! grace expiry) and `pager::acp::spawn` tests (the worker join:
//! clean exit, worker error, panic rendering, and the abandon-at-budget
//! branch this e2e cannot reach).

use test_support::*;

/// Runs headless with a SessionEnd hook that writes stdin + a marker file.
async fn run_with_session_end_hook() -> (HeadlessResult, MockInferenceServer, tempfile::TempDir) {
    let state_dir = tempfile::TempDir::new().expect("create state dir");
    let server = MockInferenceServer::start()
        .await
        .expect("start mock server");
    let mut sandbox = TestSandbox::builder().mock_url(server.url()).git().build();

    let state = state_dir.path().display();
    let script_path = sandbox.home().join("session_end_hook.sh");
    std::fs::write(
        &script_path,
        format!(
            "#!/bin/sh\n\
             cat > {state}/stdin.json\n\
             touch {state}/marker\n\
             exit 0\n"
        ),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod hook script");
    }

    let hooks_dir = sandbox.grow_home().join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    std::fs::write(
        hooks_dir.join("session_end.json"),
        serde_json::json!({
            "hooks": {
                "session_end": [{
                    "hooks": [{
                        "type": "command",
                        "command": format!("sh {}", script_path.display()),
                        "timeout": 30
                    }]
                }]
            }
        })
        .to_string(),
    )
    .expect("write hook config");

    // The sandbox builder only isolates credentials; without an explicit
    // provider config the headless prompt fails with "no LLM configured"
    // before reaching the mock server. Mirror the isolated mock provider
    // shape used by test_subagent_session_end_order.rs.
    let models_dir = sandbox.grow_home();
    std::fs::write(
        models_dir.join("config.toml"),
        format!(
            r#"[models]
default = "mock/headless"
[provider.mock]
api_backend = "chat_completions"
[provider.mock.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.mock.models.headless]
model = "headless-wire"
context_window = 200000
agent_type = "grow-build"
"#,
            server.url()
        ),
    )
    .expect("write mock provider config");

    // Diagnostic trail for the hook-lifecycle path (same shape as
    // test_subagent_session_end_order.rs), so a missing marker can be
    // attributed to a specific session-end lifecycle decision. Kept inside
    // the sandbox tempdir (no shared /tmp) and attached to failure messages.
    let diagnostic_log = state_dir.path().join("grow-session-end-e2e.log");
    sandbox.set_env("GROW_LOG_FILE", &diagnostic_log);

    let mut cmd = tokio::process::Command::new(grow_binary());
    cmd.args([
        "-p",
        "say hello",
        // The regression targets the non-leader exit path; without this flag
        // the default leader mode may keep the session resident and never
        // route the process exit through SessionCommand::Shutdown.
        "--no-leader",
        "--permission-mode",
        "always-approve",
    ])
    .current_dir(sandbox.workspace())
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true);
    let result = run_headless_in_sandbox(cmd, sandbox).await;

    (result, server, state_dir)
}

#[tokio::test]
#[ignore]
async fn session_end_hook_fires_on_headless_exit() {
    let (result, server, state_dir) = run_with_session_end_hook().await;
    assert_headless_success(&result, "session_end hook e2e", Some(&server));

    // Attach the diagnostic trail to failure messages so a missing marker can
    // be attributed to a specific session-end lifecycle decision.
    let diagnostic_log = state_dir.path().join("grow-session-end-e2e.log");
    let diagnostics = std::fs::read_to_string(&diagnostic_log)
        .map(|text| format!("\n--- {diagnostic_log:?} ---\n{text}"))
        .unwrap_or_else(|e| format!("\n(diagnostic log {diagnostic_log:?} unreadable: {e})"));

    let marker = state_dir.path().join("marker");
    assert!(
        marker.is_file(),
        "SessionEnd hook must write a marker on process exit (non-leader flush path); \
         missing {marker:?}. stderr:\n{}{diagnostics}",
        result.stderr
    );

    let stdin_path = state_dir.path().join("stdin.json");
    let text = std::fs::read_to_string(&stdin_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", stdin_path.display()));
    let envelope: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("hook stdin not JSON: {e}\n{text}"));

    let event = envelope["hookEventName"]
        .as_str()
        .unwrap_or_else(|| panic!("hookEventName missing: {envelope}"));
    assert!(
        event == "session_end",
        "expected SessionEnd event name, got {event:?}"
    );

    // `reason` is an already-shipped part of the hook payload that user scripts
    // match on: `shutdown` is emitted by the `SessionCommand::Shutdown` arm
    // (leader auto-update / relaunch today), `channel_closed` by the actor's
    // channel-closed arm. This change adds no new value — it routes non-leader
    // exits through the existing Shutdown command — so renaming `shutdown` to
    // something narrower here would break those scripts. Future distinct causes
    // (e.g. a signal-driven or idle-eviction end) should be added as new values
    // alongside it.
    let reason = envelope["reason"]
        .as_str()
        .unwrap_or_else(|| panic!("reason missing: {envelope}"));
    assert_eq!(
        reason, "shutdown",
        "flush path should send SessionCommand::Shutdown (reason=shutdown), got {reason:?}"
    );
}
