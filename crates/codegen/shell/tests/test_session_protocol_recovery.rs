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
