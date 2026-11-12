//! Real CLI regression for a delivery released before its canonical result.
//! Run with the current GROW_BINARY; all state and providers are test-owned.

#![cfg(unix)]

use std::convert::Infallible;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::post;
use chat_state::{SubagentEvent, SubagentOutcome, Timeline, TimelineEventKind};
use futures::StreamExt;
use serde_json::{Value, json};
use test_support::*;
use tool_types::{LocalPlan, ReplanProgress, ReviewDecision};

const STEP: &str = "released-delivery";
const REPORT: &str = "The isolated Goal exercised a real required background child.\nReleased the delivery before its result, verified live execution and the parent update blocked completion, then explicitly cancelled and waited for the child.\nCanonical parent/child linkage and the terminal plan were verified; the historical release stayed unbound and no acceptance was invented.\nAll providers were loopback mocks. This does not recover any existing user session.";

struct BlockingChildProvider {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl BlockingChildProvider {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let router = axum::Router::new()
            .route("/v1/chat/completions", post(Self::reply))
            .with_state(requests.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            url,
            requests,
            task,
        }
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn request_bodies(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    async fn wait_started(&self) {
        tokio::time::timeout(Duration::from_secs(30), async {
            while self.requests.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the real child must enter its own provider stream");
    }

    async fn reply(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<Value>,
    ) -> axum::response::Response {
        let foreground = headers.contains_key("x-grow-turn-idx")
            || body["tools"]
                .as_array()
                .is_some_and(|tools| tools.len() >= 2);
        let model = body["model"].as_str().unwrap().to_owned();
        if !foreground {
            let events =
                test_support::sse::chat_completion_events("Isolated auxiliary reply.", &model);
            return Sse::new(futures::stream::iter(
                events.into_iter().map(Ok::<_, Infallible>),
            ))
            .into_response();
        }
        requests.lock().unwrap().push(body);
        // Parent edits may abandon one sample and start another. Every child
        // foreground stream stays nonterminal, including those resamples.
        let event = Event::default().data(json!({
            "id":"live-child", "object":"chat.completion.chunk", "created":0, "model":model,
            "choices":[{"index":0,"delta":{"role":"assistant","content":"The test child is live."},"finish_reason":null}]
        }).to_string());
        Sse::new(
            futures::stream::iter([Ok::<_, Infallible>(event)]).chain(futures::stream::pending()),
        )
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(1)))
        .into_response()
    }
}

impl Drop for BlockingChildProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn tool_reply(call_id: &str, name: &str, arguments: Value) -> ScriptedResponse {
    // One event keeps a blocked expectation entirely before tool dispatch.
    ScriptedResponse::sse(vec![SseEvent::data(
        json!({
            "id": call_id, "object": "chat.completion.chunk", "created": 0,
            "model": "parent-wire",
            "choices": [{"index": 0, "delta": {"tool_calls": [{
                "index": 0, "id": call_id, "type": "function",
                "function": {"name": name, "arguments": arguments.to_string()}
            }]}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        })
        .to_string(),
    )])
}

fn expect_tool(
    server: &MockInferenceServer,
    call_id: &str,
    name: &str,
    arguments: Value,
) -> InferenceExpectation {
    server.expect_response(
        call_id,
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        tool_reply(call_id, name, arguments),
    )
}

fn hold_tool(
    server: &MockInferenceServer,
    call_id: &str,
    name: &str,
    arguments: Value,
) -> InferenceExpectation {
    server.expect_response_blocked(
        call_id,
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        tool_reply(call_id, name, arguments),
    )
}

async fn wait_blocked(expectation: &mut InferenceExpectation, client: &GrowStdioClient) {
    tokio::time::timeout(Duration::from_secs(30), expectation.wait_blocked())
        .await
        .unwrap_or_else(|_| panic!("{}\n{}", expectation.diagnostic(), client.stderr()));
}

fn timeline(grow_home: &Path, session_id: &str) -> Timeline {
    uuid::Uuid::parse_str(session_id).expect("only the test's actual session IDs are read");
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
        "the private GROW_HOME must contain one canonical session path"
    );
    let bytes = std::fs::read_to_string(&paths[0]).unwrap();
    // The physical journal interleaves domain events with presentation facts.
    // Validate its sequence and fold every domain event through the real Timeline.
    let mut events = Vec::new();
    for (seq, line) in bytes.lines().enumerate() {
        let record: Value = serde_json::from_str(line).unwrap();
        assert_eq!(record["version"], 1);
        assert_eq!(record["seq"], seq as u64);
        match record["fact"]["kind"].as_str() {
            Some("domain") => {
                events.push(serde_json::from_value(record["fact"]["event"].clone()).unwrap())
            }
            Some("presentation") => {}
            kind => panic!("unexpected journal fact {kind:?}"),
        }
    }
    Timeline::from_events(events).expect("real CLI events must pass full Timeline validation")
}

fn plan(timeline: &Timeline) -> &LocalPlan {
    timeline
        .events()
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Planning(planning) => Some(planning.plan.as_ref()),
            _ => None,
        })
        .expect("native planning tools must commit a plan")
}

fn goal(timeline: &Timeline) -> Value {
    timeline
        .events()
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Control(control) => control.snapshot.get("goal").cloned(),
            _ => None,
        })
        .filter(|goal| goal.is_object())
        .expect("the actual Control must contain the test's Goal")
}

fn tool_result(server: &MockInferenceServer, call_id: &str) -> String {
    server
        .request_bodies()
        .iter()
        .rev()
        .filter_map(|body| body["messages"].as_array())
        .flatten()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == call_id)
        .and_then(|message| message["content"].as_str())
        .unwrap_or_else(|| {
            panic!(
                "missing real tool result for {call_id}: {}",
                server.request_log_summary()
            )
        })
        .to_owned()
}

fn assert_native_schema(body: &Value, name: &str, properties: &[&str]) {
    let definition = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == name)
        .unwrap_or_else(|| panic!("production model must expose {name}"));
    for property in properties {
        assert!(
            definition["function"]["parameters"]["properties"]
                .get(property)
                .is_some(),
            "{name} must expose the actual {property} parameter: {definition}"
        );
    }
}

async fn ext(client: &GrowStdioClient, method: &str, arguments: Value) {
    tokio::time::timeout(
        Duration::from_secs(30),
        client.ext_method(method, arguments),
    )
    .await
    .unwrap_or_else(|_| panic!("{method} timed out: {}", client.stderr()))
    .unwrap();
}

fn assert_canonical_chain(parent: &Timeline, child: &Timeline, child_id: &str) {
    let spawns: Vec<_> = parent
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) => Some((event.seq, spawn)),
            _ => None,
        })
        .collect();
    assert_eq!(spawns.len(), 1, "only the test's own child may be spawned");
    let (spawn_seq, spawn) = spawns[0];
    assert_eq!(spawn.subagent_id, child_id);
    assert_eq!(spawn.child_session_id, child_id);
    assert_eq!(spawn.origin_tool_call_id.as_deref(), Some("call_spawn"));
    assert_eq!(spawn.parent_step_id.as_deref(), Some(STEP));
    assert_eq!(spawn.effective_model_id, "child/live");
    let parent_plan = plan(parent);
    assert_eq!(spawn.security_parent_session_id, parent_plan.session_id);

    let seeds: Vec<_> = child
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::SubagentSeed(seed) => Some(seed),
            _ => None,
        })
        .collect();
    assert_eq!(seeds.len(), 1);
    let seed = seeds[0];
    assert_eq!(seed.parent_timeline_id, parent_plan.session_id);
    assert_eq!(seed.parent_spawn_seq, spawn_seq.get());
    assert_eq!(seed.security_parent_session_id, parent_plan.session_id);
    assert_eq!(seed.subagent_id, child_id);

    let endings: Vec<_> = parent
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(ended)) => Some(ended),
            _ => None,
        })
        .collect();
    assert_eq!(
        endings.len(),
        1,
        "the real cancellation must close the parent Spawn exactly once"
    );
    let ended = endings[0];
    assert_eq!(ended.subagent_id, child_id);
    assert_eq!(ended.child_session_id, child_id);
    assert_eq!(ended.outcome, SubagentOutcome::Cancelled);
    let reference = ended
        .result_ref
        .as_ref()
        .expect("parent Ended must reference the canonical child Result");
    assert_eq!(reference.timeline_id, child_id);
    assert_eq!(reference.first_seq, reference.last_seq);
    let results: Vec<_> = child
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::SubagentResult(result) => Some((event.seq, result)),
            _ => None,
        })
        .collect();
    assert_eq!(
        results.len(),
        1,
        "the child owns one canonical terminal Result"
    );
    let (result_seq, result) = results[0];
    assert_eq!(reference.first_seq, result_seq.get());
    assert_eq!(result.subagent_id, child_id);
    assert_eq!(result.outcome, ended.outcome);
    assert_eq!(result.error, ended.error);
    assert_eq!(result.duration_ms, ended.duration_ms);
    assert_eq!(result.tool_calls, ended.tool_calls);
    assert_eq!(result.turns, ended.turns);
    assert_eq!(result.tokens_used, ended.tokens_used);

    assert_eq!(parent_plan.links.len(), 1);
    let link = &parent_plan.links[0];
    assert_eq!(link.id, child_id);
    assert_eq!(link.child_session_id, child_id);
    assert_eq!(link.parent_step_id, STEP);
    assert!(link.required && link.terminal);
    assert!(link.dispatch_failure.is_none());
    let planning_reference: chat_state::TimelineRangeRef = serde_json::from_str(
        link.result_id
            .as_deref()
            .expect("runtime must publish the canonical result to planning"),
    )
    .unwrap();
    assert_eq!(&planning_reference, reference);
    assert!(link.pending_source_revision.is_none());
    assert!(!parent_plan.replans.is_empty());
    assert!(parent_plan.replans.iter().all(|change| {
        change.link_id == child_id
            && change.child_session_id == child_id
            && matches!(&change.progress, ReplanProgress::Unapplied { reason } if !reason.trim().is_empty())
    }));
    assert!(parent_plan.waits.is_empty());
    assert!(parent_plan.unresolved_required_deliveries().is_empty());
    parent_plan.validate().unwrap();
}

#[tokio::test]
#[ignore = "requires the current GROW_BINARY; all providers and state are isolated"]
async fn released_delivery_accepts_real_terminal_result_and_completes_goal() {
    tokio::task::LocalSet::new().run_until(async {
        // Fail instead of allowing the shared harness to build or select another binary.
        let binary = std::env::var_os("GROW_BINARY").expect("set GROW_BINARY to the current build");
        let binary = std::fs::canonicalize(binary).expect("GROW_BINARY must exist");
        let parent_server = MockInferenceServer::start().await.unwrap();
        let child_server = BlockingChildProvider::start().await;
        assert_ne!(parent_server.url(), child_server.url());
        parent_server.set_response("The isolated fixture is ready.");
        let mut sandbox = TestSandbox::builder().git().mock_url(parent_server.url()).build();
        sandbox.set_env("GROW_GOAL", "1");
        let workspace = sandbox.workspace().to_path_buf();
        let grow_home = sandbox.grow_home().to_path_buf();
        assert!(workspace.starts_with(sandbox.root()) && grow_home.starts_with(sandbox.root()));
        let child_env = sandbox.env();
        assert!(child_env.iter().any(|(key, value)| key == "GROW_HOME" && Path::new(value) == grow_home));
        assert!(!child_env.iter().any(|(key, _)| key == "OPENAI_API_KEY" || key == "ANTHROPIC_API_KEY"));
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
[provider.child.models.live]
model = "child-wire"
context_window = 200000
agent_type = "grow-build"
"#, parent_server.url(), child_server.url())).unwrap();
        eprintln!("isolated binary={} GROW_HOME={} workspace={} parent={} child={}",
            binary.display(), grow_home.display(), workspace.display(), parent_server.url(), child_server.url());
        let mut client = GrowStdioClient::spawn_with_sandbox(&parent_server, &workspace, sandbox).await;
        client.initialize_with_timeout().await;
        let session = client.create_session_with_model_timeout(&workspace, "mock/parent").await;
        client.prompt_with_timeout(&session, "Read the native tool catalog for this isolated regression.").await.unwrap();
        let catalog = parent_server.request_bodies().into_iter()
            .find(|body| body["tools"].as_array().is_some_and(|tools| tools.len() >= 2)).unwrap();
        for (name, properties) in [
            ("create_goal", vec!["objective", "token_budget"]),
            ("todo_write", vec!["todos", "actions"]),
            ("spawn_subagent", vec!["background", "required_deliverable", "parent_step_id"]),
            ("update_goal", vec!["status", "completion_report"]),
            ("kill_command_or_subagent", vec!["task_id"]),
            ("wait_subagent", vec!["subagent_ids", "timeout_ms"]),
        ] { assert_native_schema(&catalog, name, &properties); }

        let create_step = expect_tool(&parent_server, "call_step", "todo_write", json!({"todos":[{
            "id":STEP,"content":"Own a live required child, then explicitly cancel it and verify late delivery.","status":"in_progress"
        }]}));
        let spawn = expect_tool(&parent_server, "call_spawn", "spawn_subagent", json!({
            "description":"Own isolated live child","prompt":"Stay in this test-owned provider stream until the parent cancels you.",
            "subagent_type":"general-purpose","background":true,"required_deliverable":true,
            "parent_step_id":STEP,"model":"child/live","isolation":"none"
        }));
        let mut after_spawn = hold_tool(&parent_server, "call_inspect_spawn", "todo_write", json!({"todos":[]}));
        // This is the existing host activation command. Completion below uses native UpdateGoal.
        ext(&client, "grow/commands/execute", json!({"sessionId":session.0.as_ref(),
            "command":"/goal set Verify the isolated Released(None) late-result completion chain",
            "description":"start the isolated regression Goal","invocationId":uuid::Uuid::now_v7().to_string()})).await;
        wait_blocked(&mut after_spawn, &client).await;
        child_server.wait_started().await;
        create_step.assert_satisfied();
        spawn.assert_satisfied();
        let started = tool_result(&parent_server, "call_spawn");
        let child_id = started.lines().find_map(|line| line.trim().strip_prefix("subagent_id: "))
            .expect("background Task must return the actual child ID").to_owned();
        let spawned_timeline = timeline(&grow_home, session.0.as_ref());
        let spawned_plan = plan(&spawned_timeline);
        assert_eq!(spawned_plan.links.len(), 1);
        let link = &spawned_plan.links[0];
        assert_eq!(link.id, child_id);
        assert!(link.required && !link.terminal);
        assert!(link.result_id.is_none() && link.dispatch_failure.is_none());
        let active_goal = goal(&spawned_timeline);
        assert_eq!(active_goal["status"], "active");

        let release = expect_tool(&parent_server, "call_release", "todo_write", json!({"todos":[],"actions":[{"review":{"review":{
            "linkId":link.id,"resultId":null,"reviewedSourceRevision":link.review_required_revision,
            "decision":"released","reason":"This test explicitly cancels its own child; its deliverable is no longer required."
        }}}]}));
        let edit = expect_tool(&parent_server, "call_edit", "todo_write", json!({"todos":[{
            "id":STEP,"content":"Cancel the same child and verify its canonical result without accepting the released delivery."
        }]}));
        let reject = expect_tool(&parent_server, "call_complete_live", "update_goal", json!({"status":"complete","completion_report":REPORT}));
        let mut before_kill = hold_tool(&parent_server, "call_kill", "kill_command_or_subagent", json!({"task_id":child_id}));
        after_spawn.release();
        wait_blocked(&mut before_kill, &client).await;
        after_spawn.assert_satisfied();
        release.assert_satisfied();
        edit.assert_satisfied();
        reject.assert_satisfied();
        let rejected = tool_result(&parent_server, "call_complete_live");
        assert!(rejected.contains("Goal completion has unresolved work:"), "must reach the actual Goal completion gate: {rejected}");
        assert!(rejected.contains(&format!("required task {child_id} has an unhandled parent update")), "missing replan blocker: {rejected}");
        assert!(rejected.contains(&format!("released task {child_id} still needs explicit execution and wait disposition")), "missing live execution blocker: {rejected}");
        let released_timeline = timeline(&grow_home, session.0.as_ref());
        let released_plan = plan(&released_timeline);
        let historical_reviews = released_plan.reviews.clone();
        assert_eq!(historical_reviews.len(), 1);
        assert_eq!(historical_reviews[0].decision, ReviewDecision::Released);
        assert_eq!(historical_reviews[0].result_id, None);
        assert!(!released_plan.links[0].terminal && released_plan.links[0].result_id.is_none());
        assert!(released_plan.links[0].dispatch_failure.is_none());
        assert!(released_plan.links[0].pending_source_revision.is_some());
        assert!(released_plan.replans.iter().any(|change| matches!(
            change.progress,
            ReplanProgress::Pending | ReplanProgress::Dispatching | ReplanProgress::Received | ReplanProgress::Consumed
        )));
        let child_before = timeline(&grow_home, &child_id);
        assert!(!child_before.events().iter().any(|event| matches!(event.kind, TimelineEventKind::SubagentResult(_))));
        assert!(!child_server.request_bodies().is_empty(), "the released child must have entered its real nonterminal provider");
        let unchanged_goal = goal(&released_timeline);
        assert_eq!(unchanged_goal["goal_id"], active_goal["goal_id"]);
        assert_eq!(unchanged_goal["definition_revision"], active_goal["definition_revision"]);
        assert_eq!(unchanged_goal["status"], "active");
        assert!(unchanged_goal["completion_report"].is_null());
        eprintln!("native Complete rejected for child={child_id}: {rejected}");

        let wait = expect_tool(&parent_server, "call_wait", "wait_subagent", json!({"subagent_ids":[child_id],"timeout_ms":30000}));
        let mut after_wait = hold_tool(&parent_server, "call_inspect_terminal", "todo_write", json!({"todos":[]}));
        before_kill.release();
        wait_blocked(&mut after_wait, &client).await;
        before_kill.assert_satisfied();
        wait.assert_satisfied();
        let killed = tool_result(&parent_server, "call_kill");
        assert_eq!(killed, "killed: Subagent cancellation initiated");
        let waited: Value = serde_json::from_str(&tool_result(&parent_server, "call_wait")).unwrap();
        assert_eq!(waited["reason"], "any_finished", "wait must observe actual terminal delivery: {waited}");
        assert_eq!(waited["subagents"].as_array().unwrap().len(), 1);
        assert_eq!(waited["subagents"][0]["subagent_id"], child_id);
        let terminal_parent = timeline(&grow_home, session.0.as_ref());
        let terminal_child = timeline(&grow_home, &child_id);
        assert_eq!(waited["subagents"][0]["status"], "cancelled",
            "native wait={waited}; parent Ended={:?}; child Result={:?}; stderr={}",
            terminal_parent.events().iter().filter(|event| matches!(event.kind, TimelineEventKind::Subagent(SubagentEvent::Ended(_)))).collect::<Vec<_>>(),
            terminal_child.events().iter().filter(|event| matches!(event.kind, TimelineEventKind::SubagentResult(_))).collect::<Vec<_>>(),
            client.stderr());
        assert_canonical_chain(&terminal_parent, &terminal_child, &child_id);
        assert_eq!(plan(&terminal_parent).reviews, historical_reviews);

        let complete = expect_tool(&parent_server, "call_complete_terminal", "update_goal", json!({"status":"complete","completion_report":REPORT}));
        after_wait.release();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if goal(&timeline(&grow_home, session.0.as_ref()))["status"] == "complete" { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.unwrap_or_else(|_| panic!("native Complete did not commit: {}", client.stderr()));
        after_wait.assert_satisfied();
        complete.assert_satisfied();
        let live_goal = goal(&timeline(&grow_home, session.0.as_ref()));
        assert_eq!(live_goal["goal_id"], active_goal["goal_id"]);
        assert_eq!(live_goal["definition_revision"], active_goal["definition_revision"]);
        assert_eq!(live_goal["completion_report"].as_str(), Some(REPORT));
        ext(&client, "grow/session/close", json!({"sessionId":session.0.as_ref()})).await;
        tokio::time::timeout(Duration::from_secs(10), client.close()).await.expect("own process cleanup is bounded").unwrap();
        assert!(client.child_pid().is_none(), "the test's writer process must be reaped");
        let cold_parent = timeline(&grow_home, session.0.as_ref());
        let cold_child = timeline(&grow_home, &child_id);
        assert_canonical_chain(&cold_parent, &cold_child, &child_id);
        assert_eq!(plan(&cold_parent), plan(&terminal_parent));
        assert_eq!(plan(&cold_parent).reviews, historical_reviews);
        assert!(plan(&cold_parent).reviews.iter().all(|review| review.decision != ReviewDecision::Accepted));
        let cold_goal = goal(&cold_parent);
        assert_eq!(cold_goal["goal_id"], active_goal["goal_id"]);
        assert_eq!(cold_goal["status"], "complete");
        assert_eq!(cold_goal["completion_report"].as_str(), Some(REPORT));
        let first_complete = cold_parent.events().iter().find_map(|event| match &event.kind {
            TimelineEventKind::Control(control) => control.snapshot.get("goal")
                .filter(|goal| goal["status"] == "complete"),
            _ => None,
        }).expect("Complete and its report must exist in the same Control event");
        assert_eq!(first_complete["goal_id"], active_goal["goal_id"]);
        assert_eq!(first_complete["completion_report"].as_str(), Some(REPORT));
        assert!(parent_server.request_bodies().iter().all(|body| body["model"] == "parent-wire"));
        assert!(child_server.request_bodies().iter().all(|body| body["model"] == "child-wire"));
        eprintln!("cold verified child={child_id}: Spawn/Seed/Result/Ended exact link; historical Released(None); Unapplied replan; Goal Complete + exact report; own writer reaped");
    }).await;
}
