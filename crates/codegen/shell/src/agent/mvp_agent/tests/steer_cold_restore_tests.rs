//! F1 physical cold-restore fixture: an allowed, unconsumed Steer bound to a
//! Turn preserved by a suspended checkpoint survives a real process exit and
//! is rebuilt into the restored actor's interjection buffer, then consumed
//! exactly once at the post-`ExecutionReleased` safe boundary with its
//! original identity, full text and image. Two OS-process phases share a
//! temporary GROW_HOME/HOME; only the provider is mocked (axum). The
//! non-checkpoint interrupted-turn control stays in
//! `session::actor::tests::restore_steer_preserved_tests`.

use super::*;
use acp_transport::AcpAgentHandler as _;
use chat_state::{TimelineEventKind as K, session_runtime::*};

const HOME_ENV: &str = "GROW_TEST_STEER_COLD_HOME";
const PHASE_ENV: &str = "GROW_TEST_STEER_COLD_PHASE";
const TORN_TAIL_ENV: &str = "GROW_TEST_STEER_COLD_TORN_TAIL";
const CRASH_EXIT: i32 = 73;
const STEER_TEXT: &str = "cold-steer: reconsider the restored plan with this image";

fn tiny_png_base64() -> String {
    let image = image::RgbImage::from_pixel(24, 24, image::Rgb([1, 2, 3]));
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(png.into_inner())
}

fn sse_terminal_text(id: &str, content: &str) -> String {
    let delta = serde_json::json!({
        "id": id, "object": "chat.completion.chunk", "created": 1, "model": "default",
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": content}, "finish_reason": null}]
    });
    let done = serde_json::json!({
        "id": id, "object": "chat.completion.chunk", "created": 1, "model": "default",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    format!("data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n")
}

#[test]
fn preserved_steer_survives_crash_with_partial_journal_tail() {
    run_cold_restore_fixture(true);
}

#[test]
fn preserved_steer_survives_cold_restore_and_consumes_once_after_release() {
    run_cold_restore_fixture(false);
}

fn run_cold_restore_fixture(torn_tail: bool) {
    let Ok(home) = std::env::var(HOME_ENV) else {
        let temp = tempfile::tempdir().unwrap();
        for phase in ["suspend", "restore"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::steer_cold_restore_tests::preserved_steer_survives_cold_restore_and_consumes_once_after_release", "--nocapture"])
                .env("HOME", temp.path())
                .env("GROW_HOME", temp.path())
                .env(HOME_ENV, temp.path())
                .env(PHASE_ENV, phase)
                .env(TORN_TAIL_ENV, if torn_tail { "1" } else { "0" })
                .output()
                .unwrap();
            let expected = if torn_tail && phase == "suspend" {
                CRASH_EXIT
            } else {
                0
            };
            assert_eq!(
                output.status.code(),
                Some(expected),
                "{phase}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let torn_tail = std::env::var(TORN_TAIL_ENV).as_deref() == Ok("1");
    run_local_for_bridge_test(|| async move {
        let home = std::path::Path::new(&home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let phase = std::env::var(PHASE_ENV).unwrap();
        let restore = phase == "restore";

        // Provider: ancillary (tools-free) requests answer immediately. The
        // turn's tool-bearing sampling request is the controlled boundary. In
        // the suspend phase its arrival is announced through `arrival` and its
        // response never completes; node suspension cancels the in-flight
        // sampler request, dropping this handler. In the restore phase every
        // request answers with a terminal plain-text response.
        let arrival = Arc::new(tokio::sync::Notify::new());
        let arrival_seen = arrival.clone();
        let recorded = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorded_seen = recorded.clone();
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let arrival = arrival_seen.clone();
                let recorded = recorded_seen.clone();
                let restore = restore;
                async move {
                    let tool_step = body["tools"].as_array().is_some_and(|t| !t.is_empty());
                    if !tool_step {
                        return (
                            [("content-type", "text/event-stream")],
                            sse_terminal_text("ancillary", "meta"),
                        );
                    }
                    recorded
                        .lock()
                        .unwrap()
                        .push(serde_json::to_string(&body).unwrap());
                    if !restore {
                        arrival.notify_one();
                        // Pending forever: only suspension cancellation
                        // ends this held response.
                        std::future::pending::<()>().await;
                    }
                    (
                        [("content-type", "text/event-stream")],
                        sse_terminal_text("restored-final", "restored final answer"),
                    )
                }
            }),
        );
        let bind_address = if restore {
            std::fs::read_to_string(home.join("provider-address")).unwrap()
        } else {
            "127.0.0.1:0".to_owned()
        };
        let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
        let address = listener.local_addr().unwrap();
        if !restore {
            std::fs::write(home.join("provider-address"), address.to_string()).unwrap();
        }
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let raw = if restore {
            std::fs::read_to_string(home.join("config.toml")).unwrap()
        } else {
            format!(
                r#"
[models]
default = "test/default"
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://{address}/v1"
api_key = "test-only"
[provider.test.models.default]
model = "default"
context_window = 128000
"#
            )
        };
        if !restore {
            std::fs::write(home.join("config.toml"), &raw).unwrap();
        }
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());

        let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::task::spawn_local(async move {
            while let Some(message) = gateway_rx.recv().await {
                match message {
                    acp_transport::AcpClientMessage::SessionNotification(args) => {
                        let _ = args.response_tx.send(Ok(()));
                    }
                    acp_transport::AcpClientMessage::ExtNotification(args) => {
                        let _ = args.response_tx.send(Ok(()));
                    }
                    _ => {}
                }
            }
        });
        let agent = MvpAgent::new(GatewaySender::new(gateway_tx), &cfg).unwrap();
        cold_recovery_stage("steer-cold.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"skipGitStatus":true,"skipProjectLayout":true,"nonInteractive":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));

        if !restore {
            // ── Suspend phase: drive one real Turn into an in-flight provider
            // request, admit one Steer against it, then checkpoint the tree.
            let sid = cold_recovery_stage(
                "steer-cold.new",
                agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
            )
            .await
            .unwrap()
            .session_id;
            std::fs::write(home.join("session-id"), sid.0.as_ref()).unwrap();
            let handle = agent.control_session_handle(&sid).unwrap();
            let mut prompt_fut = agent.prompt(acp::PromptRequest::new(
                sid.clone(),
                vec![acp::ContentBlock::from("original cold task")],
            ));
            // Poll the original prompt while waiting for its sampling request
            // to reach the provider; the safe boundary cannot drain and
            // consume the steer before suspension.
            cold_recovery_stage("steer-cold.provider-held", async {
                tokio::select! {
                    biased;
                    () = arrival.notified() => (),
                    result = &mut prompt_fut => {
                        panic!("the original turn ended before the steer was admitted: {result:?}");
                    }
                }
            })
            .await;
            let prompt_id = cold_recovery_stage("steer-cold.prompt-id", async {
                loop {
                    if let Some(id) = handle.current_prompt_id.lock().unwrap().clone() {
                        return id;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await;
            std::fs::write(home.join("prompt-id"), &prompt_id).unwrap();
            cold_recovery_stage(
                "steer-cold.steer",
                agent.ext_method(acp::ExtRequest::new(
                    "grow/steer",
                    std::sync::Arc::from(
                        serde_json::value::to_raw_value(&serde_json::json!({
                            "sessionId": sid.0.as_ref(),
                            "expectedTurnId": prompt_id,
                            "interjectionId": "steer-cold-1",
                            "content": [
                                {"type": "text", "text": STEER_TEXT},
                                {"type": "image", "data": tiny_png_base64(), "mimeType": "image/png"}
                            ]
                        }))
                        .unwrap(),
                    ),
                )),
            )
            .await
            .unwrap();
            let pending = handle
                .chat_state_handle
                .get_pending_allowed_inputs()
                .await
                .unwrap();
            let (input_id, target_turn) = pending
                .iter()
                .find_map(|entry| match entry.route {
                    chat_state::InputRoute::Steer { target_turn } => {
                        Some((entry.input_id.clone(), target_turn))
                    }
                    chat_state::InputRoute::Fifo => None,
                })
                .expect("the admitted steer is still pending and unconsumed");
            std::fs::write(home.join("steer-input-id"), &input_id).unwrap();
            std::fs::write(home.join("turn-id"), target_turn.0.to_string()).unwrap();
            let (operation, nodes) = cold_recovery_stage(
                "steer-cold.suspend",
                agent.suspend_session_tree(
                    &sid,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(20),
                ),
            )
            .await
            .unwrap();
            assert_eq!(nodes.len(), 1);
            std::fs::write(home.join("operation"), operation).unwrap();
            assert!(
                agent.control_session_handle(&sid).is_none(),
                "old actor retired before the process exits"
            );
            if torn_tail {
                use std::io::Write as _;
                let path =
                    crate::session::persistence::session_dir(&handle.info).join("timeline.jsonl");
                let mut journal = std::fs::OpenOptions::new().append(true).open(path).unwrap();
                journal.write_all(b"{\"torn_test_record\":").unwrap();
                journal.sync_all().unwrap();
                // Skip process cleanup after the complete checkpoint and partial tail.
                std::process::exit(CRASH_EXIT);
            }
            drop(prompt_fut);
            let _ = &server;
            // Process exit here is the physical cold boundary.
        } else {
            // ── Restore phase: same port, real load, ExecutionReleased, then
            // the resumed Turn consumes the steer once at its safe boundary.
            let sid =
                acp::SessionId::new(std::fs::read_to_string(home.join("session-id")).unwrap());
            let input_id = std::fs::read_to_string(home.join("steer-input-id")).unwrap();
            let expected_input_ids = vec![input_id.clone()];
            let turn = chat_state::TurnId(
                std::fs::read_to_string(home.join("turn-id"))
                    .unwrap()
                    .parse()
                    .unwrap(),
            );
            let loaded = cold_recovery_stage(
                "steer-cold.load",
                agent.load_session(acp::LoadSessionRequest::new(sid.clone(), cwd)),
            )
            .await
            .unwrap();
            let foreground = loaded
                .meta
                .as_ref()
                .unwrap()
                .get("grow/foreground")
                .unwrap();
            let prompt_id = std::fs::read_to_string(home.join("prompt-id")).unwrap();
            assert_eq!(foreground["promptId"].as_str(), Some(prompt_id.as_str()));
            let handle = agent.control_session_handle(&sid).unwrap();
            // Wait for the full causal outcome before taking any snapshot:
            // the resumed model must actually see the steer (recorded request
            // content) and the canonical Consumed fact must exist.
            let events = cold_recovery_stage("steer-cold.await-consumed", async {
                loop {
                    let model_saw_steer = recorded
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|body| body.contains(STEER_TEXT));
                    let events = handle.chat_state_handle.timeline_events().await.unwrap();
                    let consumed = events.iter().any(|event| {
                        matches!(
                            &event.kind,
                            K::Input(chat_state::InputEvent::Consumed { input_ids, .. })
                                if input_ids == &expected_input_ids
                        )
                    });
                    if model_saw_steer && consumed {
                        return events;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await;
            let position = |predicate: &dyn Fn(&K) -> bool| {
                events
                    .iter()
                    .position(|event| predicate(&event.kind))
                    .expect("expected runtime fact is present")
            };
            let is_submitted = |kind: &K| {
                matches!(
                    kind,
                    K::Input(chat_state::InputEvent::Submitted { input_id: id, .. })
                        if id == &input_id
                )
            };
            let is_hook = |kind: &K| {
                matches!(
                    kind,
                    K::Hook(chat_state::HookEvent::Triggered {
                        event: chat_state::HookEventType::UserPromptSubmit,
                        cause: chat_state::HookCause::Input { input_id: id },
                        ..
                    }) if id == &input_id
                )
            };
            let is_consumed = |kind: &K| {
                matches!(
                    kind,
                    K::Input(chat_state::InputEvent::Consumed { input_ids, .. })
                        if input_ids == &expected_input_ids
                )
            };
            assert_eq!(
                events.iter().filter(|e| is_submitted(&e.kind)).count(),
                1,
                "one immutable submission survives the cold boundary"
            );
            assert_eq!(
                events.iter().filter(|e| is_hook(&e.kind)).count(),
                1,
                "admission Hook must not rerun after restoration"
            );
            assert!(
                !events.iter().any(|event| matches!(
                    &event.kind,
                    K::Input(chat_state::InputEvent::Rerouted { input_ids, .. })
                        if input_ids == &expected_input_ids
                )),
                "the preserved steer must never be rerouted to FIFO"
            );
            let consumed = events
                .iter()
                .filter_map(|event| match &event.kind {
                    K::Input(chat_state::InputEvent::Consumed {
                        input_ids,
                        turn,
                        item,
                        ..
                    }) if input_ids == &expected_input_ids => Some((*turn, item)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(consumed.len(), 1, "exactly one safe-boundary consumption");
            assert_eq!(consumed[0].0, turn, "consumption keeps the original Turn");
            let item = serde_json::to_string(consumed[0].1).unwrap();
            assert!(
                item.contains(STEER_TEXT),
                "consumption carries the full steer text: {item}"
            );
            assert!(
                item.contains("image/png"),
                "consumption carries the original image"
            );
            let released = position(&|kind| {
                matches!(
                    kind,
                    K::Runtime(SessionRuntimeEvent::ExecutionReleased { .. })
                )
            });
            let consumed_at = position(&is_consumed);
            let load_accounted = position(&|kind| {
                matches!(
                    kind,
                    K::Observation(observation)
                        if observation.scope == "session_usage"
                            && observation.name == "resume_started"
                )
            });
            assert!(
                load_accounted < released,
                "cold load must commit its resume boundary before execution is released"
            );
            assert!(
                released < consumed_at,
                "ExecutionReleased must precede the safe-boundary consumption"
            );
            if torn_tail {
                let path =
                    crate::session::persistence::session_dir(&handle.info).join("timeline.jsonl");
                let bytes = std::fs::read(path).unwrap();
                assert!(
                    !bytes
                        .windows(b"torn_test_record".len())
                        .any(|part| part == b"torn_test_record")
                );
            }
            let _ = &server;
        }
    });
}
