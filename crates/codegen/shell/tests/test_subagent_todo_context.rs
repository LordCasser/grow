//! Real CLI TODO -> spawn/resume -> first child request, using only loopback providers.
//! Requires an explicit current GROW_BINARY; the harness must never build a fallback.
//! Authored Task prompts and canonical delegation bindings are verified without plan injection.

use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_transport::protocol as acp;
use chat_state::{
    MessageCause, SubagentEvent, SubagentOutcome, SubagentSpawnEvent, Timeline, TimelineEventKind,
};
use serde_json::{Value, json};
use test_support::mock_server::LogEntry;
use test_support::sse::chat_completions_reasoning_then_tool_call_events;
use test_support::*;

const GUIDANCE: &str = "Current planning context (summary; guidance, not permission).";
const TASK_PROMPT: &str = "Return a short completion. Use only this test's scripted provider; no tools or external effects.";
const STEP_A: &str = "fresh-parent-step-a";
const STEP_B: &str = "resumed-parent-step-b";
const OBJECTIVE_A: &str =
    "Fresh objective A: inspect \"本次委派\".\nPreserve the actual parent binding.";
const OBJECTIVE_B: &str =
    "Current objective B: verify \"恢复委派\".\nUse this new step, not source A.";
const REQUIREMENTS_A: &[&str] = &[
    "Fresh acceptance: retain A's identity.",
    "记录首请求中的 \"A\"。\nNo inferred IDs.",
];
const REQUIREMENTS_B: &[&str] = &[
    "Resumed acceptance: bind the current B task.",
    "记录首请求中的 \"B\"。\nNo inherited binding.",
];

fn expect_tool(
    server: &MockInferenceServer,
    call_id: &str,
    name: &str,
    arguments: Value,
) -> InferenceExpectation {
    server.expect_response(
        call_id,
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
            "Execute this isolated fixture's native tool.",
            call_id,
            name,
            &arguments.to_string(),
            "parent-wire",
        )),
    )
}

fn child_completion() -> ScriptedResponse {
    // A single terminal event keeps the barrier before ALL response content,
    // so neither a finish_reason nor a tool dispatch can overtake the capture.
    ScriptedResponse::sse(vec![SseEvent::data(json!({
        "id":"child-context-completion", "object":"chat.completion.chunk", "created":0,
        "model":"child-wire",
        "choices":[{"index":0,"delta":{"role":"assistant","content":"The isolated child completed normally."},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15},
    }).to_string())])
}

fn foreground_requests(server: &MockInferenceServer) -> Vec<LogEntry> {
    server
        .requests()
        .into_iter()
        .filter(|entry| {
            if entry.method != "POST" || entry.path != "/v1/chat/completions" {
                return false;
            }
            // Use the mock's actual foreground/auxiliary classification, including
            // its request-id precedence. Model/session names cannot match scripts.
            if entry
                .header("x-grow-turn-idx")
                .is_some_and(|value| !value.is_empty())
            {
                return true;
            }
            if entry
                .header("x-grow-req-id")
                .is_some_and(|value| !value.is_empty())
            {
                return false;
            }
            entry.body.as_ref().is_some_and(|body| {
                body["tools"]
                    .as_array()
                    .is_some_and(|tools| tools.len() >= 2)
            })
        })
        .collect()
}

fn timeline(grow_home: &Path, session_id: &str) -> (PathBuf, Timeline) {
    uuid::Uuid::parse_str(session_id).expect("read only a canonical ID created by this test");
    let paths: Vec<_> = std::fs::read_dir(grow_home.join("sessions"))
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .join(session_id)
                .join("timeline.jsonl")
        })
        .filter(|path| path.is_file())
        .collect();
    assert_eq!(
        paths.len(),
        1,
        "one canonical session in the private GROW_HOME"
    );
    let bytes = std::fs::read_to_string(&paths[0]).unwrap();
    let mut events = Vec::new();
    for (seq, line) in bytes.lines().enumerate() {
        let record: Value = serde_json::from_str(line).unwrap();
        assert_eq!(record["version"], 1);
        assert_eq!(record["seq"], seq as u64);
        match record["fact"]["kind"].as_str() {
            Some("domain") => {
                events.push(serde_json::from_value(record["fact"]["event"].clone()).unwrap());
            }
            Some("presentation") => {}
            kind => panic!("unexpected physical journal fact {kind:?}"),
        }
    }
    (
        paths.into_iter().next().unwrap(),
        Timeline::from_events(events).expect("all physical domain facts must pass the real fold"),
    )
}

fn spawn_for_call<'a>(parent: &'a Timeline, call_id: &str) -> (u64, &'a SubagentSpawnEvent) {
    let spawns: Vec<_> = parent
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                if spawn.origin_tool_call_id.as_deref() == Some(call_id) =>
            {
                Some((event.seq.get(), spawn.as_ref()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        spawns.len(),
        1,
        "the actual tool must persist exactly one Spawn"
    );
    spawns[0]
}

fn assert_first_request(
    body: &Value,
    parent_id: &str,
    parent: &Timeline,
    child: &Timeline,
    spawn_seq: u64,
    spawn: &SubagentSpawnEvent,
    step_id: &str,
) {
    assert_eq!(body["model"], "child-wire");
    assert_no_runtime_value(body);
    let messages = body["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message["content"].to_string().contains(TASK_PROMPT))
    );
    assert!(
        !body["messages"]
            .to_string()
            .contains(sampling_types::RECEIVE_AGENT_MESSAGE_TOOL_NAME)
    );
    assert!(child.planning_snapshot().is_none());
    assert_eq!(spawn.parent_step_id.as_deref(), Some(step_id));
    let snapshot = parent.planning_snapshot().unwrap();
    let link = snapshot
        .plan
        .links
        .iter()
        .find(|link| link.id == spawn.subagent_id)
        .unwrap();
    assert_eq!(link.parent_step_id, step_id);
    assert_eq!(link.child_session_id, spawn.child_session_id);
    assert!(!link.required && !link.terminal);
    let first_link_seq = parent
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Planning(planning)
                if planning
                    .plan
                    .links
                    .iter()
                    .any(|link| link.id == spawn.subagent_id) =>
            {
                Some(event.seq.get())
            }
            _ => None,
        })
        .unwrap();
    assert!(
        first_link_seq < spawn_seq,
        "Link must be durable before the actual Spawn"
    );
    let seeds: Vec<_> = child
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::SubagentSeed(seed) => Some(seed),
            _ => None,
        })
        .collect();
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].parent_timeline_id, parent_id);
    assert_eq!(seeds[0].security_parent_session_id, parent_id);
    assert_eq!(seeds[0].parent_spawn_seq, spawn_seq);
    assert_eq!(seeds[0].subagent_id, spawn.subagent_id);
    assert!(
        !child
            .events()
            .iter()
            .any(|event| matches!(&event.kind, TimelineEventKind::SubagentResult(_))),
        "the first request is captured before its canonical result exists"
    );
    assert_no_runtime_persistence(child);
}

fn assert_no_runtime_value(value: &Value) {
    match value {
        Value::String(text) => {
            assert!(
                !text.contains(GUIDANCE),
                "new runtime GUIDANCE was persisted"
            );
            assert!(
                !(text.contains("\"read_entry\"") && text.contains("\"associated_step\"")),
                "new runtime JSON was persisted without its GUIDANCE"
            );
        }
        Value::Array(values) => values.iter().for_each(assert_no_runtime_value),
        Value::Object(object) => {
            assert!(
                !(object.contains_key("read_entry")
                    && object.contains_key("child_session_id")
                    && object.contains_key("parent")),
                "new runtime structure was persisted"
            );
            object.values().for_each(assert_no_runtime_value);
        }
        _ => {}
    }
}

fn assert_no_runtime_persistence(child: &Timeline) {
    // Authored prompts may legitimately contain A/B's same step text.
    // Only the new runtime GUIDANCE/structure is forbidden in persistent history.
    assert_no_runtime_value(&serde_json::to_value(child.events()).unwrap());
    assert_no_runtime_value(&serde_json::to_value(child.surface()).unwrap());
    for event in child.events() {
        if let TimelineEventKind::Messages(messages) = &event.kind
            && messages.cause == MessageCause::MemoryContext
        {
            assert_no_runtime_value(&serde_json::to_value(&messages.items).unwrap());
        }
    }
}

fn assert_completed(parent: &Timeline, child: &Timeline, spawn: &SubagentSpawnEvent) {
    let results: Vec<_> = child
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::SubagentResult(result) => Some((event.seq.get(), result)),
            _ => None,
        })
        .collect();
    assert_eq!(
        results.len(),
        1,
        "normal execution owns one canonical child Result"
    );
    let (seq, result) = results[0];
    assert_eq!(result.subagent_id, spawn.subagent_id);
    assert_eq!(result.outcome, SubagentOutcome::Completed);
    assert!(result.error.is_none());
    let endings: Vec<_> = parent
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(ended))
                if ended.subagent_id == spawn.subagent_id =>
            {
                Some(ended)
            }
            _ => None,
        })
        .collect();
    assert_eq!(endings.len(), 1);
    let ended = endings[0];
    assert_eq!(ended.child_session_id, spawn.child_session_id);
    assert_eq!(ended.outcome, result.outcome);
    assert_eq!(ended.error, result.error);
    assert_eq!(ended.duration_ms, result.duration_ms);
    assert_eq!(ended.tool_calls, result.tool_calls);
    assert_eq!(ended.turns, result.turns);
    assert_eq!(ended.tokens_used, result.tokens_used);
    let reference = ended
        .result_ref
        .as_ref()
        .expect("real parent terminal must cite child Result");
    assert_eq!(reference.timeline_id, spawn.child_session_id);
    assert_eq!(reference.first_seq, seq);
    assert_eq!(reference.last_seq, seq);
    let plan = &parent.planning_snapshot().unwrap().plan;
    let link = plan
        .links
        .iter()
        .find(|link| link.id == spawn.subagent_id)
        .unwrap();
    assert!(link.terminal && !link.required);
    assert_eq!(link.child_session_id, spawn.child_session_id);
    assert_eq!(
        serde_json::from_str::<chat_state::TimelineRangeRef>(link.result_id.as_deref().unwrap())
            .unwrap(),
        *reference
    );
    assert_no_runtime_persistence(child);
}

#[tokio::test]
#[ignore = "requires the current GROW_BINARY; only loopback providers and test-owned state"]
async fn native_todo_spawn_and_resume_keep_binding_without_injecting_plan_context() {
    tokio::task::LocalSet::new().run_until(async {
        // Fail before the shared harness can choose or build any fallback binary.
        let binary = std::env::var("GROW_BINARY").expect("set GROW_BINARY to the current build");
        let binary = std::fs::canonicalize(binary).expect("GROW_BINARY must exist");
        assert!(binary.is_file());
        // Matchers cannot select a model/session: parent and child must have
        // independent endpoints, or a child can consume the next parent script.
        let parent_server = MockInferenceServer::start_with_models(vec![MockModelEntry::with_agent_type("parent-wire", "grow-build")]).await.unwrap();
        let child_server = MockInferenceServer::start_with_models(vec![MockModelEntry::with_agent_type("child-wire", "grow-build")]).await.unwrap();
        assert_ne!(parent_server.url(), child_server.url());
        parent_server.set_response("The isolated delegation completed.");
        child_server.set_response("The isolated child completed normally.");
        let sandbox = TestSandbox::builder().git().mock_url(parent_server.url()).build();
        let workspace = sandbox.workspace().to_path_buf();
        let grow_home = sandbox.grow_home().to_path_buf();
        for path in [sandbox.home(), sandbox.grow_home(), sandbox.workspace(), sandbox.temp_dir()] {
            assert!(path.starts_with(sandbox.root()));
        }
        std::fs::write(grow_home.join("config.toml"), format!(r#"[models]
default = "mock/parent"
[provider.mock]
api_backend = "chat_completions"
[provider.mock.options]
base_url = {:?}
env_key = "GROW_API_KEY"
[provider.mock.models.parent]
model = "parent-wire"
context_window = 200000
agent_type = "grow-build"
[provider.child]
api_backend = "chat_completions"
[provider.child.options]
base_url = {:?}
env_key = "GROW_API_KEY"
[provider.child.models.context]
model = "child-wire"
context_window = 200000
agent_type = "grow-build"
"#, parent_server.url(), child_server.url())).unwrap();
        let mut client = GrowStdioClient::spawn_with_sandbox(&parent_server, &workspace, sandbox).await;
        client.initialize_with_timeout().await;
        let session = client.create_session_with_model_timeout(&workspace, "mock/parent").await;
        let parent_id = session.0.as_ref();
        let mut source: Option<(SubagentSpawnEvent, PathBuf, Vec<u8>)> = None;
        let mut completed_spawns = Vec::new();
        for (index, (case, step_id, objective, requirements)) in [
            ("fresh", STEP_A, OBJECTIVE_A, REQUIREMENTS_A),
            ("resume", STEP_B, OBJECTIVE_B, REQUIREMENTS_B),
        ].into_iter().enumerate() {
            let todos = if source.is_none() {
                let mut todos: Vec<_> = (0..16).map(|index| json!({
                    "id":format!("neighbor-{index}"), "content":format!("Unrelated sibling objective {index}"), "status":"pending"
                })).collect();
                todos.push(json!({"id":step_id,"content":objective,"requirements":requirements,"priority":"high","status":"in_progress"}));
                todos.push(json!({"id":"tail-neighbor","content":"Keep an overall-plan neighbor after A","status":"pending"}));
                todos
            } else {
                vec![json!({"id":STEP_A,"status":"completed"}),
                    json!({"id":step_id,"content":objective,"requirements":requirements,"priority":"high","status":"in_progress"})]
            };
            let todo_call = format!("call_{case}_todo");
            let spawn_call = format!("call_{case}_spawn");
            let todo = expect_tool(&parent_server, &todo_call, "todo_write", json!({"todos":todos}));
            let mut arguments = json!({
                "parent_step_id":step_id,"description":format!("Verify {case} parent context"),
                "prompt":TASK_PROMPT,
                "subagent_type":"general-purpose","background":false,"required_deliverable":false,"isolation":"none"
            });
            if let Some((source_spawn, _, _)) = &source {
                arguments["resume_from"] = json!(source_spawn.subagent_id);
            } else {
                arguments["model"] = json!("child/context");
            }
            let spawn = expect_tool(&parent_server, &spawn_call, "spawn_subagent", arguments);
            assert_eq!(foreground_requests(&child_server).len(), index);
            let mut first = child_server.expect_response_blocked(
                format!("{case}-FIRST-child-request"),
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                child_completion(),
            );
            let prompt = format!("Run the isolated {case} delegation.");
            let (reply, captured_spawn) = tokio::join!(
                client.prompt_with_timeout(&session, &prompt),
                async {
                    tokio::time::timeout(scaled(Duration::from_secs(30)), first.wait_blocked()).await
                        .unwrap_or_else(|_| panic!("{}\n{}", first.diagnostic(), client.stderr()));
                    assert!(!first.is_satisfied(), "capture before allowing the terminal response");
                    let requests = foreground_requests(&child_server);
                    assert_eq!(requests.len(), index + 1, "no later resample can stand in for the FIRST request");
                    let request = &requests[index];
                    let body = request.body.as_ref().unwrap();
                    let (_, parent) = timeline(&grow_home, parent_id);
                    let (spawn_seq, current_spawn) = spawn_for_call(&parent, &spawn_call);
                    assert_eq!(current_spawn.prompt, TASK_PROMPT);
                    assert!(!current_spawn.prompt.contains("Current delegated parent step"));
                    assert_eq!(current_spawn.resumed_from.as_deref(), source.as_ref().map(|(spawn, _, _)| spawn.subagent_id.as_str()));
                    let (_, child) = timeline(&grow_home, &current_spawn.child_session_id);
                    assert_first_request(body, parent_id, &parent, &child, spawn_seq, current_spawn, step_id);
                    if let Some((source_spawn, source_path, source_bytes)) = &source {
                        assert_ne!(current_spawn.subagent_id, source_spawn.subagent_id);
                        assert_ne!(current_spawn.parent_step_id, source_spawn.parent_step_id);
                        assert_eq!(std::fs::read(source_path).unwrap(), *source_bytes, "resume cannot mutate its completed source");
                    }
                    let captured_spawn = current_spawn.clone();
                    first.release();
                    captured_spawn
                }
            );
            assert_eq!(reply.unwrap().stop_reason, acp::StopReason::EndTurn);
            todo.assert_satisfied();
            spawn.assert_satisfied();
            first.assert_satisfied();
            assert_eq!(foreground_requests(&child_server).len(), index + 1,
                "each child must finish from the captured first request without resampling");
            let (_, parent) = timeline(&grow_home, parent_id);
            let (child_path, child) = timeline(&grow_home, &captured_spawn.child_session_id);
            assert_completed(&parent, &child, &captured_spawn);
            if let Some((source_spawn, source_path, source_bytes)) = &source {
                assert_eq!(std::fs::read(source_path).unwrap(), *source_bytes);
                assert_eq!(spawn_for_call(&parent, "call_fresh_spawn").1, source_spawn);
            } else {
                source = Some((captured_spawn.clone(), child_path.clone(), std::fs::read(child_path).unwrap()));
            }
            completed_spawns.push(captured_spawn);
        }
        tokio::time::timeout(scaled(Duration::from_secs(30)), client.ext_method("grow/session/close", json!({"sessionId":parent_id}))).await.unwrap().unwrap();
        tokio::time::timeout(scaled(Duration::from_secs(10)), client.close()).await.unwrap().unwrap();
        assert!(client.child_pid().is_none(), "the test-owned writer process must be reaped");
        let (_, cold_parent) = timeline(&grow_home, parent_id);
        for spawn in &completed_spawns {
            let (_, child) = timeline(&grow_home, &spawn.child_session_id);
            assert_completed(&cold_parent, &child, spawn);
        }
        let (_, source_path, source_bytes) = source.unwrap();
        assert_eq!(std::fs::read(source_path).unwrap(), source_bytes);
        assert!(foreground_requests(&parent_server).iter().all(|entry| entry.body.as_ref().unwrap()["model"] == "parent-wire"));
    }).await;
}
