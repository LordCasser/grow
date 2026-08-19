//! PTY, fully flag-file driven: the model backgrounds a flag-gated command,
//! the test extracts the runtime task id and enqueues a blocking
//! `get_command_or_subagent_output` on it (park), then types mid-park.
//! Plain Enter must keep that message in FIFO until the wait finishes; the
//! park itself writes no transcript marker.
#[allow(unused_imports)]
use super::common::*;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored (see Cargo.toml)"]
async fn parked_enter_queues_until_wait_finishes() {
    let content = ContentController::start().await.expect("start content");
    content.seed_llm_config().expect("seed mock LLM config");
    // Gates the background command the watching cue counts (released at the end).
    let park_flag = content.home().join("endline_park_flag");
    // Gates the id-extraction hold: created once the wait script is enqueued.
    let id_ready_flag = content.home().join("endline_id_ready_flag");

    let gated_loop = |flag: &std::path::Path| {
        format!("while [ ! -e {} ]; do /bin/sleep 0.2; done", flag.display())
    };

    // Tool call 1: the flag-gated background command the watching cue counts.
    let bg_args = json!({
        "command": gated_loop(&park_flag),
        "description": "flag-gated command",
        "is_background": true
    })
    .to_string();
    let _background_turn =
        expect_tool_turn(&content, "call_endline_bg", "run_terminal_command", bg_args);

    // Tool call 2: a flag-gated foreground hold keeps the turn open until the
    // test has extracted the task id and enqueued the wait script.
    let id_hold_args = json!({
        "command": gated_loop(&id_ready_flag),
        "description": "hold for id extraction"
    })
    .to_string();
    let _id_hold_turn = expect_tool_turn(
        &content,
        "call_endline_id_hold",
        "run_terminal_command",
        id_hold_args,
    );

    // Fallback for the post-wait continuation and queued prompt turns.
    content.set_response("ENDLINE_FINAL_ANSWER");

    let binary = pager_binary().expect("resolve pager binary");
    // --permission-mode always-approve skips the bash permission prompt; --trust skips the folder-trust gate.
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        Some(content.home()),
    )
    .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");

    // The runtime task id rides in the tool result of the follow-up request
    // (the same request the id-hold script answers), inside a
    // <task-id>…</task-id> envelope.
    let task_id = poll_for(Duration::from_secs(60), || {
        content
            .request_bodies()
            .iter()
            .find_map(|b| extract_task_id(&b.to_string()))
    })
    .unwrap_or_else(|| {
        panic!(
            "no <task-id> in any request body\n--- non-system messages ---\n{}\n--- screen ---\n{}",
            dump_non_system_messages(&content.request_bodies()),
            harness.screen_contents()
        )
    });

    // Tool call 3: block on the real task id (600s survives the wait cap).
    let wait_args = json!({
        "task_ids": [task_id],
        "timeout_ms": 600_000
    })
    .to_string();
    let _wait_turn = expect_tool_turn(
        &content,
        "call_endline_wait",
        "get_command_or_subagent_output",
        wait_args,
    );

    // Everything downstream is scripted — let the id-extraction hold finish.
    std::fs::write(&id_ready_flag, b"ready").expect("release id-extraction hold");

    harness
        .wait_for_text("1 command still running", Duration::from_secs(90))
        .unwrap_or_else(|_| {
            panic!(
                "parked watching cue never appeared; screen:\n{}\n--- non-system messages ---\n{}",
                harness.screen_contents(),
                dump_non_system_messages(&content.request_bodies())
            )
        });
    harness
        .wait_for_text("Enter queues", Duration::from_secs(30))
        .unwrap_or_else(|_| {
            panic!(
                "parked interrupt cue never appeared; screen:\n{}",
                harness.screen_contents()
            )
        });
    let screen = harness.screen_contents();
    assert!(
        !screen.contains("Worked for"),
        "a park must write no marker; screen:\n{screen}"
    );

    harness
        .inject_keys(b"hurry up please")
        .expect("type mid-park");
    harness.update(Duration::from_millis(300));
    harness.inject_keys(b"\r").expect("submit mid-park prompt");

    harness
        .wait_for_text("1 queued — Enter to send now", Duration::from_secs(30))
        .unwrap_or_else(|_| {
            panic!(
                "mid-park Enter did not remain in FIFO; screen:\n{}",
                harness.screen_contents()
            )
        });
    harness.update(Duration::from_secs(1));
    assert!(
        !harness.contains_text("ENDLINE_FINAL_ANSWER"),
        "queued prompt ran before the blocking wait finished; screen:\n{}",
        harness.screen_contents()
    );

    // Let the current turn finish. Only then may the queued prompt promote.
    std::fs::write(&park_flag, b"done").expect("release flag");
    harness
        .wait_for_text("ENDLINE_FINAL_ANSWER", Duration::from_secs(90))
        .unwrap_or_else(|_| {
            panic!(
                "post-wait/queued turns never settled; screen:\n{}",
                harness.screen_contents()
            )
        });
    harness
        .wait_for_turn_idle(Duration::from_secs(90))
        .expect("queued turn drains after the parked owner completes");

    // The full-screen renderer may discard the previous turn's off-screen
    // rows when the FIFO owner starts, so assert the promoted row's terminal
    // plus the absence of a synthetic cancellation marker. The earlier
    // pre-release assertions prove the park itself produced no terminal.
    let promoted_turn_settled = wait_until(Duration::from_secs(30), || {
        harness.update(Duration::from_millis(100));
        let screen = harness.screen_contents();
        screen.contains("Worked for")
            && !screen.contains("Turn cancelled by user")
            && screen.contains("hurry up please")
    });
    assert!(
        promoted_turn_settled,
        "promoted FIFO row did not settle cleanly after the parked owner completed; screen:\n{}",
        harness.screen_contents()
    );

    write_cast_if_requested(&harness, "endline_park_is_markerless.cast");
}
