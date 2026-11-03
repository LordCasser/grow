//! Hermetic built-binary regression: protocol failures end a turn, not its
//! Session. Every response is constructed here and served over loopback by
//! MockInferenceServer; TestSandbox clears inherited credentials/config.
//!
//! cargo build -p cli --bin grow --offline
//! cargo test -p shell --test test_session_protocol_recovery --offline -- --ignored

#![cfg(unix)]

use acp_transport::protocol as acp;
use futures::FutureExt;
use serde_json::{Value, json};
use test_support::*;

const BACKENDS: [(&str, &str, InferenceEndpoint); 3] = [
    (
        "protocol-chat",
        "chat_completions",
        InferenceEndpoint::ChatCompletions,
    ),
    (
        "protocol-responses",
        "responses",
        InferenceEndpoint::Responses,
    ),
    ("protocol-messages", "messages", InferenceEndpoint::Messages),
];

fn terminal_model_failure_count(client: &GrowStdioClient) -> usize {
    use shell::extensions::notification::{RetryState, SessionNotification, SessionUpdate};
    client
        .grow_notifications()
        .into_iter()
        .filter(|value| {
            let notification: SessionNotification = serde_json::from_value(value.clone()).unwrap();
            matches!(
                notification.update,
                SessionUpdate::RetryState(RetryState::Failed { .. } | RetryState::Exhausted { .. })
            )
        })
        .count()
}

fn tools_reply(
    endpoint: InferenceEndpoint,
    model: &str,
    calls: &[(&str, &str, &str)],
) -> ScriptedResponse {
    let mut events = vec![
        json!({"type":"vendor.keepalive"}),
        json!({"type":"vendor.metrics","payload":{"value":1}}),
    ];
    match endpoint {
        InferenceEndpoint::ChatCompletions => {
            let calls: Vec<_> = calls.iter().enumerate().map(|(index, (id, name, arguments))|
                json!({"index":index,"id":id,"type":"function","function":{"name":name,"arguments":arguments}})).collect();
            events.push(json!({"id":"chat-protocol-test","object":"chat.completion.chunk","created":0,"model":model,
                "choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":"tool_calls"}],
                "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}));
        }
        InferenceEndpoint::Responses => {
            let calls: Vec<_> = calls.iter().enumerate().map(|(index, (id, name, arguments))|
                json!({"type":"function_call","id":format!("item-{index}"),"call_id":id,"name":name,
                    "arguments":arguments,"status":"completed"})).collect();
            events.push(json!({"type":"response.completed","sequence_number":1,"response":{
                "id":"response-protocol-test","object":"response","created_at":0,"model":model,"status":"completed",
                "output":calls,"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,
                    "input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}}}));
        }
        InferenceEndpoint::Messages => {
            events.push(json!({"type":"message_start","message":{"id":"message-protocol-test","type":"message",
                "role":"assistant","content":[],"model":model,"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}));
            for (index, (id, name, arguments)) in calls.iter().enumerate() {
                events.extend([
                    json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":id,"name":name,"input":{}}}),
                    json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":arguments}}),
                    json!({"type":"content_block_stop","index":index}),
                ]);
            }
            events.extend([
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
                json!({"type":"message_stop"}),
            ]);
        }
    }
    ScriptedResponse::sse(
        events
            .into_iter()
            .map(|event| SseEvent::data(event.to_string()))
            .collect(),
    )
}

fn foreground_bodies(server: &MockInferenceServer) -> Vec<Value> {
    server
        .request_bodies()
        .into_iter()
        .filter(|body| {
            body.get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| tools.len() >= 2)
        })
        .collect()
}

fn terminal_tool_name(body: &Value) -> String {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|tool| {
            let tool = tool.get("function").unwrap_or(tool);
            let schema = tool
                .get("parameters")
                .or_else(|| tool.get("input_schema"))?;
            schema.pointer("/properties/command")?;
            tool["name"].as_str().map(str::to_owned)
        })
        .expect("fixture model exposes a terminal tool")
}

fn has_retained_result(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            (map.get("tool_call_id")
                .or_else(|| map.get("tool_use_id"))
                .or_else(|| {
                    (map.get("type").and_then(Value::as_str) == Some("function_call_output"))
                        .then(|| map.get("call_id"))
                        .flatten()
                })
                .and_then(Value::as_str)
                == Some("call_retained"))
                || map.values().any(has_retained_result)
        }
        Value::Array(values) => values.iter().any(has_retained_result),
        _ => false,
    }
}

fn assert_clean_history(server: &MockInferenceServer, workspace: &std::path::Path, model: &str) {
    let bodies = foreground_bodies(server);
    let body = bodies.last().unwrap();
    assert_eq!(
        body["model"], model,
        "model switch must reach the actual wire request"
    );
    let history = body.get("messages").or_else(|| body.get("input")).unwrap();
    let text = history.to_string();
    assert!(
        text.contains("KEEP_HISTORY"),
        "valid user history must survive"
    );
    assert!(
        has_retained_result(history),
        "already executed tool result must survive"
    );
    assert!(
        !text.contains("call_invalid") && !text.contains("call_sibling"),
        "invalid response must not enter later requests"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("executed.txt")).unwrap(),
        "retained\n",
        "previously executed work must not be replayed"
    );
    assert!(
        !workspace.join("must-not-execute.txt").exists(),
        "no sibling of an invalid call may execute"
    );
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary, but no external API or credentials"]
async fn protocol_failure_preserves_session_across_continue_switch_and_process_restart() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (model, _, endpoint) in BACKENDS {
            for lifecycle in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let workdir = git_workdir();
                let workspace = workdir.workspace();
                let sandbox = TestSandbox::new();
                // The canonical catalog comes from explicit provider config,
                // not the mock's legacy remote model-list endpoint.
                let mut config = format!(
                    "[models]\ndefault = \"mock/{model}\"\n\n[provider.mock.options]\nbase_url = {:?}\nenv_key = \"GROW_API_KEY\"\n",
                    server.url()
                );
                for (name, backend, _) in BACKENDS {
                    config.push_str(&format!(
                        "\n[provider.mock.models.{name}]\nmodel = \"{name}\"\napi_backend = \"{backend}\"\ncontext_window = 200000\nagent_type = \"grow-build\"\nmax_retries = 5\n"
                    ));
                }
                std::fs::write(sandbox.grow_home().join("config.toml"), config).unwrap();
                let mut client = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
                if let Err(panic) = std::panic::AssertUnwindSafe(initialize_client(&client, lifecycle)).catch_unwind().await {
                    eprintln!("{}\n{}", client.process_diagnostics(), client.stderr());
                    std::panic::resume_unwind(panic);
                }
                let session = client
                    .create_session_with_model_timeout(workspace, &format!("mock/{model}"))
                    .await;
                let resident_epoch = control_epoch(&client, &session).await;
                let process_id = client.child_pid();
                let transient = server.expect_response(
                    "transient failure recovers without a terminal warning",
                    InferenceRequestMatcher::foreground(endpoint),
                    ScriptedResponse::json(503, json!({"error":{"type":"api_error","message":"constructed temporary failure"}})),
                );
                client
                    .prompt_with_timeout(&session, "KEEP_HISTORY")
                    .await
                    .unwrap();
                transient.assert_satisfied();
                let tool_name = terminal_tool_name(foreground_bodies(&server).last().unwrap());
                assert_eq!(terminal_model_failure_count(&client), 0);
                let retained_args = json!({"command":"printf 'retained\\n' | tee -a executed.txt",
                "description":"write an isolated test marker","background":false})
                .to_string();
                let forbidden_args = json!({"command":"touch must-not-execute.txt",
                "description":"must never be dispatched","background":false})
                .to_string();
                let retained = server.expect_response(
                    "valid tool before failed sample",
                    InferenceRequestMatcher::foreground(endpoint),
                    tools_reply(
                        endpoint,
                        model,
                        &[("call_retained", &tool_name, &retained_args)],
                    ),
                );
                let malformed_response = || tools_reply(endpoint, model, &[
                    ("call_sibling", &tool_name, &forbidden_args),
                    ("call_invalid", &tool_name, "{"),
                ]);
                let malformed_count = if lifecycle { 3 } else { 1 };
                let invalid_attempts: Vec<_> = (0..malformed_count).map(|attempt| server.expect_response(
                    format!("malformed tool response {attempt}"),
                    InferenceRequestMatcher::foreground(endpoint), malformed_response(),
                )).collect();
                let before = foreground_bodies(&server).len();
                let settled_before = settled_attempts(&client, &session, workspace);
                let error = client
                    .prompt_with_timeout(&session, "execute the test step")
                    .await
                    .expect_err("invalid response must fail the turn");
                assert!(
                    format!("{error:?}").contains("model returned invalid tool arguments:"),
                    "{model}: {error:?}"
                );
                retained.assert_satisfied();
                for invalid_attempt in invalid_attempts {
                    invalid_attempt.assert_satisfied();
                }
                assert_eq!(terminal_model_failure_count(&client), usize::from(lifecycle),
                    "retractable failure emits its deferred notice; irreversible output reports through the ACP error");
                assert_eq!(failed_turns(&client, &session, workspace), 1);
                assert_eq!(
                    foreground_bodies(&server).len(),
                    before + 1 + malformed_count,
                    "malformed samples respect the negotiated output delivery boundary"
                );
                assert_eq!(settled_attempts(&client, &session, workspace), settled_before + 1 + malformed_count);

                // Use actual ACP admission/control paths. Never reset Idle, swap
                // conversation items, or manually invoke handle_completion.
                let before_manual = foreground_bodies(&server).len();
                client.prompt_with_timeout(&session, "继续").await.unwrap();
                assert_eq!(foreground_bodies(&server).len(), before_manual + 1,
                    "a healthy provider must complete the first manually continued request");
                assert_clean_history(&server, workspace, model);
                let continue_bad_count = if lifecycle { 2 } else { 0 };
                let continue_invalid_attempts: Vec<_> = (0..continue_bad_count).map(|attempt| server.expect_response(
                    format!("malformed sample in next turn {attempt}"),
                    InferenceRequestMatcher::foreground(endpoint), malformed_response(),
                )).collect();
                let before_continue = foreground_bodies(&server).len();
                client
                    .prompt_with_timeout(&session, "exercise the fresh recovery budget")
                    .await
                    .unwrap();
                for invalid_attempt in continue_invalid_attempts {
                    invalid_attempt.assert_satisfied();
                }
                assert_eq!(
                    foreground_bodies(&server).len(),
                    before_continue + continue_bad_count + 1,
                    "the next turn gets its own malformed-sample budget and succeeds on the default third response"
                );
                assert_clean_history(&server, workspace, model);
                assert_eq!(control_epoch(&client, &session).await, resident_epoch,
                    "manual continuation must retain the resident actor");
                assert_eq!(client.child_pid(), process_id);
                assert_eq!(settled_attempts(&client, &session, workspace), settled_before + malformed_count + continue_bad_count + 3);
                assert_eq!(terminal_model_failure_count(&client), usize::from(lifecycle), "successful continuation must not warn again");
                assert_eq!(failed_turns(&client, &session, workspace), 1);
                for (target, _, _) in BACKENDS {
                    client
                        .set_model_with_timeout(&session, &format!("mock/{model}"))
                        .await
                        .unwrap();
                    client
                        .set_model_with_timeout(&session, &format!("mock/{target}"))
                        .await
                        .unwrap();
                    client
                        .prompt_with_timeout(&session, "continue after model switch")
                        .await
                        .unwrap();
                    assert_clean_history(&server, workspace, target);
                }

                client
                    .ext_method(
                        "grow/session/close",
                        json!({"sessionId":session.0.as_ref()}),
                    )
                    .await
                    .unwrap();
                client.close().await.unwrap();
                let sandbox = client.take_sandbox();
                drop(client);
                let mut reloaded =
                    GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
                initialize_client(&reloaded, lifecycle).await;
                reloaded
                    .load_session_with_timeout(&session, workspace)
                    .await;
                reloaded
                    .prompt_with_timeout(&session, "continue after restart")
                    .await
                    .unwrap();
                assert_clean_history(&server, workspace, BACKENDS.last().unwrap().0);
                reloaded.close().await.unwrap();
            }
            }
        })
        .await;
}

async fn initialize_client(client: &GrowStdioClient, lifecycle: bool) {
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        client.initialize_with_attempt_lifecycle(lifecycle),
    )
    .await
    .unwrap_or_else(|_| panic!("initialize timed out: {}", client.stderr()));
}

async fn control_epoch(client: &GrowStdioClient, session: &acp::SessionId) -> String {
    let response = client
        .ext_method(
            "grow/queue/prompt_status",
            json!({"sessionId":session.0.as_ref(),"promptId":"recovery-observation"}),
        )
        .await
        .unwrap();
    let response: Value = serde_json::from_str(response.0.get()).unwrap();
    response["controlEpoch"]
        .as_str()
        .expect("the session must have a resident owner")
        .to_owned()
}

fn timeline(
    client: &GrowStdioClient,
    session: &acp::SessionId,
    _cwd: &std::path::Path,
) -> Vec<chat_state::TimelineEvent> {
    shell::session::storage::load_timeline_by_id_at(
        session.0.as_ref(),
        client.sandbox().grow_home(),
    )
    .unwrap()
    .expect("the live session's Timeline must be observable")
    .events()
    .to_vec()
}

fn settled_attempts(
    client: &GrowStdioClient,
    session: &acp::SessionId,
    cwd: &std::path::Path,
) -> usize {
    let mut keys = std::collections::HashSet::new();

    timeline(client, session, cwd)
        .iter()
        .filter(|event| {
            if let chat_state::TimelineEventKind::Observation(observation) = &event.kind
                && observation.scope == "sampling_usage"
                && observation.name == "attempt_settled"
            {
                assert!(
                    keys.insert(
                        observation.data.as_ref().unwrap()["attempt_key"]
                            .as_str()
                            .unwrap()
                            .to_owned()
                    ),
                    "each attempt must settle exactly once"
                );
                true
            } else {
                false
            }
        })
        .count()
}

fn failed_turns(
    client: &GrowStdioClient,
    session: &acp::SessionId,
    cwd: &std::path::Path,
) -> usize {
    timeline(client, session, cwd)
        .iter()
        .filter(|event| {
            matches!(&event.kind,
        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { terminal, .. })
            if terminal.stop_reason == "error")
        })
        .count()
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary, but no external API or credentials"]
async fn malformed_wire_json_allows_manual_continuation_in_the_same_resident_session() {
    tokio::task::LocalSet::new().run_until(async {
        for (model, backend, endpoint) in BACKENDS {
            let server = MockInferenceServer::start().await.unwrap();
            let sandbox = TestSandbox::builder().git().build();
            let workspace = sandbox.workspace().to_path_buf();
            std::fs::write(sandbox.grow_home().join("config.toml"), format!(
                "[models]\ndefault = \"mock/{model}\"\n[provider.mock]\napi_backend = \"{backend}\"\n[provider.mock.options]\nbase_url = {:?}\nenv_key = \"GROW_API_KEY\"\n[provider.mock.models.{model}]\ncontext_window = 200000\nagent_type = \"grow-build\"\nmax_retries = 5\n", server.url(),
            )).unwrap();
            let mut client = GrowStdioClient::spawn_with_sandbox(&server, &workspace, sandbox).await;
            initialize_client(&client, true).await;
            let session = client.create_session_with_model_timeout(&workspace, &format!("mock/{model}")).await;
            let epoch = control_epoch(&client, &session).await;
            let invalid = server.expect_response("deterministically malformed wire JSON",
                InferenceRequestMatcher::foreground(endpoint),
                ScriptedResponse::sse(vec![SseEvent::data("{not-json")]));
            assert!(client.prompt_with_timeout(&session, "retain user history").await.is_err());
            invalid.assert_satisfied();
            assert_eq!(foreground_bodies(&server).len(), 1, "protocol errors must retain their no-retry classification");
            client.prompt_with_timeout(&session, "继续").await.unwrap();
            assert_eq!(control_epoch(&client, &session).await, epoch);
            assert_eq!(foreground_bodies(&server).len(), 2);
            assert!(foreground_bodies(&server).last().unwrap().to_string().contains("retain user history"));
            client.close().await.unwrap();
        }
    }).await;
}

async fn command(client: &GrowStdioClient, session: &acp::SessionId, text: &str) {
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        client.ext_method(
            "grow/commands/execute",
            json!({"sessionId":session.0.as_ref(),"command":format!("/{text}"),"description":text,
            "invocationId":uuid::Uuid::now_v7().to_string()}),
        ),
    )
    .await
    .expect("host command must settle")
    .unwrap();
}

fn durable_goal(
    client: &GrowStdioClient,
    session: &acp::SessionId,
    cwd: &std::path::Path,
) -> Option<shell::session::goal_tracker::GoalState> {
    let events = timeline(client, session, cwd);
    shell::session::control::SessionControlSnapshot::latest_from_timeline(&events)
        .unwrap()
        .and_then(|snapshot| snapshot.goal)
}

async fn wait_goal(
    client: &GrowStdioClient,
    session: &acp::SessionId,
    cwd: &std::path::Path,
    status: shell::session::goal_tracker::GoalStatus,
) -> shell::session::goal_tracker::GoalState {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            if let Some(goal) = durable_goal(client, session, cwd)
                && goal.status == status
            {
                return goal;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Goal did not reach {status:?}: {}", client.stderr()))
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary, but no external API or credentials"]
async fn goal_sampling_failure_requires_explicit_restart_and_preserves_budget_gates() {
    use shell::session::goal_tracker::GoalStatus;
    tokio::task::LocalSet::new().run_until(async {
        for (known_usage, budget, attempts, stopped) in [
            (true, 1000, 3, GoalStatus::Paused),
            (true, 30, 2, GoalStatus::BudgetLimited),
            (false, 1000, 1, GoalStatus::Paused),
        ] {
            let server = MockInferenceServer::start().await.unwrap();
            let mut sandbox = TestSandbox::builder().git().build();
            sandbox.set_env("GROW_GOAL", "1");
            let workspace = sandbox.workspace().to_path_buf();
            std::fs::write(sandbox.grow_home().join("config.toml"), format!(
                "[models]\ndefault = \"mock/recovery\"\n[provider.mock]\napi_backend = \"chat_completions\"\n[provider.mock.options]\nbase_url = {:?}\nenv_key = \"GROW_API_KEY\"\n[provider.mock.models.recovery]\ncontext_window = 200000\nagent_type = \"grow-build\"\nmax_retries = 5\n", server.url(),
            )).unwrap();
            let mut client = GrowStdioClient::spawn_with_sandbox(&server, &workspace, sandbox).await;
            initialize_client(&client, true).await;
            let session = client.create_session_with_model_timeout(&workspace, "mock/recovery").await;
            client.prompt_with_timeout(&session, "prime the test catalog").await.unwrap();
            let body = foreground_bodies(&server).pop().unwrap();
            let tool_name = terminal_tool_name(&body);
            let update_goal = body["tools"].as_array().unwrap().iter().find_map(|tool| {
                let tool = tool.get("function").unwrap_or(tool);
                let name = tool["name"].as_str()?;
                (name.to_ascii_lowercase().replace('_', "") == "updategoal").then(|| name.to_owned())
            }).expect("Goal-enabled production session must expose UpdateGoal");
            let before_failure = foreground_bodies(&server).len();
            let mut expectations = Vec::new();
            for attempt in 0..attempts {
                let response = if known_usage {
                    tools_reply(InferenceEndpoint::ChatCompletions, "recovery", &[("call_invalid", &tool_name, "{")])
                } else {
                    ScriptedResponse::sse(vec![SseEvent::data(json!({
                        "id":"goal-unknown-usage","object":"chat.completion.chunk","created":0,"model":"recovery",
                        "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_invalid","type":"function",
                            "function":{"name":tool_name,"arguments":"{"}}]},"finish_reason":"tool_calls"}]
                    }).to_string()), SseEvent::data("[DONE]")])
                };
                expectations.push(server.expect_response(format!("Goal malformed attempt {attempt}"),
                    InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions), response));
            }
            command(&client, &session, &format!("goal set finish the recovery fixture --budget {budget}")).await;
            let paused = wait_goal(&client, &session, &workspace, stopped).await;
            for expectation in expectations { expectation.assert_satisfied(); }
            assert_eq!(foreground_bodies(&server).len(), before_failure + attempts);
            assert_eq!(paused.tokens_used, if known_usage { attempts as i64 * 15 } else { 0 });
            assert_eq!(paused.usage_incomplete, !known_usage);
            // Healthy provider alone cannot restart autonomous work.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            assert_eq!(foreground_bodies(&server).len(), before_failure + attempts);
            let before_continue = foreground_bodies(&server).len();
            let continued = client.prompt_with_timeout(&session, "继续").await;
            continued.expect("manual interaction remains available after a Goal stops");
            assert_eq!(foreground_bodies(&server).len(), before_continue + 1);
            let after_continue = durable_goal(&client, &session, &workspace).unwrap();
            assert_eq!(after_continue.goal_id, paused.goal_id);
            assert_eq!(after_continue.tokens_used, paused.tokens_used);
            assert_eq!(after_continue.status, stopped);
            if known_usage && budget > paused.tokens_used {
                let completed = server.expect_response("explicitly restarted Goal can finish",
                    InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                    tools_reply(InferenceEndpoint::ChatCompletions, "recovery", &[("call_goal_complete", &update_goal, r#"{"status":"complete","completion_report":"All requested work verified complete."}"#)]));
                command(&client, &session, "goal restart").await;
                let finished = wait_goal(&client, &session, &workspace, GoalStatus::Complete).await;
                completed.assert_satisfied();
                assert_eq!(finished.goal_id, paused.goal_id);
                assert!(finished.tokens_used >= paused.tokens_used + 15);
                assert!(!finished.usage_incomplete);
            } else {
                // The explicit user Turn is outside the stopped Goal. Its
                // successful usage cannot resolve that Goal's old budget gate.
                let after_manual = foreground_bodies(&server).len();
                command(&client, &session, "goal restart").await;
                command(&client, &session, "goal status").await;
                let still_stopped = durable_goal(&client, &session, &workspace).unwrap();
                assert_eq!(still_stopped.goal_id, paused.goal_id);
                assert_eq!(still_stopped.tokens_used, paused.tokens_used);
                assert_eq!(still_stopped.status, stopped);
                assert_eq!(still_stopped.usage_incomplete, !known_usage);
                assert_eq!(foreground_bodies(&server).len(), after_manual,
                    "a blocked Goal restart cannot dispatch autonomous model work");
            }
            client.close().await.unwrap();
        }
    }).await;
}

#[cfg(unix)]
mod auxiliary_image_process_journeys {
    use acp_transport::protocol as acp;
    use base64::Engine as _;
    use serde_json::json;
    use test_support::*;

    fn messages_text_turn(text: &str, model: &str) -> ScriptedResponse {
        ScriptedResponse::sse(vec![
            SseEvent::data(json!({
                "type": "message_start",
                "message": {"id":"journey-message","type":"message","role":"assistant","content":[],"model":model,"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}
            }).to_string()),
            SseEvent::data(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}).to_string()),
            SseEvent::data(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}).to_string()),
            SseEvent::data(json!({"type":"content_block_stop","index":0}).to_string()),
            SseEvent::data(json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":10}}).to_string()),
            SseEvent::data(json!({"type":"message_stop"}).to_string()),
        ])
    }

    fn valid_png() -> Vec<u8> {
        let image = image::ImageBuffer::from_pixel(32, 32, image::Rgba([128, 64, 32, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode valid test PNG");
        png.into_inner()
    }

    fn valid_png_base64() -> String {
        base64::engine::general_purpose::STANDARD.encode(valid_png())
    }

    fn chat_text_turn(text: &str, model: &str) -> ScriptedResponse {
        ScriptedResponse::sse(vec![
            SseEvent::data(json!({
                "id":"flush-override",
                "object":"chat.completion.chunk",
                "created":0,
                "model":model,
                "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}],
                "usage":{"prompt_tokens":30,"completion_tokens":10,"total_tokens":40}
            }).to_string()),
            SseEvent::data(json!({
                "id":"flush-override",
                "object":"chat.completion.chunk",
                "created":0,
                "model":model,
                "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":30,"completion_tokens":10,"total_tokens":40}
            }).to_string()),
            SseEvent::data("[DONE]"),
        ])
    }

    async fn init(client: &GrowStdioClient) {
        tokio::time::timeout(std::time::Duration::from_secs(20), client.initialize())
            .await
            .unwrap_or_else(|_| panic!("initialize timed out\n{}", client.stderr()));
    }

    async fn ext_with_timeout(
        client: &GrowStdioClient,
        method: &str,
        params: serde_json::Value,
    ) -> acp::Result<acp::ExtResponse> {
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.ext_method(method, params),
        )
        .await
        .unwrap_or_else(|_| panic!("{method} timed out\n{}", client.stderr()))
    }

    fn configure_journey_models(sandbox: &TestSandbox, server: &MockInferenceServer, memory: bool) {
        let memory_config = if memory {
            "\n[memory]\nenabled = true\n"
        } else {
            ""
        };
        let config = format!(
            r#"[models]
            default = "mock/vision"
            image_description = "mock/vision"
            session_title = "title/title"

            [provider.mock.options]
            base_url = {:?}
            env_key = "GROW_API_KEY"

            [provider.mock.models.vision]
            model = "vision"
            api_backend = "messages"
            context_window = 200000
            agent_type = "grow-build"

            [provider.mock.models.text]
            model = "text"
            api_backend = "messages"
            context_window = 200000
            agent_type = "grow-build"

                [provider.mock.models.text-coordination]
                model = "text-coordination"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.mock.models.text-btw]
                model = "text-btw"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.mock.models.text-recap]
                model = "text-recap"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.mock.models.text-flush]
                model = "text-flush"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

            [provider.title.options]
            base_url = {:?}
            env_key = "GROW_API_KEY"

            [provider.title.models.title]
            model = "title"
            api_backend = "responses"
            context_window = 200000
            agent_type = "grow-build"
            "#,
            server.url(),
            server.url()
        );
        std::fs::write(
            sandbox.grow_home().join("config.toml"),
            format!("{config}{memory_config}"),
        )
        .unwrap();
    }

    fn expect_unknown_route_recovery(
        server: &MockInferenceServer,
        label: &str,
        text_model: &str,
        conversion_required: bool,
    ) -> Vec<InferenceExpectation> {
        let mut expectations = vec![server.expect_response(
                format!("{label}: text route rejects image"),
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                ScriptedResponse::json(400, json!({
                    "type":"error",
                    "error":{"type":"invalid_request_error","code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}
                })),
            )];
        if conversion_required {
            expectations.push(server.expect_response(
                format!("{label}: vision route describes frozen image"),
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn("The screenshot contains build error E42.", "vision"),
            ));
        }
        expectations.push(server.expect_response(
            format!("{label}: text route retries with description"),
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_text_turn("The answer is based on E42.", text_model),
        ));
        expectations
    }

    fn assert_recovery_requests(
        server: &MockInferenceServer,
        start: usize,
        text_model: &str,
        converted_before: bool,
    ) {
        let requests = server.requests();
        let bodies: Vec<_> = requests[start..]
            .iter()
            .filter(|request| request.path == "/v1/messages")
            .filter_map(|request| request.body.as_ref())
            .collect();
        assert_eq!(
            bodies.len(),
            if converted_before { 2 } else { 3 },
            "one rejection, optional vision conversion, and one retry"
        );
        assert_eq!(bodies[0]["model"], text_model);
        assert_eq!(
            wire_image_count(bodies[0]),
            1,
            "unknown route retains its first optimistic image attempt"
        );
        let retry = if converted_before {
            &bodies[1]
        } else {
            assert_eq!(bodies[1]["model"], "vision");
            assert_eq!(
                wire_image_count(bodies[1]),
                1,
                "visual route receives frozen original image"
            );
            &bodies[2]
        };
        assert_eq!(retry["model"], text_model);
        assert_eq!(
            wire_image_count(retry),
            0,
            "retry sends the acknowledged description without raw media"
        );
        assert!(retry.to_string().contains("E42"));
    }

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

    fn request_summary(server: &MockInferenceServer) -> Vec<String> {
        server
            .requests()
            .iter()
            .filter_map(|request| {
                let body = request.body.as_ref()?;
                let last_message_text = body
                    .get("messages")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|messages| messages.last())
                    .and_then(|message| message.get("content"))
                    .map(|content| text_fields(content).join(" "))
                    .unwrap_or_default();
                Some(format!(
                    "{} model={:?} images={} tools={} last_text={:?}",
                    request.path,
                    body.get("model"),
                    wire_image_count(body),
                    body.get("tools")
                        .and_then(serde_json::Value::as_array)
                        .map_or(0, Vec::len),
                    last_message_text.chars().take(240).collect::<String>(),
                ))
            })
            .collect()
    }

    fn text_fields(value: &serde_json::Value) -> Vec<String> {
        match value {
            serde_json::Value::Object(object) => {
                let mut texts = object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(|text| vec![text.to_owned()])
                    .unwrap_or_default();
                texts.extend(object.values().flat_map(text_fields));
                texts
            }
            serde_json::Value::Array(values) => values.iter().flat_map(text_fields).collect(),
            _ => Vec::new(),
        }
    }

    fn files_below(root: &std::path::Path) -> Vec<std::path::PathBuf> {
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

    fn session_dir(grow_home: &std::path::Path, session_id: &str) -> std::path::PathBuf {
        let sessions = grow_home.join("sessions");
        for cwd in std::fs::read_dir(&sessions).unwrap().flatten() {
            let candidate = cwd.path().join(session_id);
            if candidate.join("summary.json").is_file() {
                return candidate;
            }
        }
        panic!(
            "session {session_id} has no persisted summary below {}",
            sessions.display()
        );
    }

    fn persistent_workspace() -> tempfile::TempDir {
        let parent = std::env::current_dir()
            .expect("read workspace root")
            .join("target/auxiliary-journey-workspaces");
        std::fs::create_dir_all(&parent).expect("create ignored test workspace root");
        tempfile::Builder::new()
            .prefix("grow-auxiliary-")
            .tempdir_in(parent)
            .expect("create isolated non-ephemeral workspace")
    }

    #[tokio::test]
    #[ignore = "requires a freshly built grow binary; all model traffic is loopback"]
    async fn text_route_rejection_projects_image_and_survives_process_reload() {
        tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_settings(json!({"image_description_model":"mock/vision"}));
            let workdir = persistent_workspace();
            let workspace = workdir.path();
            let sandbox = TestSandbox::new();
            let config = format!(
                r#"[models]
                default = "mock/text"
                image_description = "mock/vision"
                session_title = "title/title"

                [provider.mock.options]
                base_url = {:?}
                env_key = "GROW_API_KEY"

                [provider.mock.models.text]
                model = "text"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.mock.models.text-after-reload]
                model = "text"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.mock.models.vision]
                model = "vision"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.title.options]
                base_url = {:?}
                env_key = "GROW_API_KEY"

                [provider.title.models.title]
                model = "title"
                api_backend = "responses"
                context_window = 200000
                agent_type = "grow-build"
                "#,
                server.url(),
                server.url()
            );
            std::fs::write(sandbox.grow_home().join("config.toml"), config).unwrap();

            let mut client = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
            init(&client).await;
            let session = client.create_session_with_model_timeout(workspace, "mock/text").await;

            let rejected = server.expect_response(
                "text model explicitly rejects image input",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                ScriptedResponse::json(400, json!({
                    "type":"error",
                    "error":{"type":"invalid_request_error","code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}
                })),
            );
            let described = server.expect_response(
                "configured vision model describes the image",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn("A red build error dialog displays E42.", "vision"),
            );
            let recovered = server.expect_response(
                "same text route retries with projected text",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("The screenshot shows build error E42.", "text"),
            );
            client.prompt_with_content_timeout(&session, vec![
                acp::ContentBlock::Text(acp::TextContent::new("Read this screenshot.")),
                acp::ContentBlock::Image(acp::ImageContent::new(
                    valid_png_base64(),
                    "image/png",
                )),
            ]).await.unwrap_or_else(|error| panic!(
                "image recovery prompt failed: {error:?}; requests={:?}",
                request_summary(&server)
            ));
            rejected.assert_satisfied();
            described.assert_satisfied();
            recovered.assert_satisfied();

            let foreground: Vec<_> = server.requests().into_iter()
                .filter(|request| request.path == "/v1/messages" && request.body.as_ref().is_some_and(|body| body.get("tools").and_then(serde_json::Value::as_array).map_or(0, Vec::len) >= 2))
                .filter_map(|request| request.body)
                .collect();
            assert_eq!(foreground.len(), 2, "one rejected image request and one recovery request; requests={:?}", request_summary(&server));
            assert_eq!(wire_image_count(&foreground[0]), 1);
            assert_eq!(wire_image_count(&foreground[1]), 0);
            let description_requests: Vec<_> = server.requests().into_iter()
                .filter(|request| request.path == "/v1/messages")
                .filter_map(|request| request.body)
                .filter(|body| body["model"] == "vision" && wire_image_count(body) == 1
                    && body.get("tools").and_then(serde_json::Value::as_array).map_or(0, Vec::len) == 0)
                .collect();
            assert_eq!(description_requests.len(), 1, "one vision request must receive the original image; requests={:?}", request_summary(&server));
            assert!(description_requests[0].to_string().contains("Transcribe the attached image"));
            let sideband_root = session_dir(client.sandbox().grow_home(), session.0.as_ref())
                .join("sidebands");
            let ledger_events: Vec<serde_json::Value> = files_below(&sideband_root)
                .into_iter()
                .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
                .flat_map(|path| std::fs::read_to_string(path).unwrap().lines().map(str::to_owned).collect::<Vec<_>>())
                .map(|line| serde_json::from_str(&line).expect("valid sideband JSONL"))
                .collect();
            let description_id = ledger_events.iter()
                .find(|event| event.pointer("/kind/event/purpose").and_then(serde_json::Value::as_str) == Some("image-description"))
                .and_then(|event| event.get("sideband_id"))
                .and_then(serde_json::Value::as_str)
                .expect("image description must be durably recorded as a sideband");
            let description_output = ledger_events.iter()
                .find(|event| event.get("sideband_id").and_then(serde_json::Value::as_str) == Some(description_id)
                    && event.pointer("/kind/type").and_then(serde_json::Value::as_str) == Some("result"))
                .and_then(|event| event.pointer("/kind/event/raw_output"))
                .and_then(serde_json::Value::as_str)
                .expect("ImageDescription sideband must persist its raw output");
            assert!(description_output.contains("E42"), "actual vision output was {description_output:?}");
            assert!(
                foreground[1].to_string().contains("E42"),
                "retry must carry acknowledged vision output; requests={:?}; actual ImageDescription output={description_output:?}",
                request_summary(&server)
            );

            client.close().await.unwrap();
            let sandbox = client.take_sandbox();
            drop(client);
            let mut reloaded = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
            init(&reloaded).await;
            reloaded.load_session_with_timeout(&session, workspace).await;
            reloaded.set_model_with_timeout(&session, "mock/text-after-reload").await.unwrap();
            let after_reload = server.expect_response(
                "reloaded text model sees durable description without image media",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("The durable image description is E42.", "text"),
            );
            reloaded.prompt_with_timeout(&session, "Continue from the screenshot evidence.").await.unwrap();
            after_reload.assert_satisfied();
            let last = server.request_bodies().into_iter().filter(|body| body.get("tools").is_some()).last().unwrap();
            let serialized = last.to_string();
            assert_eq!(wire_image_count(&last), 0, "reloaded prompt must not resend raw image: {last:#}");
            assert!(serialized.contains("E42"), "reloaded request must include durable projected text");

            let fork_workdir = persistent_workspace();
            let fork_cwd = fork_workdir.path();
            let forked: serde_json::Value = serde_json::from_str(
                ext_with_timeout(&reloaded, "grow/session/fork", json!({
                    "sourceSessionId": session.0.as_ref(),
                    "sourceCwd": workspace.to_string_lossy(),
                    "newCwd": fork_cwd.to_string_lossy(),
                    "newModelId": "mock/text-after-reload"
                })).await.unwrap().0.get()
            ).unwrap();
            let fork_session = acp::SessionId::new(forked["newSessionId"].as_str().unwrap());
            reloaded.load_session_with_timeout(&fork_session, fork_cwd).await;
            ext_with_timeout(
                    &reloaded,
                    "grow/rewind/execute",
                    json!({
                        "sessionId": fork_session.0.as_ref(),
                        "targetPromptIndex": 1,
                        "force": false,
                        "mode": "conversation_only"
                    }),
                )
                .await
                .unwrap();
            let fork_rejection = server.expect_response(
                "forked, rewound text route makes its first optimistic image attempt",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                ScriptedResponse::json(400, json!({
                    "type":"error",
                    "error":{"type":"invalid_request_error","code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}
                })),
            );
            let fork_prompt = server.expect_response(
                "forked, rewound and loaded session recovers with durable image description",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("The fork retained E42.", "text"),
            );
            let before_fork_prompt = server.requests().len();
            reloaded.prompt_with_timeout(&fork_session, "Use the forked screenshot context.").await.unwrap();
            fork_rejection.assert_satisfied();
            fork_prompt.assert_satisfied();
            let requests = server.requests();
            let fork_requests: Vec<_> = requests[before_fork_prompt..]
                .iter()
                .filter_map(|request| request.body.as_ref())
                .filter(|body| body.get("model").and_then(serde_json::Value::as_str) == Some("text"))
                .collect();
            assert_eq!(fork_requests.len(), 2, "fork first rejects then retries on the same model");
            assert_eq!(wire_image_count(fork_requests[0]), 1);
            assert_eq!(wire_image_count(fork_requests[1]), 0);
            assert!(fork_requests[1].to_string().contains("E42"), "forked retry must use the persisted projection");
            reloaded.close().await.unwrap();
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires a freshly built grow binary; all model traffic is loopback"]
    async fn auxiliary_process_entries_recover_frozen_image_and_flush_to_memory_file() {
        tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_settings(json!({"image_description_model":"mock/vision"}));
            let workdir = persistent_workspace();
            let workspace = workdir.path();
            let sandbox = TestSandbox::new();
            configure_journey_models(&sandbox, &server, true);

            let mut client = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
            init(&client).await;
            let target = client.create_session_with_model_timeout(workspace, "mock/vision").await;
            let source = client.create_session_with_model_timeout(workspace, "mock/vision").await;
            let primary = server.expect_response(
                "multimodal primary accepts original image",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("The image is a build error dialog, E42.", "vision"),
            );
            client
                .prompt_with_content_timeout(
                    &target,
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("Inspect this image.")),
                        acp::ContentBlock::Image(acp::ImageContent::new(
                            valid_png_base64(),
                            "image/png",
                        )),
                    ],
                )
                .await
                .unwrap();
            primary.assert_satisfied();
            // Each public auxiliary owner captures the unresolved original image.
            // The first route learns its limitation and records a reusable frozen
            // description; later consumer routes prove recovery from that ledger.
            client.set_model_with_timeout(&target, "mock/text-coordination").await.unwrap();
            let coordination = expect_unknown_route_recovery(&server, "coordination", "text-coordination", true);
            let before_coordination = server.requests().len();
            let inquiry = ext_with_timeout(
                    &client,
                    "grow/coordination/ask",
                    json!({
                        "inquiryId":uuid::Uuid::now_v7().to_string(),
                        "sourceSessionId":source.0.as_ref(),
                        "targetSessionId":target.0.as_ref(),
                        "question":"What does the screenshot show?"
                    }),
                )
                .await
                .unwrap();
            let inquiry: serde_json::Value = serde_json::from_str(inquiry.0.get()).unwrap();
            assert_eq!(
                inquiry["status"],
                "answered",
                "coordination response={inquiry}; requests={:?}",
                request_summary(&server)
            );
            assert!(inquiry["answer"].as_str().unwrap().contains("E42"));
            for expectation in coordination {
                expectation.assert_satisfied();
            }
            assert_recovery_requests(&server, before_coordination, "text-coordination", false);

            client.set_model_with_timeout(&target, "mock/text-btw").await.unwrap();
            let btw = expect_unknown_route_recovery(&server, "btw", "text-btw", false);
            let before_btw = server.requests().len();
            let answer = ext_with_timeout(
                    &client,
                    "grow/btw",
                    json!({ "sessionId": target.0.as_ref(), "question": "What is in the image?" }),
                )
                .await
                .unwrap();
            let answer: serde_json::Value = serde_json::from_str(answer.0.get()).unwrap();
            assert!(answer["result"]["answer"].as_str().is_some_and(|answer| answer.contains("E42")), "btw response={answer}; requests={:?}; stderr={}", request_summary(&server), client.stderr());
            for expectation in btw {
                expectation.assert_satisfied();
            }
            assert_recovery_requests(&server, before_btw, "text-btw", true);

            client.set_model_with_timeout(&target, "mock/text-recap").await.unwrap();
            let recap = expect_unknown_route_recovery(&server, "recap", "text-recap", false);
            let before_recap = server.requests().len();
            ext_with_timeout(&client, "grow/recap", json!({ "sessionId": target.0.as_ref() }))
                .await
                .unwrap();
            for mut expectation in recap {
                tokio::time::timeout(std::time::Duration::from_secs(10), expectation.wait_satisfied())
                    .await
                    .expect("recap inference should finish from its live process request");
                expectation.assert_satisfied();
            }
            assert_recovery_requests(&server, before_recap, "text-recap", true);
            let recap_timeline = shell::session::storage::load_timeline_by_id_at(
                target.0.as_ref(),
                client.sandbox().grow_home(),
            )
            .unwrap()
            .unwrap();
            assert!(recap_timeline.events().iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Sideband(spawn)
                    if spawn.purpose == chat_state::SidebandPurpose::SessionRecap
            )), "manual recap completes as a durable SessionRecap sideband");

            let flush = [
                server.expect_response(
                    "flush's unknown text route rejects original image",
                    InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                    ScriptedResponse::json(400, json!({
                        "type":"error",
                        "error":{"type":"invalid_request_error","code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}
                    })),
                ),
                server.expect_response(
                    "flush retries text model with selected description",
                    InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                    messages_text_turn("## Debugging\nThe screenshot showed build error E42.", "text-flush"),
                ),
            ];
            client.set_model_with_timeout(&target, "mock/text-flush").await.unwrap();
            let before_flush = server.requests().len();
            ext_with_timeout(&client, "grow/memory/flush", json!({ "session_id": target.0.as_ref() }))
                .await
                .unwrap();
            for expectation in flush {
                expectation.assert_satisfied();
            }
            assert_recovery_requests(&server, before_flush, "text-flush", true);

            let requests = server.requests();
            let auxiliary: Vec<_> = requests
                .iter()
                .filter(|request| request.path == "/v1/messages" && request.body.as_ref().is_some_and(|body| body.get("tools").and_then(serde_json::Value::as_array).map_or(0, Vec::len) < 2))
                .filter_map(|request| request.body.as_ref())
                .collect();
            let recovered_text = auxiliary
                .iter()
                .filter(|body| body["model"].as_str().is_some_and(|model| model.starts_with("text-")) && wire_image_count(body) == 0)
                .collect::<Vec<_>>();
            assert!(recovered_text.len() >= 4, "coordination, btw, recap and flush each reach text after recovery");
            assert!(recovered_text.iter().all(|body| body.to_string().contains("E42")));

            // The user Surface and its original image remain unresolved by the
            // primary owner even though sideband consumers recovered independently.
            let timeline = shell::session::storage::load_timeline_by_id_at(
                target.0.as_ref(), client.sandbox().grow_home(),
            ).unwrap().unwrap();
            assert!(!timeline.events().iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )), "auxiliary requests must not project images onto the primary Timeline");
            assert!(timeline.surface().iter().any(|item| {
                sampling_types::conversation::conversation_image_groups(std::slice::from_ref(item)).len() > 0
            }), "the original image remains in the primary Surface");
            let memory_root = client.sandbox().grow_home().join("memory");
            let memory_file = files_below(&memory_root)
                .into_iter()
                .find(|path| path.extension().is_some_and(|extension| extension == "md"))
                .unwrap_or_else(|| panic!("flush should write an actual memory markdown file below {}", memory_root.display()));
            let memory_text = std::fs::read_to_string(&memory_file).unwrap();
            assert!(memory_text.contains("E42"), "written memory file records the mock flush response");

            let before_prompt_flush = server.requests().len();
            let headless_flush = server.expect_response(
                "headless /flush prompt writes recovered screenshot evidence",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn("## Prompt Flush Evidence\n\nACP_PROMPT_FLUSH_E42", "text-flush"),
            );
            client.prompt_with_timeout(&target, "/flush").await.unwrap();
            headless_flush.assert_satisfied();
            let prompt_flush_requests = server.requests();
            let prompt_flush_bodies: Vec<_> = prompt_flush_requests[before_prompt_flush..]
                .iter()
                .filter(|request| request.body.as_ref().is_some_and(|body| body.get("model").and_then(serde_json::Value::as_str) == Some("text-flush")))
                .filter_map(|request| request.body.as_ref())
                .collect();
            assert_eq!(prompt_flush_bodies.len(), 1, "cached image recovery needs one flush provider call");
            assert_eq!(prompt_flush_bodies[0]["model"], "text-flush");
            assert_eq!(wire_image_count(prompt_flush_bodies[0]), 0);
            assert!(prompt_flush_bodies[0].to_string().contains("E42"));
            let prompt_memory_file = files_below(&memory_root)
                .into_iter()
                .find(|path| {
                    path.extension().is_some_and(|extension| extension == "md")
                        && std::fs::read_to_string(path)
                            .is_ok_and(|text| text.contains("ACP_PROMPT_FLUSH_E42"))
                })
                .expect("headless /flush prompt must persist generated Markdown, not an empty acknowledgement");
            assert!(std::fs::read_to_string(prompt_memory_file).unwrap().contains("ACP_PROMPT_FLUSH_E42"));

            // A new process must replay Sideband history and retain the primary's
            // unprojected image. The durable JSONL is the evidence, not only the
            // old process's in-memory notification cache.
            client.close().await.unwrap();
            let sandbox = client.take_sandbox();
            drop(client);
            let mut reloaded = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
            init(&reloaded).await;
            reloaded.load_session_with_timeout(&target, workspace).await;
            let reloaded_timeline = shell::session::storage::load_timeline_by_id_at(
                target.0.as_ref(), reloaded.sandbox().grow_home(),
            ).unwrap().unwrap();
            let sideband_spawns = reloaded_timeline.events().iter().filter(|event| {
                matches!(&event.kind, chat_state::TimelineEventKind::Sideband(_))
            }).count();
            assert!(sideband_spawns >= 4, "Timeline JSONL reload retains all four sideband sources");
            let durable_purposes: std::collections::BTreeSet<_> = reloaded_timeline.events().iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Sideband(spawn) => Some(spawn.purpose.as_str()),
                    _ => None,
                })
                .collect();
            for purpose in ["info-request", "side-question", "session-recap", "memory-flush"] {
                assert!(durable_purposes.contains(purpose), "reloaded Timeline must preserve {purpose} sideband purpose");
            }
            let sideband_root = session_dir(reloaded.sandbox().grow_home(), target.0.as_ref()).join("sidebands");
            let sideband_ledgers: Vec<_> = files_below(&sideband_root)
                .into_iter()
                .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
                .collect();
            assert!(sideband_ledgers.len() >= 4, "each public auxiliary request persists its sideband JSONL ledger");
            assert!(sideband_ledgers.iter().any(|path| {
                std::fs::read_to_string(path).is_ok_and(|text| text.contains("E42"))
            }), "sideband JSONL ledger retains image recovery result text");
            let ledger_events: Vec<serde_json::Value> = sideband_ledgers.iter()
                .flat_map(|path| std::fs::read_to_string(path).unwrap().lines().map(str::to_owned).collect::<Vec<_>>())
                .map(|line| serde_json::from_str(&line).expect("valid durable sideband JSONL event"))
                .collect();
            let request_purposes: std::collections::BTreeSet<_> = ledger_events.iter()
                .filter(|event| event.pointer("/kind/type").and_then(serde_json::Value::as_str) == Some("request"))
                .filter_map(|event| event.pointer("/kind/event/purpose").and_then(serde_json::Value::as_str))
                .collect();
            for purpose in ["info-request", "side-question", "session-recap", "memory-flush"] {
                assert!(request_purposes.contains(purpose), "sideband ledgers must durably identify {purpose}");
            }
            assert!(ledger_events.iter().any(|event| event.pointer("/kind/type").and_then(serde_json::Value::as_str) == Some("attempt")
                && event.pointer("/kind/event/attempt_no").and_then(serde_json::Value::as_u64) == Some(1)),
                "durable image recovery ledger must include its successful attempt number");
            assert!(!reloaded_timeline.events().iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )));
            reloaded.close().await.unwrap();
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires a freshly built grow binary; all provider traffic is loopback"]
    async fn flush_model_override_resolves_cross_provider_route_for_image_recovery() {
        tokio::task::LocalSet::new().run_until(async {
            let primary_server = MockInferenceServer::start().await.unwrap();
            primary_server.set_settings(json!({"image_description_model":"mock/vision"}));
            let flush_server = MockInferenceServer::start().await.unwrap();
            let workdir = persistent_workspace();
            let workspace = workdir.path();
            let mut sandbox = TestSandbox::new();
            sandbox.set_env("FLUSH_API_KEY", "isolated-flush-key");
            let config = format!(
                r#"[models]
                default = "mock/vision"
                image_description = "mock/vision"
                session_title = "title/title"

                [provider.mock.options]
                base_url = {:?}
                env_key = "GROW_API_KEY"

                [provider.mock.models.vision]
                model = "vision"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.mock.models.text]
                model = "text"
                api_backend = "messages"
                context_window = 200000
                agent_type = "grow-build"

                [provider.title.options]
                base_url = {:?}
                env_key = "GROW_API_KEY"

                [provider.title.models.title]
                model = "title"
                api_backend = "responses"
                context_window = 200000
                agent_type = "grow-build"

                [provider.flushmock.options]
                base_url = {:?}
                env_key = "FLUSH_API_KEY"

                [provider.flushmock.models.target]
                model = "flush-target"
                api_backend = "chat_completions"
                context_window = 200000
                agent_type = "grow-build"

                [memory]
                enabled = true

                [compaction.memory_flush]
                enabled = true
                flush_model = "flushmock/target"
                "#,
                primary_server.url(),
                primary_server.url(),
                flush_server.url()
            );
            std::fs::write(sandbox.grow_home().join("config.toml"), config).unwrap();
            let mut client = GrowStdioClient::spawn_with_sandbox(
                &primary_server,
                workspace,
                sandbox,
            )
            .await;
            init(&client).await;
            let session = client.create_session_with_model_timeout(workspace, "mock/vision").await;
            let main = primary_server.expect_response(
                "multimodal primary accepts screenshot and remains unprojected",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("A screenshot shows build error E42.", "vision"),
            );
            client
                .prompt_with_content_timeout(
                    &session,
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("Inspect this screenshot.")),
                        acp::ContentBlock::Image(acp::ImageContent::new(
                            valid_png_base64(),
                            "image/png",
                        )),
                    ],
                )
                .await
                .unwrap();
            main.assert_satisfied();
            client.set_model_with_timeout(&session, "mock/text").await.unwrap();

            let first = flush_server.expect_response(
                "resolved flush model target rejects image input",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::json(400, json!({
                    "error":{"code":"1210","message":"messages.content.type 参数非法，取值范围 ['text']"}
                })),
            );
            let description = primary_server.expect_response(
                "configured visual description route resolves from primary provider",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_text_turn("A screenshot with E42 in a red error dialog.", "vision"),
            );
            let retry = flush_server.expect_response(
                "same flush override route retries without raw image",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::ChatCompletions),
                chat_text_turn("## Debugging\nThe screenshot showed error E42.", "flush-target"),
            );
            ext_with_timeout(&client, "grow/memory/flush", json!({ "session_id": session.0.as_ref() }))
                .await
                .unwrap_or_else(|error| panic!(
                    "cross-provider memory flush failed: {error:?}; notifications={:?}; stderr={}; primary={:?}; flush={:?}",
                    client.grow_notifications(),
                    client.stderr(),
                    request_summary(&primary_server),
                    request_summary(&flush_server)
                ));
            first.assert_satisfied();
            description.assert_satisfied();
            retry.assert_satisfied();

            let target_requests = flush_server.requests();
            let target_bodies: Vec<_> = target_requests
                .iter()
                .filter(|request| request.path == "/v1/chat/completions")
                .filter_map(|request| request.body.as_ref())
                .collect();
            assert_eq!(target_bodies.len(), 2, "only failed optimistic call and one retry use flush provider");
            assert_eq!(target_bodies[0]["model"], "flush-target");
            assert_eq!(wire_image_count(target_bodies[0]), 1);
            assert_eq!(target_bodies[1]["model"], "flush-target");
            assert_eq!(wire_image_count(target_bodies[1]), 0);
            assert!(target_bodies[1].to_string().contains("E42"));
            assert!(target_requests.iter().all(|request| request.authorization.as_deref() == Some("Bearer isolated-flush-key")));
            let primary_requests = primary_server.requests();
            let primary_foreground = primary_requests.iter().filter(|request| {
                request.path == "/v1/messages" && request.body.as_ref().is_some_and(|body| {
                    body.get("tools").and_then(serde_json::Value::as_array).map_or(0, Vec::len) >= 2
                })
            }).count();
            assert_eq!(primary_foreground, 1,
                "only the original user image turn reaches primary foreground; requests={:?}", request_summary(&primary_server));
            assert_eq!(primary_requests.iter().filter(|request| request.path == "/v1/messages").count(), 2,
                "primary route should receive one foreground image and one image-description transaction (request summaries show any unexpected extra call): {:?}",
                request_summary(&primary_server));
            let memory_file = files_below(&client.sandbox().grow_home().join("memory"))
                .into_iter()
                .find(|path| path.extension().is_some_and(|extension| extension == "md"))
                .expect("cross-provider Memory Flush writes a real Markdown file");
            assert!(std::fs::read_to_string(memory_file).unwrap().contains("E42"));
            client.close().await.unwrap();
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires a freshly built grow binary; tool and provider traffic are loopback/local"]
    async fn read_file_image_tool_evidence_recovers_on_text_sideband_route() {
        tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_settings(json!({"image_description_model":"mock/vision"}));
            let workdir = persistent_workspace();
            let workspace = workdir.path();
            let image_path = workspace.join("evidence.png");
            let image_bytes = base64::engine::general_purpose::STANDARD
                .decode(valid_png_base64())
                .unwrap();
            std::fs::write(&image_path, image_bytes).unwrap();
            let sandbox = TestSandbox::new();
            configure_journey_models(&sandbox, &server, false);
            let mut client = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
            init(&client).await;
            let session = client.create_session_with_model_timeout(workspace, "mock/vision").await;

            let read_call = server.expect_response(
                "visual model asks the real workspace read_file tool for a PNG",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                super::tools_reply(
                    InferenceEndpoint::Messages,
                    "vision",
                    &[("read-image-evidence", "read_file", &json!({
                        "target_file": image_path.to_string_lossy()
                    }).to_string())],
                ),
            );
            let visual_answer = server.expect_response(
                "visual model receives read_file's actual image tool result",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("I inspected the image returned by read_file; its pixels show E42.", "vision"),
            );
            client.prompt_with_timeout(&session, "Use read_file to inspect evidence.png.").await.unwrap();
            read_call.assert_satisfied();
            visual_answer.assert_satisfied();

            let foreground_bodies: Vec<_> = server.request_bodies().into_iter()
                .filter(|body| body.get("tools").is_some())
                .collect();
            assert_eq!(foreground_bodies.len(), 2, "tool call and visual follow-up are distinct provider turns");
            assert!(foreground_bodies[0].to_string().contains("read_file"));
            assert!(wire_image_count(&foreground_bodies[1]) > 0,
                "actual read_file tool result carries image media into the primary visual request: {:?}",
                foreground_bodies[1]);
            assert!(foreground_bodies[1].to_string().contains("evidence.png"),
                "the tool result retains its file source evidence");

            client.set_model_with_timeout(&session, "mock/text-btw").await.unwrap();
            let recovery = expect_unknown_route_recovery(&server, "read_file image evidence btw", "text-btw", true);
            let before = server.requests().len();
            let result = ext_with_timeout(&client, "grow/btw", json!({
                "sessionId": session.0.as_ref(),
                "question": "What does read_file's image evidence show?"
            })).await.unwrap();
            let result: serde_json::Value = serde_json::from_str(result.0.get()).unwrap();
            assert!(result["result"]["answer"].as_str().is_some_and(|answer| answer.contains("E42")), "read_file btw response={result}; requests={:?}; stderr={}", request_summary(&server), client.stderr());
            for expectation in recovery { expectation.assert_satisfied(); }
            assert_recovery_requests(&server, before, "text-btw", false);

            let timeline = shell::session::storage::load_timeline_by_id_at(
                session.0.as_ref(), client.sandbox().grow_home(),
            ).unwrap().unwrap();
            assert!(!timeline.events().iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )), "sideband image recovery does not project tool evidence onto the primary Timeline");
            assert!(timeline.surface().iter().any(|item| {
                sampling_types::conversation::conversation_image_groups(std::slice::from_ref(item)).len() > 0
            }), "primary Timeline still owns the original read_file image result");
            client.close().await.unwrap();
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires a freshly built grow binary; foreground/auxiliary model traffic is loopback"]
    async fn memory_flush_sideband_completes_while_foreground_request_is_held() {
        tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let workdir = persistent_workspace();
            let workspace = workdir.path();
            let sandbox = TestSandbox::new();
            configure_journey_models(&sandbox, &server, true);
            let mut client = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
            init(&client).await;
            let session = client.create_session_with_model_timeout(workspace, "mock/vision").await;

            let setup_turn = server.expect_response(
                "completed evidence exists before the held foreground turn",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("Keep durable memory evidence BUSY_FLUSH_EVIDENCE.", "vision"),
            );
            client
                .prompt_with_timeout(&session, "Record BUSY_FLUSH_EVIDENCE before the next step.")
                .await
                .unwrap();
            setup_turn.assert_satisfied();

            let mut foreground = server.expect_response_blocked(
                "foreground prompt remains held while a flush sideband runs",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_text_turn("The foreground turn completes after flush release.", "vision"),
            );
            {
                let prompt = client.prompt_with_timeout(&session, "Keep this foreground turn running.");
                tokio::pin!(prompt);
                tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    tokio::select! {
                        _ = foreground.wait_received() => {},
                        result = prompt.as_mut() => panic!("held foreground request completed early: {result:?}"),
                    }
                })
                .await
                .expect("foreground request reaches the mock provider and remains held");

                let flush = server.expect_response(
                    "memory flush sideband completes before foreground release",
                    InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                    messages_text_turn("## Busy Flush\n\nBUSY_FLUSH_SIDEBAND_WRITTEN", "vision"),
                );
                ext_with_timeout(
                    &client,
                    "grow/memory/flush",
                    json!({ "session_id": session.0.as_ref() }),
                )
                .await
                .unwrap();
                flush.assert_satisfied();
                assert!(
                    tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        prompt.as_mut()
                    )
                    .await
                    .is_err(),
                    "foreground turn must still be pending after sideband flush"
                );
                let memory_file = files_below(&client.sandbox().grow_home().join("memory"))
                    .into_iter()
                    .find(|path| {
                        path.extension().is_some_and(|extension| extension == "md")
                            && std::fs::read_to_string(path)
                                .is_ok_and(|text| text.contains("BUSY_FLUSH_SIDEBAND_WRITTEN"))
                    })
                    .expect("flush writes its actual generated Markdown while foreground is held");
                assert!(
                    std::fs::read_to_string(memory_file)
                        .unwrap()
                        .contains("BUSY_FLUSH_SIDEBAND_WRITTEN")
                );

                foreground.release();
                tokio::time::timeout(std::time::Duration::from_secs(15), prompt.as_mut())
                    .await
                    .expect("foreground turn completes after explicit release")
                    .unwrap();
                foreground.assert_satisfied();
            }
            client.close().await.unwrap();
        })
        .await;
    }
}
