// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// **Edit-interject of the lone queued row must not brick the TUI.**
///
/// Regression e2e for the orphaned invisible `EditConfirm`: interjecting
/// (Ctrl+Enter) a DIRTY edit of a LOCAL queue row that is also the LAST
/// visible row empties the queue mid-flow — the pane auto-hide used to run
/// its pane switch while still in `EditingQueued`, arming a confirm modal
/// that never renders but consumes every subsequent key. The interjection
/// itself still went out (toast + drain both looked healthy), so only an
/// after-the-fact liveness probe catches the brick: on a broken binary the
/// probe text never echoes and step 7 times out.
///
/// The queued message carries a pasted image (as in the original report) —
/// that is also what forces it onto the LOCAL queue: mid-turn plain text
/// takes the server-authoritative immediate-send path instead
/// (`immediate_server_send_eligible`), which never hits the fixed code.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn edit_interject_lone_queued_row_keeps_tui_alive() {
    let content = ContentController::start().await.expect("start content");
    content.set_chunk_delay(Some(Duration::from_millis(150)));
    // Turn 1 must stay open long enough for the ENTIRE mid-turn setup — type
    // the queued text, paste the image path, queue it, focus the queue pane,
    // enter edit, dirty it, Ctrl+Enter — to land WHILE it is still streaming.
    // Only then does the edit-interject drain into turn 1 (→ STEPTWO). Under
    // the ~60-way-parallel suite a short stream can collapse before the
    // interject lands (same flake class the lifecycle test documents), so
    // stream ~150 tokens (~22s) while STEPTWO still drains well inside its
    // 40s wait below.
    let step_one = {
        let mut s = String::from("STEPONE");
        for i in 0..150 {
            s.push_str(&format!(" streaming{i}"));
        }
        s
    };
    let _turn_one = content.expect_agent_turn("running turn before edited interjection", step_one);
    let _turn_two = content.expect_agent_turn(
        "edited interjection continuation",
        "STEPTWO interjection acknowledged.",
    );
    let _turn_three = content.expect_agent_turn(
        "post-interjection liveness prompt",
        "STEPTHREE liveness prompt handled.",
    );

    // Image fixture under the isolated HOME; the pasted absolute path becomes
    // an `[Image #1]` composer chip (path-paste detection reads + decodes it).
    let png_path = content.home().join("queue-edit-fixture.png");
    std::fs::write(&png_path, PNG_32X32_GRAY).expect("write png fixture");

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
        .wait_for_text("STEPONE", Duration::from_secs(30))
        .expect("step 1: turn streaming");

    // Queue ONE image-bearing message → the lone LOCAL queue row. The prose
    // and the path must reach the pager as SEPARATE events: back-to-back
    // injects can land in one EventStream batch and get paste-coalesced,
    // merging the prose into the paste payload — and the paste pipeline is
    // whole-paste-or-nothing (`try_read_dropped_paths`), so a mixed
    // prose+path payload falls back to plain text instead of a chip. Gate
    // on the typed prose rendering before pasting the bare path alone.
    harness
        .inject_keys(b"brick repro payload ")
        .expect("type queued text");
    harness
        .wait_for_text("brick repro payload", Duration::from_secs(10))
        .expect("step 2a: typed prose echoed before the paste");
    harness
        .inject_keys(format!("\x1b[200~{}\x1b[201~", png_path.display()).as_bytes())
        .expect("paste png path");
    harness.update(Duration::from_millis(500));
    harness
        .wait_for_text("[Image #", Duration::from_secs(10))
        .expect("step 2b: image chip attached");
    harness.inject_keys(b"\r").expect("queue the message");
    // The `#1 ` row prefix is queue-pane-only chrome — the composer echo
    // (on screen since step 2a) can never match it, so this proves the
    // Enter actually queued the row.
    harness
        .wait_for_text("#1 brick repro payload", Duration::from_secs(10))
        .expect("step 3: message queued as row #1 in the queue pane");

    // Edit the row (prompt info row flips to "editing queued #1"), dirty it,
    // then force-interject the edit.
    harness
        .inject_keys(CTRL_SEMICOLON)
        .expect("focus queue pane");
    harness.update(Duration::from_millis(300));
    harness.inject_keys(b"e").expect("edit queued row");
    harness
        .wait_for_text("editing queued #1", Duration::from_secs(10))
        .expect("step 4: edit mode entered");
    harness.inject_keys(b"EDITED ").expect("dirty the edit");
    harness.inject_keys(CTRL_ENTER).expect("interject the edit");

    harness
        .wait_for_text("Interjection sent", Duration::from_secs(10))
        .expect("step 5: interjection toast");
    harness
        .wait_for_text("STEPTWO", Duration::from_secs(40))
        .expect("step 6: interjection drained into turn 1");

    // THE regression assertion — liveness probe. Both prior steps pass on a
    // broken binary too (the interjection dispatch is not gated by the agent
    // modal); what breaks is everything AFTER: the orphaned EditConfirm eats
    // all input. Space first: the queue-pane auto-hide left focus on the
    // scrollback, where Space focuses the prompt (on a broken binary the
    // modal eats the Space as well, so the probe below never echoes).
    harness
        .inject_keys(b" ")
        .expect("focus prompt from scrollback");
    harness.update(Duration::from_millis(200));
    harness
        .inject_keys(b"liveness-probe-xyz")
        .expect("type liveness probe");
    harness
        .wait_for_text("liveness-probe-xyz", Duration::from_secs(10))
        .expect("step 7: typed input echoes — an orphaned EditConfirm would eat it");
    harness.inject_keys(b"\r").expect("submit liveness probe");
    harness
        .wait_for_text("STEPTHREE", Duration::from_secs(40))
        .expect("step 8: liveness prompt round-trips input, dispatch, and wire");

    // Wire checks: final request's user_query sequence is exactly
    // [prompt, edited interjection (with wire prefix), liveness probe].
    let bodies = content.request_bodies();
    let last = bodies.last().expect("final request recorded");
    // User-role context preambles (user_info, skill reminders) don't carry
    // <user_query>; real prompts and interjections do. Content is a plain
    // string OR a parts array (the image-bearing interjection), so extract
    // text parts-aware instead of `as_str`.
    let finals: Vec<String> = last["messages"]
        .as_array()
        .expect("messages array")
        .iter()
        .filter(|m| m["role"] == "user")
        .map(message_text)
        .filter(|c| c.contains("<user_query>"))
        .collect();
    assert_eq!(3, finals.len(), "expected 3 user messages: {finals:#?}");
    assert!(finals[0].contains(PROMPT), "first: {finals:#?}");
    assert!(
        finals[1].contains("EDITED")
            && finals[1].contains("brick repro payload")
            && finals[1].contains(INTERJECTION_WIRE_PREFIX),
        "second must be the EDITED interjection: {finals:#?}"
    );
    assert!(
        finals[2].contains("liveness-probe-xyz"),
        "third: {finals:#?}"
    );
    // The queued row's stored image must ride the interjection to the wire.
    let interjection_bodies: Vec<&serde_json::Value> = bodies
        .iter()
        .filter(|b| b.to_string().contains("EDITED"))
        .collect();
    assert!(
        !interjection_bodies.is_empty(),
        "no request carried the edited interjection"
    );
    assert!(
        interjection_bodies.iter().any(|b| contains_image_part(b)),
        "queued row's image never reached the wire: {interjection_bodies:#?}"
    );

    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );
    harness.quit().expect("clean quit");
}

/// An explicit Enter promotion and a local image steer wait behind the same
/// foreground tool. Cancelling the tool preserves both inputs in FIFO order.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_queue test with --ignored"]
async fn cancelling_blocked_foreground_displays_each_pending_steer_once() {
    const SHARED: &str = "shared steering probe";
    const LOCAL: &str = "local image steering probe";
    let content = ContentController::start().await.expect("start content");
    git2::Repository::init(content.home()).expect("initialize isolated project");
    content.seed_llm_config().expect("seed mock model");
    let config = content.home().join(".grow/config.toml");
    let mut settings = std::fs::read_to_string(&config).unwrap();
    settings.push_str("\n[ui]\npage_flip_on_send = false\nfollow_up_behavior = \"queue\"\n");
    std::fs::write(config, settings).unwrap();

    let started = content.home().join("foreground-started");
    let command = format!(
        "printf started > '{}'; /bin/sleep 90",
        started.to_string_lossy().replace('\'', "'\\''")
    );
    let _tool = expect_tool_turn(
        &content,
        "call_steering_block",
        "run_terminal_command",
        json!({
            "command": command,
            "description": "hold foreground for steering",
            "timeout": 90_000
        })
        .to_string(),
    );
    let _shared = content.expect_agent_turn("shared FIFO delivery", "SHARED_AFTER_CANCEL");
    let _local = content.expect_agent_turn("image FIFO delivery", "LOCAL_AFTER_CANCEL");
    let png_path = content.home().join("steering-attachment.png");
    std::fs::write(&png_path, PNG_32X32_GRAY).unwrap();

    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &pager_binary().unwrap(),
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        Some(content.home()),
    )
    .expect("spawn pager");
    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .unwrap();
    harness.inject_keys(b"s").unwrap();
    harness
        .wait_until_stable(
            "ready composer after startup",
            WELCOME_TIMEOUT,
            Duration::from_millis(100),
            |harness| harness.contains_text("grow · Mock") && harness.contains_text("❯ s"),
        )
        .unwrap();
    harness.inject_keys(b"tart the foreground hold\r").unwrap();
    assert!(
        poll_for(Duration::from_secs(30), || {
            harness.update(Duration::from_millis(50));
            started.exists().then_some(())
        })
        .is_some(),
        "foreground tool never started\nscreen:\n{}\nrequests:\n{}",
        harness.screen_contents(),
        dump_non_system_messages(&content.request_bodies())
    );
    let timeline_path = session_dir(&content, &mut harness).join("timeline.jsonl");

    harness
        .inject_keys(format!("{SHARED}\r").as_bytes())
        .unwrap();
    harness
        .wait_for_text(&format!("#1 {SHARED}"), Duration::from_secs(10))
        .unwrap();
    harness
        .inject_keys(b"\r")
        .expect("promote shared queue row");
    wait_for_pending_steers(&mut harness, &timeline_path, 1);

    harness.inject_keys(format!("{LOCAL} ").as_bytes()).unwrap();
    harness
        .wait_for_text(LOCAL, Duration::from_secs(10))
        .unwrap();
    harness
        .inject_keys(format!("\x1b[200~{}\x1b[201~", png_path.display()).as_bytes())
        .unwrap();
    harness
        .wait_for_text("[Image #", Duration::from_secs(10))
        .unwrap();
    harness.inject_keys(b"\r").unwrap();
    harness
        .wait_for_text(&format!("#1 {LOCAL}"), Duration::from_secs(10))
        .unwrap();
    harness
        .inject_keys(b"\r")
        .expect("promote local image queue row");
    wait_for_pending_steers(&mut harness, &timeline_path, 2);

    harness
        .inject_keys(keys::CTRL_C)
        .expect("cancel the blocked foreground");
    harness
        .wait_for_text("LOCAL_AFTER_CANCEL", Duration::from_secs(30))
        .unwrap();
    let bodies = content.request_bodies();
    let final_users = bodies.last().unwrap()["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .collect::<Vec<_>>();
    for text in [SHARED, LOCAL] {
        let delivered = final_users
            .iter()
            .filter(|message| message_text(message).contains(text))
            .collect::<Vec<_>>();
        assert_eq!(delivered.len(), 1, "{text} reaches the model once");
        if text == LOCAL {
            assert!(
                contains_image_part(delivered[0]),
                "the local steer keeps its attachment"
            );
        }
    }
    assert_eq!(
        [SHARED, LOCAL].map(|text| block_lines_containing(&harness, text)),
        [1, 1],
        "each pending steer must render once after cancellation\nscreen:\n{}",
        harness.screen_contents()
    );
    assert!(
        [SHARED, LOCAL]
            .into_iter()
            .all(|text| !composer_holds(&harness, text)),
        "an admitted steer must not be restored into the composer\nscreen:\n{}",
        harness.screen_contents()
    );
    harness.quit().expect("clean quit");
}

#[cfg(unix)]
fn wait_for_pending_steers(harness: &mut PtyHarness, timeline: &Path, count: usize) {
    use chat_state::{InputEvent, InputRoute, TimelineEventKind};
    assert!(
        poll_for(Duration::from_secs(10), || {
            harness.update(Duration::from_millis(50));
            let contents = std::fs::read_to_string(timeline).ok()?;
            let timeline = shell::session::testkit::decode_timeline_fixture(&contents).ok()?;
            let steers = timeline
                .events()
                .iter()
                .filter(|event| {
                    matches!(
                        &event.kind,
                        TimelineEventKind::Input(
                            InputEvent::Rerouted {
                                route: InputRoute::Steer { .. },
                                ..
                            } | InputEvent::AdmissionResolved {
                                route: Some(InputRoute::Steer { .. }),
                                ..
                            }
                        )
                    )
                })
                .count();
            (steers == count).then_some(())
        })
        .is_some(),
        "expected {count} accepted pending steers\nscreen:\n{}",
        harness.screen_contents()
    );
}

/// A user message's text: plain-string content verbatim, parts-array content
/// (image-bearing messages) as the joined `text` parts.
fn message_text(m: &serde_json::Value) -> String {
    match &m["content"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Whether any nested object is an image content part — same shape the
/// harness's scenario assertions accept: a `type` containing "image"
/// (`image_url`, `input_image`, …) or inline `mime_type` + `data`.
fn contains_image_part(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            let is_image = map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|ty| ty.contains("image"))
                || (map.contains_key("mime_type") && map.contains_key("data"));
            is_image || map.values().any(contains_image_part)
        }
        serde_json::Value::Array(values) => values.iter().any(contains_image_part),
        _ => false,
    }
}

/// Valid 32×32 8-bit grayscale PNG (signature + IHDR + IDAT + IEND, CRCs
/// correct, IDAT zlib round-trips). Hardcoded rather than encoded via the
/// `image` dep so the fixture is byte-stable and encoder-independent.
/// 32×32 = 1024 total pixels clears the API/client 512-total-pixel floor
/// (`MIN_VISION_TOTAL_PX`) so the image rides the interjection to the wire
/// instead of being replaced by an `image_dropped_notice`.
const PNG_32X32_GRAY: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x08, 0x00, 0x00, 0x00, 0x00, 0x56, 0x11, 0x25,
    0x28, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x68, 0x20, 0x00, 0x18,
    0x46, 0x15, 0x8c, 0x2a, 0x18, 0x55, 0x30, 0x52, 0x15, 0x00, 0x00, 0x42, 0x00, 0x00, 0x1f, 0x37,
    0x97, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];
