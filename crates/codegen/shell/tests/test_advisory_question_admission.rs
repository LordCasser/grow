//! Runtime admission integration: a real in-process `MvpAgent` on duplex ACP
//! pipes drives a loopback mock provider that emits an actual
//! `ask_user_question` tool call; the tool runs through the Factory-built
//! toolset, the shell worker, and a real ACP reverse request to this client
//! fixture, whose `Presented`/`Accepted` replies flow back so the provider
//! receives the next model request. Covers the milestone plan's admission
//! discrimination without hand-built `UserQuestionRequest`s or forged tool
//! outputs:
//!
//! 1. advisory (`blocking=false`, valid `recommended_option`): client sees the
//!    effective `blocking=false` wire flag, answers `Presented` only, and the
//!    provider's next request carries exactly one tool result for the call —
//!    the Agent recommendation with an explicit user-has-NOT-answered marker.
//! 2. the same alive session persists `allow_non_blocking=false` via the pub
//!    setter: the model still asks `blocking=false`, but the client now sees
//!    effective `blocking=true`; with the reverse request provably delivered
//!    and unanswered, a bounded observation window shows no further provider
//!    request; only the client's real `Accepted` unblocks the turn.
//! 3. re-enabling via the setter makes the next admission advisory again.
//!
//! The client-side ext handler is a protocol-peer fixture, not the Pager UI
//! (tracked separately); it records the typed `AskUserQuestionExtRequest` it
//! received and lets the test decide when to reply, providing the causal
//! barriers.
//!
//! Single `#[test]`: the env mutation relies on nothing else running
//! concurrently (same safety argument as `test_registry_churn`).

use std::sync::Arc;
use std::time::Duration;

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    LineBufferedRead,
};
use acp_transport::{AcpAgentHandler as _, AcpClientHandler, protocol as acp};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use shell::agent::mvp_agent::MvpAgent;
use tempfile::TempDir;
use test_support::{
    InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer, ScriptedResponse, SseEvent,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tools::implementations::grow_build::ask_user_question::AskUserQuestionExtRequest;

const MODEL: &str = "test-model";
const DUPLEX_BUFFER_BYTES: usize = 8 * 1024 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(60);
/// Bounded window proving a blocking admission holds the turn: if the turn
/// had (incorrectly) continued after `Presented`-style admission, the next
/// foreground provider request would land well inside this window.
const NO_CONTINUE_WINDOW: Duration = Duration::from_millis(750);

fn test_model_config(base_url: &str) -> String {
    format!(
        "[models]\ndefault = \"test/test-model\"\n\n\
         [provider.test]\napi_backend = \"chat_completions\"\n\n\
         [provider.test.options]\nbase_url = \"{base_url}\"\n\n\
         [provider.test.models.test-model]\nname = \"Test Model\"\ncontext_window = 128000\n"
    )
}

// ── Provider scripts ──────────────────────────────────────────────────────

fn ask_tool_call_response(call_id: &str, question: &str, recommended: &str) -> ScriptedResponse {
    let arguments = json!({
        "questions": [{
            "question": question,
            "options": [
                {"label": recommended, "description": "Short and direct"},
                {"label": "Detailed", "description": "Full detail"}
            ],
            "recommended_option": recommended
        }],
        "blocking": false,
    })
    .to_string();
    ScriptedResponse::sse(vec![
        SseEvent::data(
            json!({
                "id": call_id, "object": "chat.completion.chunk", "created": 0, "model": MODEL,
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": 0, "id": call_id, "type": "function",
                    "function": {"name": "ask_user_question", "arguments": arguments}
                }]}, "finish_reason": null}]
            })
            .to_string(),
        ),
        SseEvent::data(
            json!({
                "id": call_id, "object": "chat.completion.chunk", "created": 0, "model": MODEL,
                "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
            })
            .to_string(),
        ),
    ])
}

fn text_response(id: &str, text: &str) -> ScriptedResponse {
    ScriptedResponse::sse(vec![
        SseEvent::data(json!({
            "id": id, "object": "chat.completion.chunk", "created": 0, "model": MODEL,
            "choices": [{"index": 0, "delta": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }).to_string()),
        SseEvent::data("[DONE]"),
    ])
}

// ── Protocol-peer client fixture ──────────────────────────────────────────

/// One ask reverse request as the fixture received it, with the reply sender
/// the test controls. Holding the sender holds the worker's bounded wait.
struct AskAtClient {
    request: AskUserQuestionExtRequest,
    raw_params: Value,
    reply_tx: tokio::sync::oneshot::Sender<acp::ExtResponse>,
}

#[derive(Clone)]
struct AskPeerClient {
    asks: tokio::sync::mpsc::UnboundedSender<AskAtClient>,
}

#[async_trait::async_trait(?Send)]
impl AcpClientHandler for AskPeerClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        let outcome = args
            .options
            .iter()
            .find(|o| o.kind == acp::PermissionOptionKind::AllowOnce)
            .or(args.options.first())
            .map(|o| {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    o.option_id.clone(),
                ))
            })
            .unwrap_or(acp::RequestPermissionOutcome::Cancelled);
        Ok(acp::RequestPermissionResponse::new(outcome))
    }

    async fn session_notification(&self, _args: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }

    /// Deliver the reverse request to the test and relay its reply verbatim.
    async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        if args.method.as_ref() != "grow/ask_user_question" {
            return Err(acp_transport::acp_internal_error(format!(
                "unexpected ext method {}",
                args.method.as_ref()
            )));
        }
        let raw_params: Value = serde_json::from_str(args.params.get())
            .map_err(|e| acp_transport::acp_internal_error(e.to_string()))?;
        let typed: AskUserQuestionExtRequest = serde_json::from_value(raw_params.clone())
            .map_err(|e| acp_transport::acp_internal_error(e.to_string()))?;
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.asks
            .send(AskAtClient {
                request: typed,
                raw_params,
                reply_tx,
            })
            .map_err(|_| acp_transport::acp_internal_error("ask peer dropped"))?;
        reply_rx
            .await
            .map_err(|_| acp_transport::acp_internal_error("ask peer dropped the reply"))
    }
}

fn presented_reply() -> acp::ExtResponse {
    let raw = serde_json::value::to_raw_value(&json!({"outcome": "presented"})).expect("serialize");
    acp::ExtResponse::new(raw.into())
}

fn accepted_reply(question: &str, answer: &str) -> acp::ExtResponse {
    let raw = serde_json::value::to_raw_value(&json!({
        "outcome": "accepted",
        "answers": { question: [answer] }
    }))
    .expect("serialize");
    acp::ExtResponse::new(raw.into())
}

// ── Harness (mirrors test_registry_churn) ─────────────────────────────────

async fn connect_and_auth(
    base_url: &str,
    asks: tokio::sync::mpsc::UnboundedSender<AskAtClient>,
) -> acp_transport::ClientSideConnection {
    let raw: toml::Value =
        toml::from_str(&test_model_config(base_url)).expect("valid explicit test model TOML");
    let mut agent_config =
        AgentConfig::new_from_toml_cfg(&raw).expect("valid explicit test config");
    agent_config.models.default = Some("test/test-model".to_owned());
    agent_config.default_model_override = None;
    agent_config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    let (gw_tx, gw_rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(gw_tx);
    let agent = MvpAgent::new(gateway, &agent_config).expect("valid config");
    let (c2a_a, c2a_b) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let (a2c_a, a2c_b) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let agent_incoming = LineBufferedRead::spawn_local(c2a_b.compat());
    let (agent_conn, agent_io) =
        acp_transport::connect_agent_v1(agent, a2c_a.compat_write(), agent_incoming, |fut| {
            tokio::task::spawn_local(fut);
        });
    tokio::task::spawn_local(GatewayReceiver::new(gw_rx, agent_conn).run());
    tokio::task::spawn_local(agent_io);
    let client_incoming = LineBufferedRead::spawn_local(a2c_b.compat());
    let (client_conn, client_io) = acp_transport::connect_client_v1(
        AskPeerClient { asks },
        c2a_a.compat_write(),
        client_incoming,
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );
    tokio::task::spawn_local(client_io);
    let init = tokio::time::timeout(
        RPC_TIMEOUT,
        client_conn.initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_capabilities(
                acp::ClientCapabilities::new()
                    .fs(acp::FileSystemCapabilities::new())
                    .terminal(false),
            ),
        ),
    )
    .await
    .expect("initialize timed out")
    .expect("initialize failed");
    let method = init
        .auth_methods
        .iter()
        .find(|m| &*m.id().0 == "provider.api_key")
        .expect("provider.api_key auth method not advertised");
    tokio::time::timeout(
        RPC_TIMEOUT,
        client_conn.authenticate(
            acp::AuthenticateRequest::new(method.id().clone())
                .meta(json!({ "headless": true }).as_object().cloned()),
        ),
    )
    .await
    .expect("authenticate timed out")
    .expect("authenticate failed");
    client_conn
}

async fn new_session(
    conn: &acp_transport::ClientSideConnection,
    cwd: &std::path::Path,
) -> acp::SessionId {
    tokio::time::timeout(
        RPC_TIMEOUT,
        conn.new_session(
            acp::NewSessionRequest::new(cwd.to_path_buf())
                .meta(json!({ "modelId": MODEL }).as_object().cloned()),
        ),
    )
    .await
    .expect("session/new timed out")
    .expect("session/new failed")
    .session_id
}

async fn prompt_turn(
    conn: &acp_transport::ClientSideConnection,
    session_id: &acp::SessionId,
    text: &str,
) {
    let resp = tokio::time::timeout(
        RPC_TIMEOUT,
        conn.prompt(acp::PromptRequest::new(
            session_id.clone(),
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                text.to_owned(),
            ))],
        )),
    )
    .await
    .unwrap_or_else(|_| panic!("prompt on {} timed out", session_id.0))
    .unwrap_or_else(|e| panic!("prompt on {} failed: {}", session_id.0, e));
    assert!(
        matches!(resp.stop_reason, acp::StopReason::EndTurn),
        "expected EndTurn on {}, got {:?}",
        session_id.0,
        resp.stop_reason
    );
}

/// Foreground chat-completions request bodies, in arrival order.
fn foreground_bodies(server: &MockInferenceServer) -> Vec<Value> {
    server
        .requests()
        .into_iter()
        .filter(|entry| entry.path == "/v1/chat/completions" && entry.body.is_some())
        .filter_map(|entry| entry.body)
        .collect()
}

/// Tool-result settlement count for one tool call id inside one request body.
/// History repetition across *later* requests is expected; more than one
/// inside a single request would be a double settlement.
fn tool_result_count(body: &Value, call_id: &str) -> usize {
    body["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter(|m| m["role"] == "tool" && m["tool_call_id"].as_str() == Some(call_id))
                .count()
        })
        .unwrap_or(0)
}

fn assistant_ask_call_count(body: &Value, call_id: &str) -> usize {
    body["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter(|m| m["role"] == "assistant")
                .map(|m| {
                    m["tool_calls"]
                        .as_array()
                        .map(|calls| {
                            calls
                                .iter()
                                .filter(|c| c["id"].as_str() == Some(call_id))
                                .count()
                        })
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Wait for the next ask reverse request at the client fixture.
async fn next_ask(asks: &mut tokio::sync::mpsc::UnboundedReceiver<AskAtClient>) -> AskAtClient {
    tokio::time::timeout(RPC_TIMEOUT, asks.recv())
        .await
        .expect("ask reverse request timed out")
        .expect("ask peer channel closed")
}

// ── Test ──────────────────────────────────────────────────────────────────

#[test]
fn advisory_ask_admits_bounded_presentation_and_continues_through_real_chain() {
    diagnostics::tls::install_ring_provider_once();
    let mock_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("mock runtime");
    let server = mock_rt
        .block_on(MockInferenceServer::start())
        .expect("mock server");
    let grow_home = TempDir::new().expect("grow home");
    let workdir = TempDir::new().expect("workdir");
    unsafe {
        std::env::set_var("GROW_HOME", grow_home.path());
        std::env::set_var("GROW_CLI_CHAT_PROXY_BASE_URL", server.url());
        std::env::set_var("GROW_INFERENCE_BASE_URL", server.url());
        std::env::set_var("GROW_API_KEY", "test-key-for-ci");
    }
    std::fs::write(
        grow_home.path().join("config.toml"),
        test_model_config(&server.url()),
    )
    .expect("write explicit test model config");

    let matcher = InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions);
    // FIFO in registration order; the foreground matcher consumes them one
    // foreground request at a time.
    let ask_advisory = server.expect_response(
        "advisory ask tool call",
        matcher,
        ask_tool_call_response("call-advisory-1", "Pick a tone?", "Concise"),
    );
    let cont_a = server.expect_response(
        "continuation after presented",
        matcher,
        text_response("resp-cont-a", "ADVISORY_CONTINUATION"),
    );
    let ask_disabled = server.expect_response(
        "ask while preference disabled",
        matcher,
        ask_tool_call_response("call-blocking-2", "Pick a tone again?", "Concise"),
    );
    let cont_b = server.expect_response(
        "continuation after real accept",
        matcher,
        text_response("resp-cont-b", "BLOCKING_CONTINUATION"),
    );
    let ask_reenabled = server.expect_response(
        "ask after re-enabling",
        matcher,
        ask_tool_call_response("call-advisory-3", "Pick once more?", "Concise"),
    );
    let cont_c = server.expect_response(
        "continuation after re-enable",
        matcher,
        text_response("resp-cont-c", "READVISORY_CONTINUATION"),
    );
    let ask_invalid = server.expect_response(
        "ask under typed-invalid policy",
        matcher,
        ask_tool_call_response("call-advisory-4", "Pick despite bad config?", "Concise"),
    );
    let cont_d = server.expect_response(
        "continuation after policy error",
        matcher,
        text_response("resp-cont-d", "POLICY_ERROR_CONTINUATION"),
    );

    let agent_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("agent runtime");
    let local = tokio::task::LocalSet::new();
    agent_rt.block_on(local.run_until(async move {
        let (asks_tx, mut asks_rx) = tokio::sync::mpsc::unbounded_channel();
        let client_conn = connect_and_auth(&server.url(), asks_tx).await;
        let session = new_session(&client_conn, workdir.path()).await;

        // ── Scenario A: advisory admission ────────────────────────────────
        let prompt_a = tokio::task::spawn_local({
            let conn = client_conn.clone();
            let sid = session.clone();
            async move { prompt_turn(&conn, &sid, "ask me an advisory question").await }
        });
        let ask = next_ask(&mut asks_rx).await;
        assert_eq!(
            ask.request.blocking, false,
            "wire must carry the effective advisory flag"
        );
        assert_eq!(ask.request.questions.len(), 1);
        assert_eq!(
            ask.request.questions[0].recommended_option.as_deref(),
            Some("Concise")
        );
        assert_eq!(ask.raw_params["toolCallId"], "call-advisory-1");
        ask.reply_tx
            .send(presented_reply())
            .expect("reply channel open");
        prompt_a.await.expect("scenario A prompt join");
        ask_advisory.assert_satisfied();
        cont_a.assert_satisfied();

        // The continuation request must carry exactly one settlement for the
        // call, marked as the Agent's recommendation with no user answer.
        let bodies = foreground_bodies(&server);
        let cont_body = bodies
            .iter()
            .rev()
            .find(|b| {
                b["messages"]
                    .as_array()
                    .is_some_and(|ms| ms.iter().any(|m| m["role"] == "tool"))
            })
            .expect("continuation request with a tool result");
        assert_eq!(tool_result_count(cont_body, "call-advisory-1"), 1);
        assert_eq!(assistant_ask_call_count(cont_body, "call-advisory-1"), 1);
        let tool_text = cont_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == "call-advisory-1")
            .map(|m| m["content"].to_string())
            .expect("advisory tool result present");
        assert!(
            tool_text.contains("the user has NOT answered"),
            "result must be marked as not a user answer: {tool_text}"
        );
        assert!(
            tool_text.contains("\\\"Concise\\\"") || tool_text.contains("Concise"),
            "result must name the recommendation: {tool_text}"
        );

        // ── Scenario B: pub setter false, same alive session ─────────────
        shell::util::config::set_ask_user_question_allow_non_blocking(false)
            .await
            .expect("persist allow_non_blocking=false");
        let prompt_b = tokio::task::spawn_local({
            let conn = client_conn.clone();
            let sid = session.clone();
            async move { prompt_turn(&conn, &sid, "ask me again advisory").await }
        });
        let ask = next_ask(&mut asks_rx).await;
        assert_eq!(
            ask.request.blocking, true,
            "disabled preference must force the blocking form on the wire"
        );
        assert_eq!(ask.raw_params["toolCallId"], "call-blocking-2");
        // Causal barrier part 1: the ask provably reached this client above.
        // Part 2: bounded window with no further provider request while the
        // real reverse future is unanswered.
        let during_wait = foreground_bodies(&server).len();
        tokio::time::sleep(NO_CONTINUE_WINDOW).await;
        assert_eq!(
            foreground_bodies(&server).len(),
            during_wait,
            "no next provider request may arrive while the blocking ask is unanswered"
        );
        // A real user answer releases the turn.
        ask.reply_tx
            .send(accepted_reply("Pick a tone again?", "Concise"))
            .expect("reply channel open");
        prompt_b.await.expect("scenario B prompt join");
        ask_disabled.assert_satisfied();
        cont_b.assert_satisfied();
        let bodies = foreground_bodies(&server);
        let cont_b_body = bodies.last().expect("post-accept request");
        assert_eq!(tool_result_count(cont_b_body, "call-blocking-2"), 1);
        let answered_text = cont_b_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == "call-blocking-2")
            .map(|m| m["content"].to_string())
            .expect("blocking tool result present");
        assert!(
            answered_text.contains("User has answered"),
            "accepted blocking answer must be the user result: {answered_text}"
        );
        // The earlier advisory settlement is history, not a re-settlement:
        // still exactly one tool result per call in the canonical request.
        assert_eq!(tool_result_count(cont_b_body, "call-advisory-1"), 1);

        // ── Scenario C: re-enable, next admission is advisory again ──────
        shell::util::config::set_ask_user_question_allow_non_blocking(true)
            .await
            .expect("persist allow_non_blocking=true");
        let prompt_c = tokio::task::spawn_local({
            let conn = client_conn.clone();
            let sid = session.clone();
            async move { prompt_turn(&conn, &sid, "one more advisory").await }
        });
        let ask = next_ask(&mut asks_rx).await;
        assert_eq!(
            ask.request.blocking, false,
            "re-enabling must restore the advisory admission"
        );
        ask.reply_tx
            .send(presented_reply())
            .expect("reply channel open");
        prompt_c.await.expect("scenario C prompt join");
        ask_reenabled.assert_satisfied();
        cont_c.assert_satisfied();

        // ── Scenario D: typed-invalid policy fails before publication ────
        // A structurally valid file whose `allow_non_blocking` value has the
        // wrong type must make the tool call fail as a policy error before
        // any Shell reverse request is published — the failure must not
        // become an advisory presentation or a user answer, and the turn
        // only continues after the model sees the failed tool result.
        let config_path = shell::util::config::user_config_path();
        std::fs::write(
            &config_path,
            "[toolset.ask_user_question]\nallow_non_blocking = \"false\"\n",
        )
        .expect("write typed-invalid policy");
        let prompt_d = tokio::task::spawn_local({
            let conn = client_conn.clone();
            let sid = session.clone();
            async move { prompt_turn(&conn, &sid, "ask under a broken policy").await }
        });
        // No ask reverse request may reach this client for the whole bounded
        // window: the resolver error precedes enqueue/publication.
        assert!(
            tokio::time::timeout(Duration::from_secs(2), asks_rx.recv())
                .await
                .is_err(),
            "typed-invalid policy must not publish an ask reverse request"
        );
        prompt_d.await.expect("scenario D prompt join");
        ask_invalid.assert_satisfied();
        cont_d.assert_satisfied();
        let final_body = foreground_bodies(&server)
            .last()
            .expect("final request")
            .clone();
        let policy_tool_text = final_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == "call-advisory-4")
            .map(|m| m["content"].to_string())
            .expect("policy-failed tool result present");
        assert!(
            policy_tool_text.contains("policy"),
            "result must be a policy error, got: {policy_tool_text}"
        );
        assert!(
            !policy_tool_text.contains("the user has NOT answered"),
            "a policy error must never masquerade as an advisory presentation: {policy_tool_text}"
        );
        for call_id in [
            "call-advisory-1",
            "call-blocking-2",
            "call-advisory-3",
            "call-advisory-4",
        ] {
            assert_eq!(
                tool_result_count(&final_body, call_id),
                1,
                "canonical timeline must carry exactly one settlement per ask call"
            );
        }

        // ── Route probes: `grow/input/admission_status` over public ACP ───
        // The backend handler lives in `extensions/session_admin`; this
        // probes the top-level `ext_method` routing through the real client
        // connection (no direct handler calls).
        let probe_status = |params: Value| {
            let conn = client_conn.clone();
            async move {
                let raw = serde_json::value::RawValue::from_string(params.to_string())
                    .expect("serialize probe params");
                conn.ext_method(acp::ExtRequest::new(
                    "grow/input/admission_status",
                    Arc::from(raw),
                ))
                .await
            }
        };
        // Same alive session, a stable question-reply prompt id that was
        // never Submitted: durable observation must be unknown_unconfirmed
        // (no record is not proof of absence), with exact identity echo.
        let prompt_id = format!("question-reply:{}:call-advisory-1", session.0.as_ref());
        let response = probe_status(json!({
            "sessionId": session.0.as_ref(),
            "promptId": prompt_id,
        }))
        .await
        .expect("grow/input/admission_status must be routed by the top-level ext_method");
        let body: Value =
            serde_json::from_str(response.0.get()).expect("typed admission status response");
        // The observation enum is internally tagged on "status", so the
        // wire nests one level: status.status is the observation name.
        assert_eq!(body["status"]["status"], "unknown_unconfirmed");
        assert_eq!(body["sessionId"], session.0.as_ref());
        assert_eq!(body["promptId"], prompt_id);
        assert_eq!(body["source"], "durable");
        // Missing session: existing contract returns Absent.
        let response = probe_status(json!({
            "sessionId": "no-such-session-id",
            "promptId": prompt_id,
        }))
        .await
        .expect("probe must be routed");
        let body: Value = serde_json::from_str(response.0.get()).unwrap();
        assert_eq!(body["status"]["status"], "absent");
        assert_eq!(body["source"], "durable");
        // Invalid empty promptId: handler invalid_params, not method missing.
        let error = probe_status(json!({
            "sessionId": session.0.as_ref(),
            "promptId": "",
        }))
        .await
        .expect_err("empty promptId must be rejected by the handler");
        let wire: Value = serde_json::to_value(&error).unwrap();
        assert_eq!(
            wire["code"].as_i64(),
            Some(-32602),
            "invalid identity must be invalid_params, got {wire}"
        );
    }));
}
