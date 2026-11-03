//! Full and minimal Pager command journeys for `/recap` and `/flush`.
//!
//! Providers are hermetic loopback mocks. No host clipboard or real image
//! source is used by this command-surface test.

#![cfg(unix)]

#[allow(dead_code, unused_imports)]
#[path = "pty_e2e/common.rs"]
mod common;

use acp_transport::protocol as acp;
use base64::Engine as _;
use common::*;
use test_support::{
    GrowStdioClient, InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer, TestSandbox,
};

fn wire_image_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(object) => {
            usize::from(
                object.get("type").and_then(serde_json::Value::as_str) == Some("image")
                    || object.contains_key("image_url"),
            ) + object.values().map(wire_image_count).sum::<usize>()
        }
        serde_json::Value::Array(values) => values.iter().map(wire_image_count).sum(),
        _ => 0,
    }
}

fn valid_png_base64() -> String {
    let image = image::ImageBuffer::from_pixel(32, 32, image::Rgba([128, 64, 32, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode valid test PNG");
    base64::engine::general_purpose::STANDARD.encode(png.into_inner())
}

fn persistent_workspace() -> tempfile::TempDir {
    let parent = std::env::current_dir()
        .expect("read workspace root")
        .join("target/auxiliary-journey-workspaces");
    std::fs::create_dir_all(&parent).expect("create ignored test workspace root");
    tempfile::Builder::new()
        .prefix("grow-pager-auxiliary-")
        .tempdir_in(parent)
        .expect("create isolated non-ephemeral workspace")
}

fn messages_text_turn(text: &str, model: &str) -> ScriptedResponse {
    ScriptedResponse::sse(vec![
        SseEvent::data(json!({
            "type":"message_start",
            "message":{"id":"focus-image-recap","type":"message","role":"assistant","content":[],"model":model,"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}
        }).to_string()),
        SseEvent::data(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}).to_string()),
        SseEvent::data(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}).to_string()),
        SseEvent::data(json!({"type":"content_block_stop","index":0}).to_string()),
        SseEvent::data(json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":10}}).to_string()),
        SseEvent::data(json!({"type":"message_stop"}).to_string()),
    ])
}

fn configure_image_session(sandbox: &TestSandbox, server: &MockInferenceServer) {
    let config = format!(
        r#"[models]
        default = "mock/vision"
        image_description = "mock/vision"

        [provider.mock.options]
        base_url = {:?}
        env_key = "GROW_API_KEY"

        [provider.mock.models.vision]
        model = "vision"
        api_backend = "messages"
        context_window = 200000
        agent_type = "grow-build"

        [provider.mock.models.text-auto]
        model = "text-auto"
        api_backend = "messages"
        context_window = 200000
        agent_type = "grow-build"

        [ui.notifications]
        session_recap = true
        session_recap_threshold_secs = 180
        "#,
        server.url()
    );
    std::fs::write(sandbox.grow_home().join("config.toml"), config).unwrap();
}

fn files_below(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_below(&path));
        } else if path.is_file() {
            files.push(path);
        }
    }
    files
}

fn configure_memory(content: &ContentController) {
    content.seed_llm_config().expect("seed provider config");
    let path = content.home().join(".grow").join("config.toml");
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(b"\n[memory]\nenabled = true\n")
        .unwrap();
}

async fn run_auxiliary_commands(args: &[&str]) {
    let content = ContentController::start()
        .await
        .expect("start loopback provider");
    configure_memory(&content);
    let binary = pager_binary().expect("resolve pager binary");
    let workspace = persistent_workspace();
    let cwd = workspace.path();
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        args,
        Some(cwd),
    )
    .expect("spawn Pager");
    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome screen");

    content.set_response(format!(
        "{MOCK_RESPONSE_SENTINEL} initial user turn gives recap and flush real source history."
    ));
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("send the real foreground prompt");
    harness
        .wait_for_text(MOCK_RESPONSE_SENTINEL, Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("initial response missing:\n{}", harness.screen_contents()));
    harness
        .wait_for_turn_idle(Duration::from_secs(15))
        .expect("initial foreground turn is idle before /recap");

    // Pager's command router dispatches the registered `grow/recap` extension.
    content.set_response("The session context is retained as RECAP_IMAGE_ROUTE_EVIDENCE.");
    let before_recap = content.requests().len();
    harness.inject_keys(b"/recap\r").expect("submit /recap");
    harness
        .wait_for_text("RECAP_IMAGE_ROUTE_EVIDENCE", Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("recap response missing:\n{}", harness.screen_contents()));
    harness
        .wait_for_turn_idle(Duration::from_secs(15))
        .expect("recap turn is idle before /flush");
    assert!(
        content.requests().len() > before_recap,
        "/recap must make a real provider request"
    );
    assert!(
        content.requests()[before_recap as usize..]
            .iter()
            .any(|request| request.body.as_ref().is_some_and(|body| body
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
                < 2)),
        "/recap must use the sideband provider path, not create a foreground turn"
    );

    // The slash command reaches the same host flush executor and writes a real
    // Markdown memory log under this isolated GROW_HOME.
    content.set_response(
        "## Verified Evidence\n\nThe route kept image evidence as RECAP_IMAGE_ROUTE_EVIDENCE.",
    );
    let before_flush = content.requests().len();
    harness.inject_keys(b"/flush\r").expect("submit /flush");
    let deadline = Instant::now() + Duration::from_secs(25);
    let memory_root = content.home().join(".grow").join("memory");
    let memory_file = loop {
        if let Some(path) = files_below(&memory_root).into_iter().find(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "sessions")
                && std::fs::read_to_string(path)
                    .is_ok_and(|memory| memory.contains("RECAP_IMAGE_ROUTE_EVIDENCE"))
        }) {
            break path;
        }
        assert!(
            Instant::now() < deadline,
            "/flush did not write a session daily log; provider requests since command: {}; screen:\n{}",
            content.requests().len().saturating_sub(before_flush),
            harness.screen_contents()
        );
        harness.update(Duration::from_millis(100));
    };
    let memory = std::fs::read_to_string(&memory_file).unwrap();
    assert!(
        memory.contains("RECAP_IMAGE_ROUTE_EVIDENCE"),
        "memory file must contain the mock flush result: {}\n{}",
        memory,
        memory_file.display()
    );
    assert!(
        content.requests()[before_flush as usize..]
            .iter()
            .any(|request| request.body.as_ref().is_some_and(|body| body
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
                < 2)),
        "/flush must use its auxiliary provider request"
    );
    assert!(
        !harness.contains_text("panicked"),
        "Pager panicked:\n{}",
        harness.screen_contents()
    );
    harness.quit().expect("quit Pager cleanly");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; requires freshly built pager and grow binaries"]
async fn full_and_minimal_recap_and_flush_use_real_auxiliary_commands() {
    run_auxiliary_commands(&[]).await;
    run_auxiliary_commands(MINIMAL_ARGS).await;
}

async fn focus_return_recaps_unresolved_image_after_text_model_switch(args: &[&str]) {
    let content = ContentController::start()
        .await
        .expect("start loopback provider");
    content.server().set_settings(json!({
        "session_recap":true,
        "image_description_model":"mock/vision"
    }));
    let workspace_owner = persistent_workspace();
    let sandbox = TestSandbox::new();
    configure_image_session(&sandbox, content.server());
    let workspace = workspace_owner.path().to_path_buf();
    let provider = content.server();
    let mut client = GrowStdioClient::spawn_with_sandbox(provider, &workspace, sandbox).await;
    client.initialize_with_timeout().await;
    let session = client
        .create_session_with_model_timeout(&workspace, "mock/vision")
        .await;

    let first_turn = provider.expect_response(
        "vision primary accepts unresolved screenshot",
        InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
        messages_text_turn("The screenshot is a build error dialog with E42.", "vision"),
    );
    client
        .prompt_with_content_timeout(
            &session,
            vec![
                acp::ContentBlock::Text(acp::TextContent::new("Inspect this screenshot.")),
                acp::ContentBlock::Image(acp::ImageContent::new(valid_png_base64(), "image/png")),
            ],
        )
        .await
        .unwrap();
    first_turn.assert_satisfied();
    for (prompt, response) in [
        ("Second completed turn.", "A second completed turn."),
        ("Third completed turn.", "A third completed turn."),
    ] {
        let turn = provider.expect_response(
            "multimodal route records recap eligibility turns",
            InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
            messages_text_turn(response, "vision"),
        );
        client.prompt_with_timeout(&session, prompt).await.unwrap();
        turn.assert_satisfied();
    }
    client
        .set_model_with_timeout(&session, "mock/text-auto")
        .await
        .unwrap();
    let grow_home = client.sandbox().grow_home().to_path_buf();
    client.close().await.unwrap();

    // Resume the same durable session through the full Pager, with its real
    // GROW_HOME redirected to the ACP-created test session tree.
    let grow_home_env = grow_home.to_str().unwrap().to_owned();
    let overrides = [("GROW_HOME".to_owned(), grow_home_env)];
    let env = overrides
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let binary = pager_binary().expect("resolve pager binary");
    let mut resume_args = args.to_vec();
    resume_args.push("--continue");
    let mut harness = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &resume_args,
        &env,
        Some(&workspace),
    )
    .expect("resume image-bearing session in Pager");
    harness
        .wait_until_stable(
            "ready image-bearing resumed history",
            RESUME_TIMEOUT,
            Duration::from_millis(500),
            |screen| {
                screen.contains_text("Third completed turn.")
                    && screen.contains_text("grow · text-auto")
                    && !screen.contains_text("Loading session")
            },
        )
        .expect("real resumed session history is rendered and input is ready");
    let mut rejected = provider.expect_response(
        "automatic recap on text route first sees the unresolved image",
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
        ScriptedResponse::json(400, json!({
            "type":"error",
            "error":{"type":"invalid_request_error","code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}
        })),
    );
    let mut description = provider.expect_response(
        "automatic recap image recovery uses configured vision route",
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
        messages_text_turn("The screenshot displays error E42.", "vision"),
    );
    let mut retry = provider.expect_response(
        "automatic recap retries its frozen request on the text route",
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
        messages_text_turn(
            "The image shows automatic recap evidence AUTO_RECAP_IMAGE_E42.",
            "text-auto",
        ),
    );
    harness
        .inject_keys(b"\x1b[O")
        .expect("real CSI FocusLost event");
    // Let the production away threshold and the shell's three-minute idle
    // gate elapse while the terminal is genuinely unfocused, then return so
    // the FocusGained handler can dispatch the recap.
    harness.update(Duration::from_secs(181));
    harness
        .inject_keys(b"\x1b[I")
        .expect("real CSI FocusGained return event");
    tokio::time::timeout(Duration::from_secs(30), rejected.wait_received())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "auto recap did not reach provider after the real away/idle gate\n{}",
                harness.screen_contents()
            )
        });
    tokio::time::timeout(Duration::from_secs(15), async {
        description.wait_satisfied().await;
        retry.wait_satisfied().await;
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "image recovery did not finish\n{}",
            harness.screen_contents()
        )
    });
    rejected.assert_satisfied();
    description.assert_satisfied();
    retry.assert_satisfied();
    harness
        .wait_for_text("AUTO_RECAP_IMAGE_E42", Duration::from_secs(20))
        .unwrap_or_else(|_| {
            panic!(
                "auto recap did not display after focus return\n{}",
                harness.screen_contents()
            )
        });

    let target_bodies = provider
        .request_bodies()
        .into_iter()
        .filter(|body| body["model"] == "text-auto")
        .collect::<Vec<_>>();
    assert_eq!(target_bodies.len(), 2);
    assert_eq!(wire_image_count(&target_bodies[0]), 1);
    assert_eq!(wire_image_count(&target_bodies[1]), 0);
    assert!(target_bodies[1].to_string().contains("E42"));
    let timeline = shell::session::storage::load_timeline_by_id_at(session.0.as_ref(), &grow_home)
        .unwrap()
        .unwrap();
    assert!(
        !timeline.events().iter().any(|event| matches!(
            &event.kind,
            chat_state::TimelineEventKind::ImageProjection(_)
        )),
        "auto recap recovery does not mutate the primary image Surface"
    );
    assert!(
        timeline.surface().iter().any(|item| {
            sampling_types::conversation::conversation_image_groups(std::slice::from_ref(item))
                .len()
                > 0
        }),
        "primary history still owns the original unprojected image"
    );
    // The Grow display-cache append is acknowledged in the persistence queue
    // before the file is materialized; clean shutdown is the drain barrier.
    harness
        .quit()
        .expect("quit Pager and drain persisted updates");
    let persisted = shell::session::storage::load_session_trace_at(session.0.as_ref(), &grow_home)
        .unwrap()
        .expect("the resumed session remains in canonical storage");
    let recap_spawn = timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Sideband(spawn)
                if spawn.purpose == chat_state::SidebandPurpose::SessionRecap =>
            {
                Some(spawn)
            }
            _ => None,
        })
        .last()
        .expect("focus return records the SessionRecap Sideband in the Timeline");
    let sideband_events = persisted
        .files
        .iter()
        .filter(|file| file.relative_path.starts_with("sidebands"))
        .flat_map(|file| {
            std::str::from_utf8(&file.bytes)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<chat_state::SidebandEvent>(line).unwrap())
                .collect::<Vec<_>>()
        })
        .filter(|event| event.sideband_id == recap_spawn.sideband_id)
        .collect::<Vec<_>>();
    assert!(
        sideband_events.iter().any(|event| matches!(
            &event.kind,
            chat_state::SidebandEventKind::Request(request)
                if request.purpose == chat_state::SidebandPurpose::SessionRecap
                    && request.source_refs == recap_spawn.source_refs
                    && request.source_refs.iter().all(|source_ref| source_ref.validate().is_ok())
        )),
        "the durable recap Sideband request retains its frozen Timeline sources"
    );
    assert!(
        sideband_events.iter().any(|event| matches!(
            &event.kind,
            chat_state::SidebandEventKind::Result(result)
                if result.raw_output.contains("AUTO_RECAP_IMAGE_E42")
        )),
        "the durable SessionRecap Sideband Result contains the returned marker"
    );
    assert!(
        sideband_events.iter().any(|event| matches!(
            &event.kind,
            chat_state::SidebandEventKind::End(end)
                if end.outcome == chat_state::SidebandOutcome::Completed
        )),
        "the durable SessionRecap Sideband transaction ends Completed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; includes the real three-minute away/idle gate and loopback model calls"]
async fn full_focus_return_recaps_unresolved_image_after_text_model_switch() {
    tokio::task::LocalSet::new()
        .run_until(focus_return_recaps_unresolved_image_after_text_model_switch(&[]))
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; includes the real three-minute away/idle gate and loopback model calls"]
async fn minimal_focus_return_recaps_unresolved_image_after_text_model_switch() {
    tokio::task::LocalSet::new()
        .run_until(focus_return_recaps_unresolved_image_after_text_model_switch(MINIMAL_ARGS))
        .await;
}
