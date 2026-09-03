//! Hermetic built-binary regression: protocol failures end a turn, not its
//! Session. Every response is constructed here and served over loopback by
//! MockInferenceServer; TestSandbox clears inherited credentials/config.
//!
//! cargo build -p cli --bin grow --offline
//! cargo test -p shell --test test_session_protocol_recovery --offline -- --ignored

#![cfg(unix)]

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
                        "\n[provider.mock.models.{name}]\nmodel = \"{name}\"\napi_backend = \"{backend}\"\ncontext_window = 200000\nagent_type = \"grow-build\"\nmax_retries = 2\n"
                    ));
                }
                std::fs::write(sandbox.grow_home().join("config.toml"), config).unwrap();
                let mut client = GrowStdioClient::spawn_with_sandbox(&server, workspace, sandbox).await;
                if let Err(panic) = std::panic::AssertUnwindSafe(client.initialize_with_timeout()).catch_unwind().await {
                    eprintln!("{}\n{}", client.process_diagnostics(), client.stderr());
                    std::panic::resume_unwind(panic);
                }
                let session = client
                    .create_session_with_model_timeout(workspace, &format!("mock/{model}"))
                    .await;
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
                let invalid = server.expect_response(
                    "one invalid tool rejects its entire response",
                    InferenceRequestMatcher::foreground(endpoint),
                    tools_reply(
                        endpoint,
                        model,
                        &[
                            ("call_sibling", &tool_name, &forbidden_args),
                            ("call_invalid", &tool_name, "{"),
                        ],
                    ),
                );
                let before = foreground_bodies(&server).len();
                let error = client
                    .prompt_with_timeout(&session, "execute the test step")
                    .await
                    .expect_err("invalid response must fail the turn");
                assert!(
                    format!("{error:?}").contains("protocol:"),
                    "{model}: {error:?}"
                );
                retained.assert_satisfied();
                invalid.assert_satisfied();
                assert_eq!(terminal_model_failure_count(&client), 1, "one final notice through the existing retry-failure channel");
                assert_eq!(
                    foreground_bodies(&server).len(),
                    before + 2,
                    "no protocol-error retry with the real retry budget"
                );

                // Use actual ACP admission/control paths. Never reset Idle, swap
                // conversation items, or manually invoke handle_completion.
                client
                    .prompt_with_timeout(&session, "continue safely")
                    .await
                    .unwrap();
                assert_clean_history(&server, workspace, model);
                assert_eq!(terminal_model_failure_count(&client), 1, "successful continuation must not warn again");
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
                reloaded.initialize_with_timeout().await;
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
        })
        .await;
}
