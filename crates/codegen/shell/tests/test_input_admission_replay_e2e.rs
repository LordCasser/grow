//! End-to-end acceptance of the F3 candidate wiring through a real built
//! `grow agent stdio` binary: a retried `session/prompt` RPC with the same
//! `_meta.promptId` must be answered by the canonical input-admission replay
//! (StopReason EndTurn + `grow/inputAdmission` identity/state meta, no tokens/
//! usage/structured output), while the original RPC stays pending and no new
//! queue entry, Hook, turn, or roster Working/Idle projection is produced.
//!
//! The shell already folds the durable Timeline through the stable input
//! identity (`shell/src/session/actor/input_admission.rs`), so every replay
//! assertion below is cross-checked against the on-disk `timeline.jsonl`
//! facts rather than the RPC response alone.
//!
//! Unlike the shared `RawStdioClient` (which discards notifications while
//! waiting for a response), the local client here keeps a bounded record of
//! every agent→client notification, which is what makes the "no fake roster
//! delta" assertion possible without touching shared test support.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use test_support::{
    InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer, ScriptedResponse, SseEvent,
    TestOutput, TestProcess, TestProcessConfig, TestProcessStdout, TestSandbox, TestStdin,
    git_workdir, grow_binary, scaled, stderr_tail,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

/// Same hermetic model config shape as `test_sampler_request_lifecycle.rs`
/// (the fixture whose foreground requests demonstrably reach the mock's
/// foreground classifier): `api_backend` at the provider level, grow-build
/// agent type with its toolset, pointed at the mock inference server.
fn write_lifecycle_config(server: &MockInferenceServer, sandbox: &TestSandbox) {
    std::fs::write(
        sandbox.grow_home().join("config.toml"),
        format!(
            "[models]\ndefault = \"mock/test-model\"\nsession_title = \"mock/test-model\"\n\
             [provider.mock]\napi_backend = \"chat_completions\"\n\
             [provider.mock.options]\nbase_url = {:?}\nenv_key = \"GROW_API_KEY\"\n\
             [provider.mock.models.test-model]\nmodel = \"test-model\"\ncontext_window = 200000\n\
             agent_type = \"grow-build\"\nmax_retries = 3\n",
            server.url(),
        ),
    )
    .expect("write hermetic config.toml");
}

/// A minimal end-turn SSE response for one foreground turn.
fn text_response(text: &str) -> ScriptedResponse {
    ScriptedResponse::sse(vec![
        SseEvent::data(
            json!({
                "id":"admission-replay-response","object":"chat.completion.chunk","created":0,"model":"test-model",
                "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
            })
            .to_string(),
        ),
        SseEvent::data("[DONE]"),
    ])
}

/// `input_inbox::input_id_for_prompt`: the durable input identity is derived
/// from the session id and prompt id, so the test can assert the replay meta
/// echoes the canonical identity instead of a fresh one.
fn input_id_for_prompt(session_id: &str, prompt_id: &str) -> String {
    let mut hasher = blake3::Hasher::new_derive_key("grow.shell.input-identity.v1");
    hasher.update(&(session_id.len() as u64).to_le_bytes());
    hasher.update(session_id.as_bytes());
    hasher.update(&(prompt_id.len() as u64).to_le_bytes());
    hasher.update(prompt_id.as_bytes());
    format!("input-{}", hasher.finalize().to_hex())
}

/// Minimal raw-wire client around the shared [`TestProcess`] lifecycle owner:
/// verbatim JSON-RPC lines in, every line recorded out. Unlike
/// `RawStdioClient`, notifications are retained and non-matching responses are
/// deferred (not skipped), so a still-pending first RPC is never lost while
/// later RPCs are being read. Bounded to this file: no generic client.
struct RecordingAgentClient {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::BufReader<TestProcessStdout>,
    process: TestProcess,
    line_buf: String,
    notifications: Vec<Value>,
    deferred_responses: Vec<Value>,
}

impl RecordingAgentClient {
    fn spawn(server: &MockInferenceServer, cwd: &Path, sandbox: &mut TestSandbox) -> Self {
        sandbox.set_mock_url(server.url());
        let mut cmd = tokio::process::Command::new(grow_binary());
        cmd.args(["agent", "stdio"]).current_dir(cwd);
        let mut process = TestProcess::spawn(
            cmd,
            &sandbox,
            TestProcessConfig::new()
                .label("grow agent stdio")
                .stdin(TestStdin::Piped)
                .stdout(TestOutput::Piped),
        )
        .expect("spawn grow agent stdio");
        let stdin = process.take_stdin().expect("child stdin missing");
        let stdout = process.take_stdout().expect("child stdout missing");
        Self {
            stdin,
            stdout: tokio::io::BufReader::new(stdout),
            process,
            line_buf: String::new(),
            notifications: Vec::new(),
            deferred_responses: Vec::new(),
        }
    }

    fn stderr_text(&self) -> String {
        self.process.stderr_tail().text
    }

    async fn send_line(&mut self, line: &str) {
        self.stdin
            .write_all(line.as_bytes())
            .await
            .expect("write line to agent stdin");
        self.stdin
            .write_all(b"\n")
            .await
            .expect("write newline to agent stdin");
        self.stdin.flush().await.expect("flush agent stdin");
    }

    /// Read one stdout line (bounded by `deadline`). Notifications are
    /// recorded; agent→client requests are refused like `RawStdioClient` does
    /// so a turn can never hang on this capability-less client. Returns the
    /// parsed JSON value of the line, or `None` on timeout.
    async fn read_one_line(&mut self, deadline: tokio::time::Instant) -> Option<Value> {
        loop {
            let read =
                match tokio::time::timeout_at(deadline, self.stdout.read_line(&mut self.line_buf))
                    .await
                {
                    Err(_) => return None,
                    Ok(Err(error)) => panic!("agent stdout read failed: {error}"),
                    Ok(Ok(read)) => read,
                };
            if read == 0 {
                panic!(
                    "agent closed stdout early\nstderr:\n{}",
                    stderr_tail(&self.stderr_text(), 1200)
                );
            }
            let line = std::mem::take(&mut self.line_buf);
            let Ok(message) = serde_json::from_str::<Value>(line.trim_end()) else {
                continue;
            };
            if message.get("method").is_some() {
                if let Some(request_id) = message.get("id") {
                    let refusal = json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "error": { "code": -32601, "message": "unsupported by raw test client" },
                    });
                    self.send_line(&refusal.to_string()).await;
                }
                self.notifications.push(message);
                continue;
            }
            return Some(message);
        }
    }

    /// Wait for the response to `id`, returning it. Responses for other ids
    /// seen along the way are deferred and remain retrievable later, so the
    /// first (still-pending) RPC response is never dropped.
    async fn response_for_id(&mut self, id: &str, what: &str, timeout: Duration) -> Value {
        if let Some(index) = self
            .deferred_responses
            .iter()
            .position(|message| message.get("id").and_then(Value::as_str) == Some(id))
        {
            return self.deferred_responses.remove(index);
        }
        let deadline = tokio::time::Instant::now() + scaled(timeout);
        let mut skipped = 0_usize;
        while let Some(response) = self.read_one_line(deadline).await {
            if response.get("id").and_then(Value::as_str) == Some(id) {
                return response;
            }
            skipped += 1;
            self.deferred_responses.push(response);
        }
        panic!(
            "{what}: no response for id {id:?} within {timeout:?} ({skipped} other responses \
             deferred, {} notifications recorded)\nstderr:\n{}",
            self.notifications.len(),
            stderr_tail(&self.stderr_text(), 1200)
        );
    }

    /// Read whatever traffic is available within `quiet`, recording it.
    async fn pump(&mut self, quiet: Duration) {
        let deadline = tokio::time::Instant::now() + scaled(quiet);
        while let Some(response) = self.read_one_line(deadline).await {
            self.deferred_responses.push(response);
        }
    }

    /// `grow/sessions/changed` deltas observed so far that touch this session.
    /// Handles both wire shapes `shell/src/leader/server.rs::method_of` /
    /// `interaction_inner_params` produce today: direct
    /// `{method:"grow/sessions/changed", params:{upserted}}` and the gateway
    /// ext wrap `{method:"_grow/sessions/changed",
    /// params:{method:"grow/sessions/changed", params:{upserted}}}`.
    fn session_roster_deltas<'a>(&'a self, session_id: &str) -> Vec<&'a Value> {
        self.notifications
            .iter()
            .filter(|message| {
                let top = message.get("method").and_then(Value::as_str);
                let effective_method = match top {
                    Some(top) => match top.strip_prefix('_') {
                        Some(stripped) => message["params"]["method"].as_str().unwrap_or(stripped),
                        None => top,
                    },
                    None => return false,
                };
                if effective_method != "grow/sessions/changed" {
                    return false;
                }
                let params = &message["params"];
                let real_params = if params.get("method").is_some() {
                    &params["params"]
                } else {
                    params
                };
                real_params["upserted"].as_array().is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|entry| entry["sessionId"].as_str() == Some(session_id))
                })
            })
            .map(|message| {
                let params = &message["params"];
                if params.get("method").is_some() {
                    &message["params"]["params"]["upserted"]
                } else {
                    &message["params"]["upserted"]
                }
            })
            .flat_map(Value::as_array)
            .flatten()
            .collect()
    }

    /// Whether a response for `id` has been received (deferred or consumed).
    fn response_seen(&self, id: &str) -> bool {
        self.deferred_responses
            .iter()
            .any(|message| message.get("id").and_then(Value::as_str) == Some(id))
    }
}

/// Locate `<grow-home>/sessions/<encoded-cwd>/<session-id>/timeline.jsonl`
/// without duplicating the cwd-encoding rule: walk the sessions tree for the
/// session-id directory.
fn session_timeline_path(sandbox: &TestSandbox, session_id: &str) -> Option<PathBuf> {
    fn find(dir: &Path, session_id: &str) -> Option<PathBuf> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) == Some(session_id) {
                    let timeline = path.join("timeline.jsonl");
                    if timeline.is_file() {
                        return Some(timeline);
                    }
                }
                if let Some(found) = find(&path, session_id) {
                    return Some(found);
                }
            }
        }
        None
    }
    find(&sandbox.grow_home().join("sessions"), session_id)
}

/// Read the durable Timeline's domain (canonical) events off disk. Each
/// `timeline.jsonl` line is a checksummed `Record{version,seq,checksum,fact}`
/// whose `fact.kind == "domain"` payload is the serialized `TimelineEvent`
/// (`type`/`event` tagged). Only a torn trailing line (no terminating newline,
/// i.e. a concurrent flush in progress) may be dropped; any other malformed
/// line is a hard error — silently swallowing parse failures here would let
/// the canonical-fact assertions pass vacuously.
fn timeline_domain_events(path: &Path) -> Vec<Value> {
    let bytes = std::fs::read(path).unwrap_or_else(|error| {
        panic!(
            "failed reading durable timeline {}: {error}",
            path.display()
        )
    });
    let mut lines = bytes.split(|b| *b == b'\n').peekable();
    let mut events = Vec::new();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        if lines.peek().is_none() && bytes.last() != Some(&b'\n') {
            // Torn trailing write: the next poll will see it complete.
            break;
        }
        let record = serde_json::from_slice::<Value>(line).unwrap_or_else(|error| {
            panic!(
                "malformed timeline record in {}: {error}\nline: {}",
                path.display(),
                String::from_utf8_lossy(line)
            )
        });
        if record["fact"]["kind"] != "domain" {
            continue;
        }
        let event = record["fact"]["event"].clone();
        assert!(
            event.get("type").is_some_and(Value::is_string),
            "domain event without a type tag in {}: {event}",
            path.display()
        );
        events.push(event);
    }
    events
}

#[derive(Default, Debug, PartialEq, Eq)]
struct AdmissionFacts {
    submitted: usize,
    consumed: usize,
    prompt_submit_hooks: usize,
    turn_started: usize,
    turn_ended: usize,
}

fn admission_facts(events: &[Value], input_id: &str, prompt_id: &str) -> AdmissionFacts {
    let mut facts = AdmissionFacts::default();
    // TurnEvent::Ended carries only the (string) turn id — no prompt_id — so
    // ended turns are counted by the turn ids this prompt started.
    let mut prompt_turn_ids: Vec<&str> = Vec::new();
    for event in events {
        let state = &event["event"];
        match event["type"].as_str() {
            Some("input") => {
                // Exact current schemas: Submitted{input_id}, Consumed/
                // Handled/Dismissed{input_ids}.
                let touches = match state["state"].as_str() {
                    Some("submitted") => state["input_id"].as_str() == Some(input_id),
                    Some("consumed") | Some("handled") | Some("dismissed") => state["input_ids"]
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(input_id))),
                    _ => false,
                };
                if !touches {
                    continue;
                }
                match state["state"].as_str() {
                    Some("submitted") => facts.submitted += 1,
                    Some("consumed") => facts.consumed += 1,
                    _ => {}
                }
            }
            Some("hook") => {
                if state["state"] == "triggered" && state["event"] == "user_prompt_submit" {
                    facts.prompt_submit_hooks += 1;
                }
            }
            Some("turn") => match state["state"].as_str() {
                Some("started") => {
                    if state["prompt_id"].as_str() == Some(prompt_id)
                        && let Some(id) = state["id"].as_str()
                    {
                        prompt_turn_ids.push(id);
                        facts.turn_started += 1;
                    }
                }
                Some("ended") => {
                    if state["id"]
                        .as_str()
                        .is_some_and(|id| prompt_turn_ids.contains(&id))
                    {
                        facts.turn_ended += 1;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    facts
}

/// The `_meta` object of a prompt response — the ACP schema's exact key
/// (`#[serde(rename = "_meta")]` on `PromptResponse.meta`).
fn response_meta(response: &Value) -> Value {
    response["result"]["_meta"].clone()
}

/// A full real-binary pass of the F3 candidate wiring: first-prompt
/// submission, in-flight canonical replay, identity conflict, and the
/// post-completion consumed replay — every step cross-checked against the
/// durable Timeline and the recorded notification stream.
#[tokio::test]
#[ignore] // requires pre-built binary; run with --ignored
async fn test_prompt_admission_replay_real_acp_entry() {
    let server = MockInferenceServer::start()
        .await
        .expect("start mock server");
    let workdir = git_workdir();
    let mut sandbox = TestSandbox::new();
    write_lifecycle_config(&server, &sandbox);
    let cwd = workdir.workspace().to_path_buf();
    let mut client = RecordingAgentClient::spawn(&server, &cwd, &mut sandbox);

    // Hold only the foreground turn's completion via a per-expectation
    // terminal barrier (same mechanism as `test_sampler_request_lifecycle`):
    // aux requests (session title, …) are never blocked, and `wait_blocked`
    // proves the target response really reached its terminal event without
    // emitting it — a real in-flight turn, not a global hold that may miss.
    let mut foreground = server.expect_response_blocked(
        "admission replay foreground hold",
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        text_response("foreground turn output"),
    );

    // Raw init/auth/new handshake (same wire shape as the typed-protocol
    // built-binary tests).
    client
        .send_line(
            &json!({
                "jsonrpc": "2.0",
                "id": "init-1",
                "method": "initialize",
                "params": {
                    "protocolVersion": 1,
                    "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
                    "_meta": { "startupHints": { "nonInteractive": true, "skipGitStatus": true, "skipProjectLayout": true }, "clientType": "admission-replay-e2e" }
                }
            })
            .to_string(),
        )
        .await;
    let init = client
        .response_for_id("init-1", "initialize", Duration::from_secs(20))
        .await;
    assert!(
        init.get("result").is_some(),
        "initialize failed: {init}\nstderr:\n{}",
        stderr_tail(&client.stderr_text(), 1200)
    );

    client
        .send_line(
            &json!({
                "jsonrpc": "2.0",
                "id": "auth-1",
                "method": "authenticate",
                "params": { "methodId": "provider.api_key", "_meta": { "headless": true } }
            })
            .to_string(),
        )
        .await;
    let auth = client
        .response_for_id("auth-1", "authenticate", Duration::from_secs(20))
        .await;
    assert!(
        auth.get("error").is_none(),
        "authenticate failed: {auth}\nstderr:\n{}",
        stderr_tail(&client.stderr_text(), 1200)
    );

    client
        .send_line(
            &json!({
                "jsonrpc": "2.0",
                "id": "new-1",
                "method": "session/new",
                "params": { "cwd": cwd, "mcpServers": [] }
            })
            .to_string(),
        )
        .await;
    let new_session = client
        .response_for_id("new-1", "session/new", Duration::from_secs(30))
        .await;
    let session_id = new_session["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "session/new must return a sessionId, got: {new_session}\nstderr:\n{}",
                stderr_tail(&client.stderr_text(), 1200)
            )
        })
        .to_string();

    let prompt_id = "e2e-admission-replay-prompt".to_string();
    let input_id = input_id_for_prompt(&session_id, &prompt_id);
    let prompt_text = "admission replay probe";
    let prompt_line = |rpc_id: &str, text: &str| {
        json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }],
                "_meta": { "promptId": prompt_id }
            }
        })
        .to_string()
    };

    // --- rpc1: first submission of the immutable request (stays pending) ---
    client.send_line(&prompt_line("rpc-1", prompt_text)).await;

    // The foreground expectation must actually receive the turn's inference
    // request (classifier: foreground) and block at its terminal event.
    foreground.wait_received().await;
    foreground.wait_blocked().await;

    // Wait until the foreground turn is really live: the durable Timeline has
    // the Submitted and TurnStarted facts, and the roster Working delta
    // (published after the user message is durable) has actually been observed
    // on the wire. The blocked inference response guarantees the turn cannot
    // end while the retry RPCs observe it.
    let mut timeline_path = None;
    let foreground_deadline = tokio::time::Instant::now() + scaled(Duration::from_secs(60));
    loop {
        client.pump(Duration::from_millis(200)).await;
        timeline_path = session_timeline_path(&sandbox, &session_id);
        let facts = timeline_path
            .as_deref()
            .map(|path| admission_facts(&timeline_domain_events(path), &input_id, &prompt_id))
            .unwrap_or_default();
        let roster_working = client
            .session_roster_deltas(&session_id)
            .iter()
            .any(|entry| entry["activity"] == "working");
        if facts.turn_started >= 1 && facts.submitted >= 1 && roster_working {
            break;
        }
        assert!(
            tokio::time::Instant::now() < foreground_deadline,
            "foreground turn never went live (requests: {}, facts: {:?}, roster deltas: {})\n\
             request log:\n{}\nstderr:\n{}",
            server.request_count(),
            timeline_path
                .as_deref()
                .map(|path| admission_facts(&timeline_domain_events(path), &input_id, &prompt_id))
                .unwrap_or_default(),
            client.session_roster_deltas(&session_id).len(),
            server.request_log_summary(),
            stderr_tail(&client.stderr_text(), 1200),
        );
        tokio::time::sleep(scaled(Duration::from_millis(100))).await;
    }
    let timeline_path = timeline_path.expect("timeline located during foreground wait");

    // Baselines for the replay window: canonical facts (counted off the
    // durable Timeline) and the session's own roster activity deltas, taken
    // only after the legitimate Working delta was published. The replayed
    // RPCs must not add to either.
    let roster_baseline = client.session_roster_deltas(&session_id).len();
    let before_replay = admission_facts(
        &timeline_domain_events(&timeline_path),
        &input_id,
        &prompt_id,
    );
    let requests_before_replay = server.request_count();

    // --- rpc2: identical retry while rpc1 is pending → canonical replay ---
    client.send_line(&prompt_line("rpc-2", prompt_text)).await;
    let replay = client
        .response_for_id(
            "rpc-2",
            "in-flight admission replay",
            Duration::from_secs(30),
        )
        .await;
    assert!(
        replay.get("error").is_none(),
        "replay must resolve, not error: {replay}\nrequest log:\n{}\nstderr:\n{}",
        server.request_log_summary(),
        stderr_tail(&client.stderr_text(), 1200)
    );
    assert_eq!(
        replay["result"]["stopReason"], "end_turn",
        "replay stop reason: {replay}"
    );
    let meta = response_meta(&replay);
    assert_eq!(
        meta["promptId"], prompt_id,
        "replay meta echoes the stable promptId: {meta}"
    );
    let admission = &meta["grow/inputAdmission"];
    assert_eq!(
        admission["inputId"], input_id,
        "replay meta carries the canonical input identity: {admission}"
    );
    let status = admission["status"]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("replay admission status missing: {admission}"));
    // The fold state is decided by the durable Timeline, not hardcoded: the
    // input was consumed by the live turn iff a Consumed fact is on disk.
    let in_flight_facts = admission_facts(
        &timeline_domain_events(&timeline_path),
        &input_id,
        &prompt_id,
    );
    let expected_status = if in_flight_facts.consumed >= 1 {
        "consumed"
    } else {
        "allowed"
    };
    assert_eq!(
        status, expected_status,
        "replay status must match the durable fold state (facts: {in_flight_facts:?}): {admission}"
    );
    if let Some(route) = admission["status"]["route"].as_str() {
        assert_eq!(route, "fifo", "replay route: {admission}");
    }
    // Observation-only reply: no token/usage/structured-output cargo.
    for forbidden in [
        "totalTokens",
        "usage",
        "structuredOutput",
        "structuredOutputError",
        "inputTokens",
        "outputTokens",
    ] {
        assert!(
            meta.get(forbidden).is_none(),
            "replay meta must not carry {forbidden}: {meta}"
        );
    }

    // The in-flight replay executed nothing: identical canonical facts, no
    // additional inference request, and rpc1 has genuinely never been
    // answered (no deferred response either).
    let after_replay = admission_facts(
        &timeline_domain_events(&timeline_path),
        &input_id,
        &prompt_id,
    );
    assert_eq!(
        before_replay, after_replay,
        "in-flight replay must not add canonical facts"
    );
    assert_eq!(
        server.request_count(),
        requests_before_replay,
        "in-flight replay must not reach the inference server"
    );
    assert!(
        !client.response_seen("rpc-1"),
        "rpc-1 must still be pending after the in-flight replay"
    );

    let before_conflict = after_replay;
    let requests_before_conflict = server.request_count();

    // --- rpc3: same promptId, one-character-different payload → conflict ---
    client
        .send_line(&prompt_line("rpc-3", &format!("{prompt_text}!")))
        .await;
    let conflict = client
        .response_for_id("rpc-3", "identity conflict", Duration::from_secs(30))
        .await;
    assert!(
        conflict.get("error").is_some(),
        "mutated payload with a borrowed promptId must be rejected: {conflict}"
    );
    assert!(
        conflict["error"].to_string().contains("identity conflict"),
        "rejection must be the identity conflict, got: {conflict}"
    );

    // The conflict executed nothing: no new Submitted/Hook/Turn facts and no
    // additional inference request.
    let after_conflict = admission_facts(
        &timeline_domain_events(&timeline_path),
        &input_id,
        &prompt_id,
    );
    assert_eq!(
        before_conflict, after_conflict,
        "identity conflict must not add canonical facts"
    );
    assert_eq!(
        server.request_count(),
        requests_before_conflict,
        "identity conflict must not reach the inference server"
    );
    assert!(
        !client.response_seen("rpc-1"),
        "rpc-1 must still be pending after the identity conflict"
    );
    // And no new roster activity projection for this session was emitted for
    // either replay RPC (the Working baseline was already published above).
    assert_eq!(
        client.session_roster_deltas(&session_id).len(),
        roster_baseline,
        "replay RPCs must not publish roster deltas for this session"
    );

    // --- release the held completion; the original rpc1 ends normally ---
    foreground.release();
    let original = client
        .response_for_id(
            "rpc-1",
            "original prompt completion",
            Duration::from_secs(60),
        )
        .await;
    assert!(
        original.get("error").is_none(),
        "original prompt must complete normally: {original}\nrequest log:\n{}\nstderr:\n{}",
        server.request_log_summary(),
        stderr_tail(&client.stderr_text(), 1200)
    );
    assert_eq!(
        original["result"]["stopReason"], "end_turn",
        "original prompt stop reason: {original}"
    );
    foreground.assert_satisfied();

    let completion_deadline = tokio::time::Instant::now() + scaled(Duration::from_secs(30));
    let completed_facts = loop {
        let facts = admission_facts(
            &timeline_domain_events(&timeline_path),
            &input_id,
            &prompt_id,
        );
        if facts.turn_ended >= 1 {
            break facts;
        }
        assert!(
            tokio::time::Instant::now() < completion_deadline,
            "original turn never ended on the Timeline (facts: {facts:?})\nstderr:\n{}",
            stderr_tail(&client.stderr_text(), 1200)
        );
        tokio::time::sleep(scaled(Duration::from_millis(100))).await;
    };
    assert_eq!(
        completed_facts.turn_started, before_conflict.turn_started,
        "exactly one turn for the whole exchange"
    );
    assert_eq!(completed_facts.submitted, 1, "one canonical submission");
    assert_eq!(
        completed_facts.prompt_submit_hooks, 1,
        "one UserPromptSubmit hook"
    );

    // --- rpc4: same immutable request after completion → consumed replay ---
    client.send_line(&prompt_line("rpc-4", prompt_text)).await;
    let consumed_replay = client
        .response_for_id(
            "rpc-4",
            "post-completion admission replay",
            Duration::from_secs(30),
        )
        .await;
    assert!(
        consumed_replay.get("error").is_none(),
        "post-completion replay must resolve: {consumed_replay}"
    );
    let consumed_meta = response_meta(&consumed_replay);
    let consumed_admission = &consumed_meta["grow/inputAdmission"];
    assert_eq!(
        consumed_admission["inputId"], input_id,
        "post-completion replay keeps the canonical identity: {consumed_admission}"
    );
    assert_eq!(
        consumed_admission["status"]["status"], "consumed",
        "terminal fold state must be observed as consumed: {consumed_admission}"
    );
    for forbidden in ["totalTokens", "usage", "structuredOutput"] {
        assert!(
            consumed_meta.get(forbidden).is_none(),
            "post-completion replay must not carry {forbidden}: {consumed_meta}"
        );
    }
    let final_facts = admission_facts(
        &timeline_domain_events(&timeline_path),
        &input_id,
        &prompt_id,
    );
    assert_eq!(
        final_facts.turn_started, completed_facts.turn_started,
        "post-completion replay starts no new turn"
    );
    assert_eq!(
        final_facts.submitted, completed_facts.submitted,
        "post-completion replay adds no submission"
    );
    assert_eq!(
        final_facts.prompt_submit_hooks, completed_facts.prompt_submit_hooks,
        "post-completion replay runs no new hook"
    );
    // Positive control on the roster stream: the legitimate completion did
    // publish a real Idle projection for this session (proving the stream is
    // live and the empty replay window above was not an artifact of a dead
    // subscription).
    let idle_deadline = tokio::time::Instant::now() + scaled(Duration::from_secs(30));
    loop {
        client.pump(Duration::from_millis(200)).await;
        if client
            .session_roster_deltas(&session_id)
            .iter()
            .any(|entry| entry["activity"] == "idle")
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < idle_deadline,
            "no Idle roster delta observed for this session after normal completion \
             (deltas: {:?})\nstderr:\n{}",
            client.session_roster_deltas(&session_id),
            stderr_tail(&client.stderr_text(), 1200),
        );
    }
}
