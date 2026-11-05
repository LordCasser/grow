//! Built-binary ACP cancellation/retry milestone. All inference is constructed
//! here and served over loopback. Shell generates its usual random RequestIds;
//! reused-ID isolation is covered separately by sampler's public Handle tests.
//!
//! GROW_BINARY=/absolute/path/to/grow cargo test --locked --offline -p shell \
//!   --test test_sampler_request_lifecycle -- --ignored --test-threads=1

#![cfg(unix)]

use std::{collections::HashSet, path::Path, time::Duration};

use chat_state::{RequestEvent, TimelineEvent, TimelineEventKind, TurnEvent};
use serde_json::{Value, json};
use test_support::{
    InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer, RawStdioClient,
    ScriptedResponse, SseEvent, TestSandbox,
};

const MODEL: &str = "sampler-lifecycle";
const CANCELLED_CALL: &str = "call_cancelled_candidate";

fn text_response(text: &str) -> ScriptedResponse {
    ScriptedResponse::sse(vec![
        SseEvent::data(json!({
            "id":"lifecycle-response","object":"chat.completion.chunk","created":0,"model":MODEL,
            "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        }).to_string()),
        SseEvent::data("[DONE]"),
    ])
}

async fn rpc(client: &mut RawStdioClient, method: &str, params: Value) -> Value {
    let id = uuid::Uuid::now_v7().to_string();
    client
        .send_line(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string())
        .await;
    let response = client
        .response_for_id(&id, method, Duration::from_secs(30))
        .await;
    assert!(
        response.get("error").is_none(),
        "{method}: {response}\n{}",
        client.stderr()
    );
    response["result"].clone()
}

async fn prompt(client: &mut RawStdioClient, session: &str, text: &str) -> Value {
    rpc(
        client,
        "session/prompt",
        json!({
            "sessionId":session,"prompt":[{"type":"text","text":text}]
        }),
    )
    .await
}

async fn control_epoch(client: &mut RawStdioClient, session: &str) -> String {
    rpc(
        client,
        "_grow/queue/prompt_status",
        json!({
            "sessionId":session,"promptId":"sampler-lifecycle-observation"
        }),
    )
    .await["controlEpoch"]
        .as_str()
        .expect("resident owner epoch")
        .to_owned()
}

fn foreground_requests(server: &MockInferenceServer) -> Vec<test_support::mock_server::LogEntry> {
    server
        .requests()
        .into_iter()
        .filter(|request| {
            request.path == "/v1/chat/completions"
                && request.body.as_ref().is_some_and(|body| {
                    body.get("tools")
                        .and_then(Value::as_array)
                        .is_some_and(|tools| tools.len() >= 2)
                })
        })
        .collect()
}

fn timeline(grow_home: &Path, session: &str) -> Vec<TimelineEvent> {
    shell::session::storage::load_timeline_by_id_at(session, grow_home)
        .expect("valid persisted session and sampling artifacts")
        .expect("session history exists")
        .events()
        .to_vec()
}

#[tokio::test]
#[ignore = "requires the accepted grow binary via absolute GROW_BINARY; no external service"]
async fn native_cancel_then_retry_preserves_the_resident_session_and_attempt_settlement() {
    let binary = std::env::var_os("GROW_BINARY").expect("explicit prebuilt GROW_BINARY");
    assert!(
        Path::new(&binary).is_absolute(),
        "GROW_BINARY must be absolute"
    );
    let server = MockInferenceServer::start().await.unwrap();
    server.set_response("LOCAL_AUXILIARY_RESPONSE");
    let mut sandbox = TestSandbox::new();
    let workspace = sandbox.workspace().to_path_buf();
    let grow_home = sandbox.grow_home().to_path_buf();
    let root = sandbox.root().to_path_buf();
    for name in ["hooks", "git-template", "xdg-config", "xdg-cache"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    let gitconfig = root.join("gitconfig");
    std::fs::write(&gitconfig, "").unwrap();
    sandbox
        .set_env("GIT_CONFIG_GLOBAL", &gitconfig)
        .set_env("GIT_CONFIG_SYSTEM", &gitconfig)
        .set_env("GIT_CONFIG_COUNT", "2")
        .set_env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .set_env("GIT_CONFIG_VALUE_0", root.join("hooks"))
        .set_env("GIT_CONFIG_KEY_1", "init.templateDir")
        .set_env("GIT_CONFIG_VALUE_1", root.join("git-template"))
        .set_env("GIT_TEMPLATE_DIR", root.join("git-template"))
        .set_env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .set_env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .set_env("RUST_MIN_STACK", "16777216");
    let initialized = sandbox
        .git_command()
        .arg("init")
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "isolated git init failed: {initialized:?}"
    );
    std::fs::write(
        grow_home.join("config.toml"),
        format!(
            "[models]\ndefault = \"mock/{MODEL}\"\nsession_title = \"mock/{MODEL}\"\n\
         [provider.mock]\napi_backend = \"chat_completions\"\n\
         [provider.mock.options]\nbase_url = {:?}\nenv_key = \"GROW_API_KEY\"\n\
         [provider.mock.models.{MODEL}]\nmodel = \"{MODEL}\"\ncontext_window = 200000\n\
         agent_type = \"grow-build\"\nmax_retries = 3\n",
            server.url()
        ),
    )
    .unwrap();
    let mut client = RawStdioClient::spawn_with_sandbox(&server, &workspace, sandbox).await;
    let pid = client.child_pid().expect("CLI child pid");
    rpc(&mut client, "initialize", json!({
        "protocolVersion":1,"clientCapabilities":{"fs":{},"terminal":false},
        "_meta":{"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true},
            "clientType":"sampler-lifecycle-test","clientVersion":"0.0.0-test","samplingAttemptLifecycle":true}
    })).await;
    rpc(
        &mut client,
        "authenticate",
        json!({"methodId":"provider.api_key","_meta":{"headless":true}}),
    )
    .await;
    let session = rpc(
        &mut client,
        "session/new",
        json!({
            "cwd":workspace,"mcpServers":[],"_meta":{"modelId":format!("mock/{MODEL}")}
        }),
    )
    .await["sessionId"]
        .as_str()
        .expect("new session id")
        .to_owned();
    let epoch = control_epoch(&mut client, &session).await;
    let matcher = InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions);

    let prime = server.expect_response(
        "normal turn before cancellation",
        matcher,
        text_response("PRIME_ACCEPTED"),
    );
    assert_eq!(
        prompt(&mut client, &session, "KEEP_HISTORY").await["stopReason"],
        "end_turn"
    );
    prime.assert_satisfied();
    let first = foreground_requests(&server);
    assert_eq!(first.len(), 1);
    let tool_name = first[0].body.as_ref().unwrap()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|tool| {
            let function = &tool["function"];
            function.pointer("/parameters/properties/command")?;
            function["name"].as_str().map(str::to_owned)
        })
        .expect("actual wire exposes a terminal tool");
    let arguments = json!({"command":"touch must-not-execute.txt","description":"cancelled candidate must not run","background":false}).to_string();
    let mut cancelled = server.expect_response_blocked("cancel before candidate terminal", matcher, ScriptedResponse::sse(vec![
        SseEvent::data(json!({
            "id":"cancelled-response","object":"chat.completion.chunk","created":0,"model":MODEL,
            "choices":[{"index":0,"delta":{"content":"OLD_CANDIDATE","tool_calls":[{
                "index":0,"id":CANCELLED_CALL,"type":"function","function":{"name":tool_name,"arguments":arguments}
            }]},"finish_reason":null}]
        }).to_string()),
        SseEvent::data(json!({
            "id":"cancelled-response","object":"chat.completion.chunk","created":0,"model":MODEL,
            "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        }).to_string()),
    ]));
    let cancelled_prompt = uuid::Uuid::now_v7().to_string();
    client
        .send_line(
            &json!({"jsonrpc":"2.0","id":cancelled_prompt,"method":"session/prompt",
        "params":{"sessionId":session,"prompt":[{"type":"text","text":"CANCEL_THIS_TURN"}]}})
            .to_string(),
        )
        .await;
    tokio::time::timeout(Duration::from_secs(20), cancelled.wait_received())
        .await
        .expect("HTTP request received before cancellation");
    tokio::time::timeout(Duration::from_secs(20), cancelled.wait_blocked())
        .await
        .expect("old response terminal barrier reached");
    assert_eq!(foreground_requests(&server).len(), 2);
    client
        .send_line(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{
                "sessionId":session,"_meta":{"cancelTrigger":"user","cancelSubagents":false}
            }})
            .to_string(),
        )
        .await;
    let response = client
        .response_for_id(
            &cancelled_prompt,
            "cancelled prompt terminal",
            Duration::from_secs(30),
        )
        .await;
    assert!(
        response.get("error").is_none(),
        "{response}\n{}",
        client.stderr()
    );
    assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
    assert_eq!(
        foreground_requests(&server).len(),
        2,
        "cancel cannot resample the old turn"
    );
    // The old HTTP writer may already have been dropped by transport cancellation.
    // Release is cleanup, never proof of a late-response or reused-ID race.
    cancelled.release();
    assert!(!workspace.join("must-not-execute.txt").exists());
    let after_cancel = timeline(&grow_home, &session);
    let cancelled_end = after_cancel.iter().filter(|event| matches!(&event.kind,
        TimelineEventKind::Turn(TurnEvent::Ended { terminal, tool_count: 0, .. }) if terminal.stop_reason == "cancelled")).count();
    assert_eq!(
        cancelled_end, 1,
        "the cancelled turn must durably terminate"
    );
    eprintln!("native ACP cancel settled: stopReason=cancelled wire=2 durable_cancelled_turns=1");

    let transient = server.expect_response(
        "one transient provider attempt",
        matcher,
        ScriptedResponse::json(
            503,
            json!({"error":{"type":"api_error","message":"constructed transient overload"}}),
        ),
    );
    let recovered = server.expect_response(
        "same prompt retries successfully",
        matcher,
        text_response("RETRY_ACCEPTED"),
    );
    assert_eq!(
        prompt(&mut client, &session, "RETRY_AFTER_CANCEL").await["stopReason"],
        "end_turn"
    );
    transient.assert_satisfied();
    recovered.assert_satisfied();
    assert_eq!(
        foreground_requests(&server).len(),
        4,
        "new turn performs exactly 503 then success"
    );
    let normal = server.expect_response(
        "normal follow-up after retry",
        matcher,
        text_response("FOLLOWUP_ACCEPTED"),
    );
    assert_eq!(
        prompt(&mut client, &session, "NORMAL_AFTER_RETRY").await["stopReason"],
        "end_turn"
    );
    normal.assert_satisfied();
    assert_eq!(control_epoch(&mut client, &session).await, epoch);
    assert_eq!(client.child_pid(), Some(pid));

    let requests = foreground_requests(&server);
    assert_eq!(
        requests.len(),
        5,
        "no cancelled-turn dispatch leaks into later prompts"
    );
    let last_history = requests.last().unwrap().body.as_ref().unwrap()["messages"].to_string();
    for retained in [
        "KEEP_HISTORY",
        "PRIME_ACCEPTED",
        "CANCEL_THIS_TURN",
        "RETRY_AFTER_CANCEL",
        "RETRY_ACCEPTED",
        "NORMAL_AFTER_RETRY",
    ] {
        assert!(
            last_history.contains(retained),
            "missing retained history: {retained}"
        );
    }
    for rejected in [
        CANCELLED_CALL,
        "OLD_CANDIDATE",
        "must-not-execute.txt",
        "constructed transient overload",
    ] {
        assert!(
            !last_history.contains(rejected),
            "rejected candidate polluted history: {rejected}"
        );
    }
    assert!(!workspace.join("must-not-execute.txt").exists());
    let events = timeline(&grow_home, &session);
    let mut terminals = Vec::new();
    let mut settlements = Vec::new();
    let mut evidence = Vec::new();
    let mut completed = Vec::new();
    let mut admissions = Vec::new();
    let mut request_ids = HashSet::new();
    let mut cancelled_requests = Vec::new();
    let mut retries = 0;
    for event in &events {
        match &event.kind {
            TimelineEventKind::Messages(message) if message.response_admission.is_some() => {
                let admission = message.response_admission.as_ref().unwrap();
                assert_eq!(admission.quarantined_tool_exchanges, 0);
                admissions.push((
                    admission.identity.request_id.as_str(),
                    admission.identity.attempt,
                    serde_json::to_string(&message.items).unwrap(),
                ));
            }
            TimelineEventKind::Turn(TurnEvent::Ended {
                terminal,
                tool_count,
                ..
            }) => {
                assert_eq!(*tool_count, 0);
                terminals.push(terminal.stop_reason.as_str());
            }
            TimelineEventKind::Request(RequestEvent::Started { id, .. }) => {
                assert!(request_ids.insert(id.clone()));
            }
            TimelineEventKind::Request(RequestEvent::Completed { id, attempt, .. }) => {
                completed.push((id.as_str(), *attempt))
            }
            TimelineEventKind::Request(RequestEvent::Cancelled { id, .. }) => {
                cancelled_requests.push(id.as_str())
            }
            TimelineEventKind::Request(RequestEvent::Retrying { .. }) => retries += 1,
            TimelineEventKind::Request(RequestEvent::Failed { .. }) => {
                panic!("no terminal request failure expected: {event:?}")
            }
            TimelineEventKind::Tool(_) => {
                panic!("cancelled candidate cannot dispatch a tool: {event:?}")
            }
            TimelineEventKind::Observation(observation)
                if observation.scope == "sampling_usage"
                    && observation.name == "attempt_settled" =>
            {
                settlements.push(observation.data.as_ref().unwrap());
            }
            TimelineEventKind::Observation(observation)
                if observation.scope == "sampling_evidence" =>
            {
                evidence.push((
                    observation.name.as_str(),
                    observation.data.as_ref().unwrap(),
                ));
            }
            _ => {}
        }
    }
    let request_events: Vec<_> = events
        .iter()
        .filter(|event| matches!(event.kind, TimelineEventKind::Request(_)))
        .collect();
    eprintln!(
        "request lifecycle: {}",
        serde_json::to_string(&request_events).unwrap()
    );
    eprintln!(
        "canonical requests: completed={completed:?} cancelled={cancelled_requests:?}; settlements={}",
        serde_json::to_string(&settlements).unwrap()
    );
    let evidence_summary: Vec<_> = evidence
        .iter()
        .map(|(name, value)| {
            json!({
                "kind":name,"request_id":value["owner"]["request_id"],"metadata":value["metadata"]
            })
        })
        .collect();
    eprintln!(
        "attempt evidence: {}",
        serde_json::to_string(&evidence_summary).unwrap()
    );
    rpc(
        &mut client,
        "_grow/session/close",
        json!({"sessionId":session}),
    )
    .await;
    let status = client.close().await.expect("reap CLI process tree");
    eprintln!(
        "CLI child reaped: pid={pid} status={status}; {}",
        client.process_diagnostics()
    );
    assert_eq!(terminals, ["end_turn", "cancelled", "end_turn", "end_turn"]);
    assert_eq!(
        request_ids.len(),
        4,
        "Shell assigns one random RequestId per logical sample"
    );
    assert_eq!(
        completed
            .iter()
            .map(|(_, attempt)| *attempt)
            .collect::<Vec<_>>(),
        [1, 2, 1]
    );
    assert_eq!(cancelled_requests.len(), 1);
    assert_eq!(retries, 1);
    assert_eq!(admissions.len(), 3);
    for ((id, attempt, items), ((completed_id, completed_attempt), text)) in
        admissions.iter().zip(completed.iter().zip([
            "PRIME_ACCEPTED",
            "RETRY_ACCEPTED",
            "FOLLOWUP_ACCEPTED",
        ]))
    {
        assert_eq!((*id, *attempt), (*completed_id, *completed_attempt));
        assert!(
            items.contains(text),
            "completed response must be canonical: {text}"
        );
    }
    let keys: HashSet<_> = settlements
        .iter()
        .map(|value| value["attempt_key"].as_str().unwrap())
        .collect();
    assert_eq!(settlements.len(), 5);
    assert_eq!(keys.len(), 5, "each actual attempt settles once");
    let expected_keys: HashSet<_> = completed
        .iter()
        .flat_map(|(id, attempts)| (1..=*attempts).map(move |attempt| format!("{id}:{attempt}")))
        .chain(cancelled_requests.iter().map(|id| format!("{id}:1")))
        .collect();
    assert_eq!(
        keys.iter()
            .map(|key| (*key).to_owned())
            .collect::<HashSet<_>>(),
        expected_keys
    );
    let known: Vec<_> = settlements
        .iter()
        .filter(|value| !value["usage"].is_null())
        .collect();
    assert_eq!(
        known.len(),
        3,
        "cancelled and HTTP 503 attempts have unknown spend"
    );
    for id in [cancelled_requests[0], completed[1].0] {
        let key = format!("{id}:1");
        let settlement = settlements
            .iter()
            .find(|value| value["attempt_key"] == key)
            .unwrap();
        assert!(
            settlement["usage"].is_null(),
            "unreported usage must remain unknown: {key}"
        );
    }
    for settlement in known {
        assert_eq!(settlement["usage"]["total_tokens"], 15);
    }
    let foreground_evidence: Vec<_> = evidence
        .iter()
        .filter(|(_, value)| {
            value["owner"]["request_id"]
                .as_str()
                .is_some_and(|id| request_ids.contains(id))
        })
        .collect();
    let response_evidence: Vec<_> = foreground_evidence
        .iter()
        .filter(|(name, _)| *name == "response")
        .map(|(_, value)| *value)
        .collect();
    assert_eq!(response_evidence.len(), 5);
    assert_eq!(
        foreground_evidence
            .iter()
            .filter(|(name, _)| *name == "request")
            .count(),
        5
    );
    assert_eq!(
        foreground_evidence
            .iter()
            .filter(|(name, _)| *name == "retry")
            .count(),
        1
    );
    let evidence_keys: HashSet<_> = response_evidence
        .iter()
        .map(|value| {
            format!(
                "{}:{}",
                value["owner"]["request_id"].as_str().unwrap(),
                value["metadata"]["attempt_number"].as_u64().unwrap()
            )
        })
        .collect();
    assert_eq!(evidence_keys, expected_keys);
    let cancelled_evidence = response_evidence
        .iter()
        .find(|value| value["owner"]["request_id"].as_str() == Some(cancelled_requests[0]))
        .unwrap();
    assert_eq!(
        cancelled_evidence["metadata"]["outcome"]["details"]["cancelled"],
        true
    );
    assert!(
        cancelled_evidence["metadata"]["provider_terminal"].is_null(),
        "cancel cannot fabricate a provider terminal"
    );
    assert_eq!(
        response_evidence
            .iter()
            .filter(|value| !value["metadata"]["provider_terminal"].is_null())
            .count(),
        3
    );
    assert_eq!(
        response_evidence
            .iter()
            .filter(|value| value["metadata"]["status"] == 503)
            .count(),
        1
    );
    eprintln!(
        "milestone: wire=5 cancelled=1 end_turn=3 retry=1 settled=5 known=3 incomplete=2 response_evidence=5; same_session={session} pid={pid}"
    );
}
