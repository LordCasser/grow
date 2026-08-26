//! Truncation-recovery integration tests (Task 3): the turn-loop branches
//! for the `Length` / `ModelContextWindowExceeded` / `PauseTurn` stop
//! reasons.
//!
//! Each test drives the real turn loop (`handle_prompt`) through a real
//! `sampler::SamplerActor` against a `MockInferenceServer`, scripting
//! per-request Messages-API SSE responses so every sampling cycle can carry
//! its own stop_reason. The conversation history is inspected via the chat
//! state handle — the assertion target mandated by architecture section
//! 12.4 (the partial output must be observable in history, not just the
//! outcome classification).

use super::support::*;
use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};
use tokio::sync::mpsc;

fn prompt(text: impl Into<String>, index: usize) -> ConversationItem {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(index);
    item
}

/// Wire `stop_reason` values for the Messages API (see
/// `sampler/src/stream/messages.rs` mapping).
const MAX_TOKENS: &str = "max_tokens";
const END_TURN: &str = "end_turn";
const CONTEXT_WINDOW_EXCEEDED: &str = "model_context_window_exceeded";
const PAUSE_TURN: &str = "pause_turn";

// ─── Messages-API SSE builders ────────────────────────────────────────────

/// A text `content_block`.
fn text_block(text: &str) -> serde_json::Value {
    json!({ "type": "text", "text": text })
}

/// A `thinking` block. The wire streams `thinking` and `signature` via
/// separate delta events; `complete` controls whether the terminating
/// `content_block_stop` is emitted (an incomplete block is discarded by the
/// stream layer — Anthropic's "thinking blocks cannot be partially
/// recovered" constraint).
fn thinking_block(text: &str, signature: &str) -> serde_json::Value {
    json!({
        "type": "thinking",
        "thinking": text,
        "signature": signature,
    })
}

/// A `tool_use` block; `complete` controls whether the terminating
/// `content_block_stop` is emitted (incomplete tool_use is discarded).
fn tool_use_block(id: &str, name: &str, input_json: &str) -> serde_json::Value {
    json!({
        "type": "tool_use",
        "id": id,
        "name": name,
        "input": input_json,
    })
}

/// Like [`tool_use_block`] but the builder omits the terminating
/// `content_block_stop` (via the internal `__no_stop` marker, stripped when
/// assembling the wire), so the stream layer discards the block — the
/// "in-progress tool_use at max_tokens" wire shape.
fn tool_use_block_incomplete(id: &str, name: &str, input_json: &str) -> serde_json::Value {
    let mut block = tool_use_block(id, name, input_json);
    block["__no_stop"] = json!(true);
    block
}

/// Assemble a Messages-API SSE turn from content blocks and a terminal
/// `stop_reason`. Each block is streamed as start → delta(s) → stop; the
/// stream ends with `message_delta` carrying `stop_reason` + `message_stop`.
fn messages_turn(blocks: &[serde_json::Value], stop_reason: &str) -> ScriptedResponse {
    let mut events: Vec<String> = vec![
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_test", "type": "message", "role": "assistant",
                "content": [], "model": "test-model", "stop_reason": null,
                "usage": {
                    "input_tokens": 10, "output_tokens": 0,
                    "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
                }
            }
        })
        .to_string(),
    ];
    for (i, block) in blocks.iter().enumerate() {
        let block_type = block["type"].as_str().expect("block has a type");
        // The start event must NOT carry content: the stream layer seeds
        // block accumulators from it (messages.rs `ContentBlockStart`), and
        // the real Anthropic wire sends content only via deltas — echoing it
        // in the start event would double-count every block.
        let start_block = match block_type {
            "text" => json!({ "type": "text", "text": "" }),
            "thinking" => json!({ "type": "thinking", "thinking": "", "signature": "" }),
            "tool_use" => {
                json!({ "type": "tool_use", "id": block["id"], "name": block["name"], "input": {} })
            }
            other => panic!("unsupported block type {other:?}"),
        };
        events.push(
            json!({ "type": "content_block_start", "index": i, "content_block": start_block })
                .to_string(),
        );
        match block_type {
            "text" => {
                events.push(
                    json!({
                        "type": "content_block_delta",
                        "index": i,
                        "delta": { "type": "text_delta", "text": block["text"] }
                    })
                    .to_string(),
                );
            }
            "thinking" => {
                events.push(
                    json!({
                        "type": "content_block_delta",
                        "index": i,
                        "delta": {
                            "type": "thinking_delta",
                            "thinking": block["thinking"]
                        }
                    })
                    .to_string(),
                );
                events.push(
                    json!({
                        "type": "content_block_delta",
                        "index": i,
                        "delta": {
                            "type": "signature_delta",
                            "signature": block["signature"]
                        }
                    })
                    .to_string(),
                );
            }
            "tool_use" => {
                events.push(
                    json!({
                        "type": "content_block_delta",
                        "index": i,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": block["input"]
                        }
                    })
                    .to_string(),
                );
            }
            other => panic!("unsupported block type {other:?}"),
        }
        // Blocks marked `__no_stop` (incomplete at the cut-off) get no
        // terminating `content_block_stop`, so the stream layer discards
        // them; the marker itself is internal and never enters the wire.
        if block.get("__no_stop").and_then(serde_json::Value::as_bool) != Some(true) {
            events.push(json!({ "type": "content_block_stop", "index": i }).to_string());
        }
    }
    events.push(
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason },
            "usage": { "output_tokens": 5, "input_tokens": 10 }
        })
        .to_string(),
    );
    events.push(json!({ "type": "message_stop" }).to_string());
    ScriptedResponse::sse(events.into_iter().map(SseEvent::data).collect())
}

// ─── Chat Completions SSE builder ────────────────────────────────────────

/// Assemble a Chat Completions SSE turn: one content chunk, a terminal
/// `finish_reason` chunk, a usage chunk, then `[DONE]`. Wire shape pinned
/// against sampler's `ChatCompletionChunk` deserialization and
/// `stream/chat_completions.rs` field mapping (see also
/// `test-support/src/sse.rs` `chat_completion_script_from_deltas`):
/// `finish_reason: "length"` maps to `StopReason::Length`, which the
/// unified turn loop feeds into the same continue branch as the Messages
/// `max_tokens` stop reason.
fn chat_completions_turn(text: &str, finish_reason: Option<&str>) -> ScriptedResponse {
    let mut events: Vec<String> = vec![
        json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "content": text },
                "finish_reason": null
            }]
        })
        .to_string(),
    ];
    events.push(
        json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": finish_reason
            }]
        })
        .to_string(),
    );
    events.push(
        json!({
            "id": "chatcmpl-test",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "test-model",
            "choices": [],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })
        .to_string(),
    );
    events.push("[DONE]".to_string());
    ScriptedResponse::sse(events.into_iter().map(SseEvent::data).collect())
}

// ─── Actor fixture ────────────────────────────────────────────────────────

/// Run a test body on a dedicated thread with an 8MB stack, matching
/// `SESSION_THREAD_STACK_SIZE` in spawn.rs. The turn loop's async state
/// machine chain needs 2–4MB (the `handle_prompt` frame alone is ~63KB and
/// mock servers complete synchronously, stacking several turn iterations
/// into one poll); the default 2MB test thread stack overflows and aborts.
/// `resume_unwind` re-raises the body's panic on the test thread so the
/// original assertion message survives (plain `join().unwrap()` would
/// collapse it to `Any { .. }`).
fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

/// Build a `SessionActor` whose sampler is a real `SamplerActor` pointed at
/// `server`, with the sampler event drainer running (the production spawn
/// wires it; the bare test actor does not). `api_backend` selects the wire
/// (and thus the mock-server endpoint) for the whole actor. Returns the
/// actor and the gateway receiver for observing outbound client messages.
async fn actor_with_sampler(
    server: &MockInferenceServer,
    api_backend: sampling_types::ApiBackend,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            if let PersistenceMsg::SidebandDurablyAndAck { respond_to, .. } = message {
                let _ = respond_to.send(Ok(()));
            }
        }
    });
    let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    // Point the chat-state sampling config at the mock server so BOTH the
    // main turn requests and the compaction client (which rebuilds its
    // config from chat state) hit the same server.
    if let Some(mut cfg) = actor.chat_state_handle.get_sampling_config().await {
        cfg.base_url = server.url();
        cfg.api_backend = api_backend.clone();
        actor.chat_state_handle.update_sampling_config(cfg);
    }
    let sampler_config = sampler::SamplerConfig {
        api_key: Some("test-key".to_string()),
        base_url: server.url(),
        model: "test-model".to_string(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend,
        auth_scheme: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: 100_000,
        force_http1: false,
        max_retries: Some(0),
        stream_tool_calls: false,
        idle_timeout_secs: Some(60),
        reasoning_effort: None,
        origin_client: None,
        attribution_callback: None,
        bearer_resolver: None,
        compactions_remaining: None,
        compaction_at_tokens: None,
        doom_loop_recovery: None,
    };
    let (sampler_event_tx, sampler_event_rx) = mpsc::unbounded_channel::<sampler::SamplingEvent>();
    actor.sampler_handle = sampler::SamplerActor::spawn(
        sampler_config,
        sampler::RetryPolicy::default(),
        sampler_event_tx,
    );
    let actor = std::sync::Arc::new(actor);
    let drainer = actor.clone();
    tokio::task::spawn_local(async move {
        let mut sampler_event_rx = sampler_event_rx;
        while let Some(event) = sampler_event_rx.recv().await {
            drainer.handle_sampling_event(event).await;
        }
    });
    (actor, gateway_rx)
}

/// Drive one user turn through the real loop. Bounded by a generous timeout
/// so a broken continue loop fails the test instead of hanging it.
async fn run_user_turn(
    actor: &std::sync::Arc<SessionActor>,
    prompt_id: &str,
) -> Result<PromptTurnOk, acp::Error> {
    install_test_foreground(actor, prompt_id).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        actor.handle_prompt(
            prompt_id,
            crate::session::PromptOrigin::User,
            Vec::new(),
            crate::session::TurnKind::User,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                "write a long answer",
            ))],
            tool_types::BehaviorId::Normal,
            None,
            None,
            false,
            None,
            None,
        ),
    )
    .await
    .expect("turn must finish within the timeout (no runaway continue loop)")
}

/// Partial compaction needs an old closed range and a recent verbatim tail;
/// the current overflow turn alone is deliberately never summarized whole.
async fn seed_closed_compaction_range(actor: &SessionActor) {
    replace_test_surface(
        &actor.chat_state_handle,
        vec![ConversationItem::system("test system prompt")],
    )
    .await;
    actor
        .chat_state_handle
        .push_user_message(prompt("old closed turn", 0));
    actor
        .chat_state_handle
        .push_assistant_response(ConversationItem::assistant("x".repeat(24_000)));
    actor
        .chat_state_handle
        .push_user_message(prompt("recent retained turn", 1));
    actor
        .chat_state_handle
        .push_assistant_response(ConversationItem::assistant("y".repeat(210_000)));
    let _ = actor.chat_state_handle.get_conversation_len().await;
}

/// Collect the persisted `ConversationItem::Assistant` texts in order.
fn assistant_texts(conversation: &[ConversationItem]) -> Vec<String> {
    conversation
        .iter()
        .filter_map(|item| match item {
            ConversationItem::Assistant(a) => Some(a.content.to_string()),
            _ => None,
        })
        .collect()
}

/// Count injected truncation-continue items (User items tagged
/// `SyntheticReason::TruncationContinue` carrying the exact prompt).
fn truncation_continue_count(conversation: &[ConversationItem]) -> usize {
    conversation
        .iter()
        .filter(|item| match item {
            ConversationItem::User(u) => {
                u.synthetic_reason == Some(SyntheticReason::TruncationContinue)
                    && item.text_content()
                        == chat_state::compaction_utils::TRUNCATION_CONTINUE_PROMPT
            }
            _ => false,
        })
        .count()
}

/// Drain the gateway receiver, returning every `grow/hooks/event` hook name
/// observed (the client-hook observation channel used by `notify_client_hooks`).
fn drain_hook_event_names(
    rx: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> Vec<String> {
    let mut events = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
            && args.request.method.as_ref() == "grow/hooks/event"
        {
            let params: serde_json::Value =
                serde_json::from_str(args.request.params.get()).unwrap();
            if let Some(name) = params["hookEventName"].as_str() {
                events.push(name.to_string());
            }
        }
    }
    events
}

/// Number of `POST /v1/chat/completions` requests received so far (the
/// Chat Completions twin of `MockInferenceServer::messages_request_count`).
fn chat_completions_request_count(server: &MockInferenceServer) -> usize {
    server
        .requests()
        .iter()
        .filter(|e| e.path == "/v1/chat/completions")
        .count()
}

// ─── Tests ────────────────────────────────────────────────────────────────

/// E2E truncation continue: mock truncates once at `max_tokens`; assert the
/// partial response is persisted, the continue prompt is injected with
/// `SyntheticReason::TruncationContinue`, the final output is the
/// concatenation of both partials, and the turn completes successfully.
#[test]
fn truncation_auto_continue_e2e() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part one ")], MAX_TOKENS),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part two")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;

            let result = run_user_turn(&actor, "trunc-e2e").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // (1) Partial response persisted into conversation history.
            let texts = assistant_texts(&conversation);
            assert!(
                texts.iter().any(|t| t.contains("part one")),
                "partial response must be persisted, got {texts:?}"
            );
            // (2) Continue prompt injected with the synthetic reason.
            assert_eq!(
                truncation_continue_count(&conversation),
                1,
                "exactly one truncation_continue item must be injected"
            );
            // (3) Final output is the concatenation of the partials.
            let joined: String = texts.concat();
            assert!(
                joined.contains("part one") && joined.contains("part two"),
                "final assistant output must contain both partials, got {joined:?}"
            );
            // (4) Two sampling cycles happened (truncated + final).
            assert_eq!(server.messages_request_count(), 2);
        }));
    });
}

/// Multiple truncations: 3 truncated cycles then a completion; all partials
/// must be persisted and merged, with one continue injection per truncation.
#[test]
fn truncation_multiple_continues() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            for part in ["one ", "two ", "three "] {
                server.enqueue_response(
                    "/v1/messages",
                    messages_turn(&[text_block(part)], MAX_TOKENS),
                );
            }
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("four")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;

            let result = run_user_turn(&actor, "trunc-multi").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            let joined: String = assistant_texts(&conversation).concat();
            for part in ["one ", "two ", "three ", "four"] {
                assert!(
                    joined.contains(part),
                    "final output must contain {part:?}, got {joined:?}"
                );
            }
            assert_eq!(
                truncation_continue_count(&conversation),
                3,
                "one continue injection per truncation"
            );
            assert_eq!(server.messages_request_count(), 4);
        }));
    });
}

/// Thinking blocks: a complete thinking block survives into history (with
/// its signature); an in-progress thinking block (started, deltas streamed,
/// no `content_block_stop`) is discarded. The truncation then continues.
#[test]
fn truncation_thinking_block() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[
                        thinking_block("partial thinking", "sig_partial"),
                        thinking_block("complete thinking", "sig_complete"),
                        text_block("part one"),
                    ],
                    MAX_TOKENS,
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part two")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;

            let result = run_user_turn(&actor, "trunc-think").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // The complete thinking block is retained as a Reasoning sibling
            // with its signature; the incomplete one is nowhere to be found.
            let reasoning_texts: Vec<String> = conversation
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::Reasoning(r) => Some(format!(
                        "sig={:?} text={}",
                        r.encrypted_content,
                        r.summary
                            .iter()
                            .map(|p| match p {
                                sampling_types::rs::SummaryPart::SummaryText(s) => {
                                    s.text.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("")
                    )),
                    _ => None,
                })
                .collect();
            assert!(
                reasoning_texts
                    .iter()
                    .any(|r| r.contains("complete thinking") && r.contains("sig_complete")),
                "complete thinking block with signature must be retained, got {reasoning_texts:?}"
            );
            assert!(
                !reasoning_texts
                    .iter()
                    .any(|r| r.contains("partial thinking")),
                "incomplete thinking block must be discarded, got {reasoning_texts:?}"
            );
            assert_eq!(truncation_continue_count(&conversation), 1);
            let joined: String = assistant_texts(&conversation).concat();
            assert!(joined.contains("part one") && joined.contains("part two"));
        }));
    });
}

/// Tool-use truncation: an in-progress tool_use block (start + partial JSON,
/// no `content_block_stop`) is discarded — the persisted assistant has no
/// tool calls, the next request does not carry the partial call, and the
/// turn continues via the truncation branch.
#[test]
fn truncation_tool_use_incomplete() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[
                        tool_use_block_incomplete("call_partial", "read_file", "{\"path\":\""),
                        text_block("part one"),
                    ],
                    MAX_TOKENS,
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part two")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;

            let result = run_user_turn(&actor, "trunc-tool").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // The first assistant item kept the partial TEXT but no tool call.
            let assistant_items: Vec<&sampling_types::AssistantItem> = conversation
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::Assistant(a) => Some(a),
                    _ => None,
                })
                .collect();
            assert!(
                assistant_items[0].content.contains("part one"),
                "partial text must be persisted, got {:?}",
                assistant_items[0].content
            );
            assert!(
                assistant_items.iter().all(|a| a.tool_calls.is_empty()),
                "the incomplete tool_use must not surface as a tool call"
            );
            assert_eq!(truncation_continue_count(&conversation), 1);
            // The follow-up request must not carry the partial tool_use id.
            let requests = server.requests();
            assert_eq!(requests.len(), 2, "one truncated + one final request");
            let second_body = requests[1]
                .body
                .as_ref()
                .expect("second request must have a body");
            let serialized = serde_json::to_string(second_body).unwrap();
            assert!(
                !serialized.contains("call_partial"),
                "the incomplete tool_use must not be resent, body: {serialized}"
            );
        }));
    });
}

/// A COMPLETED tool_use block wins over truncation (tool_use wins): a full
/// tool call streamed in the same cycle as a `max_tokens` cut-off is
/// executed rather than triggering another truncation continue. The
/// continue prompt is injected exactly once (for the earlier text-only
/// truncation).
#[test]
fn truncation_tool_use_complete_wins_over_length() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part one ")], MAX_TOKENS),
            );
            // Wire stop_reason is max_tokens, but the completed tool_use is
            // real output: the stream layer overrides the stop reason to
            // ToolCalls, so the session executes the call instead of
            // continuing.
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[tool_use_block(
                        "call_full",
                        "todo_write",
                        r#"{"todos":[{"id":"t1","content":"do","status":"completed"}]}"#,
                    )],
                    MAX_TOKENS,
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("done")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            let result = run_user_turn(&actor, "trunc-tool-wins").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // Exactly one continue injection (for the text-only truncation);
            // the tool_use cycle did NOT continue.
            assert_eq!(truncation_continue_count(&conversation), 1);
            // The completed tool call and its result are in history.
            assert!(
                conversation
                    .iter()
                    .any(|item| matches!(item, ConversationItem::Assistant(a)
                        if a.tool_calls.iter().any(|tc| tc.id.as_ref() == "call_full"))),
                "completed tool_use must be persisted and executed"
            );
            assert!(
                conversation
                    .iter()
                    .any(|item| matches!(item, ConversationItem::ToolResult(t)
                        if t.tool_call_id == "call_full")),
                "tool result must be persisted after execution"
            );
        }));
    });
}

/// `model_context_window_exceeded` triggers compaction (not a continue
/// prompt): the compact request runs against the same server, and the loop
/// rebuilds from the compacted conversation.
#[test]
fn context_window_exceeded_triggers_compaction() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part one")], CONTEXT_WINDOW_EXCEEDED),
            );
            // The compaction request (same /v1/messages path) consumes the
            // next scripted response. The summary must clear the sampler's
            // `is_degenerate_summary` gate (cleaned seed >= 500 chars,
            // compaction MIN_SUMMARY_SEED_CHARS) or the retry loop
            // burns the remaining scripts.
            let long_summary = format!(
                "compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block(&long_summary)], END_TURN),
            );
            // The rebuilt main turn completes.
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("after compact")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            seed_closed_compaction_range(&actor).await;

            let result = run_user_turn(&actor, "ctx-exceeded").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // Compaction must have been triggered: no continue prompt, and
            // the compacted summary is in history. The compaction pipeline
            // persists the summary as a `CompactionMeta` user message (the
            // standard rebuilt-history shape), so scan the full conversation
            // rather than assistant items only.
            assert_eq!(
                truncation_continue_count(&conversation),
                0,
                "context-window overflow must trigger compaction, not a continue prompt"
            );
            let full_text: String = conversation
                .iter()
                .map(|item| item.text_content())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                full_text.contains("compacted summary"),
                "compacted summary must be in history, got: {full_text:.300}"
            );
            let texts = assistant_texts(&conversation);
            assert!(
                texts.iter().any(|t| t.contains("after compact")),
                "the rebuilt turn must run against the compacted conversation, got {texts:?}"
            );
            assert_eq!(server.messages_request_count(), 3);
        }));
    });
}

/// `pause_turn` resends the persisted assistant content: the sampler is
/// re-called WITHOUT any continue prompt injection.
#[test]
fn pause_turn_resend() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("first segment")], PAUSE_TURN),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("second segment")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;

            let result = run_user_turn(&actor, "pause-resend").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // Response persisted, sampler re-called (2 requests), and NO
            // truncation_continue item anywhere.
            let texts = assistant_texts(&conversation);
            assert_eq!(texts.len(), 2, "both segments must be persisted: {texts:?}");
            assert!(
                texts[0].contains("first segment") && texts[1].contains("second segment"),
                "segments must be persisted in order, got {texts:?}"
            );
            assert_eq!(
                truncation_continue_count(&conversation),
                0,
                "pause_turn resend must not inject a continue prompt"
            );
            assert_eq!(server.messages_request_count(), 2);
        }));
    });
}

/// A successfully continued turn emits NO `StopFailure` hook event.
#[test]
fn stop_failure_not_emitted_on_success() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part one ")], MAX_TOKENS),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part two")], END_TURN),
            );
            let (actor, mut gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            actor.hooks.client_hooks.borrow_mut().insert(
                ::hooks::event::HookEventName::StopFailure,
                vec![crate::extensions::hooks::ClientHookGroup {
                    matcher: None,
                    callback_ids: vec!["sf-obs".to_string()],
                    timeout: None,
                }],
            );

            let result = run_user_turn(&actor, "sf-success").await;
            result.expect("turn must complete successfully");

            let hook_events = drain_hook_event_names(&mut gateway_rx);
            assert!(
                !hook_events.iter().any(|e| e == "stop_failure"),
                "successful continue must not emit StopFailure, got {hook_events:?}"
            );
        }));
    });
}

/// An unrecoverable compaction failure after `model_context_window_exceeded`
/// still emits `StopFailure` (the existing hook contract for failed turns).
#[test]
fn stop_failure_emitted_on_unrecoverable() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("part one")], CONTEXT_WINDOW_EXCEEDED),
            );
            // The compaction request fails hard (HTTP 500).
            server.enqueue_response(
                "/v1/messages",
                ScriptedResponse::json(
                    500,
                    json!({ "error": { "type": "server_error", "message": "boom" } }),
                ),
            );
            let (actor, mut gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            actor.hooks.client_hooks.borrow_mut().insert(
                ::hooks::event::HookEventName::StopFailure,
                vec![crate::extensions::hooks::ClientHookGroup {
                    matcher: None,
                    callback_ids: vec!["sf-obs".to_string()],
                    timeout: None,
                }],
            );

            let result = run_user_turn(&actor, "sf-unrecoverable").await;
            assert!(result.is_err(), "compaction failure must fail the turn");

            let hook_events = drain_hook_event_names(&mut gateway_rx);
            assert!(
                hook_events.iter().any(|e| e == "stop_failure"),
                "unrecoverable truncation recovery must emit StopFailure, got {hook_events:?}"
            );
        }));
    });
}

/// Cross-backend consistency (architecture §12.2): the Chat Completions
/// backend must produce the SAME continue behavior as Messages. Its wire
/// has no `model_context_window_exceeded`/`pause_turn` stop reasons (the
/// `FinishReason` enum is Stop/Length/ToolCalls/ContentFilter only), so the
/// comparable scenario is `finish_reason: "length"` → `StopReason::Length`
/// → the same truncation-continue branch in the unified turn loop. The
/// Messages side is pinned by `truncation_auto_continue_e2e`; this test
/// asserts the identical contract on the `/v1/chat/completions` wire:
/// partial persisted, exactly one continue injection, concatenated final
/// output, successful turn, two sampling cycles.
#[test]
fn cross_backend_consistency() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/chat/completions",
                chat_completions_turn("part one ", Some("length")),
            );
            server.enqueue_response(
                "/v1/chat/completions",
                chat_completions_turn("part two", Some("stop")),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::ChatCompletions).await;

            let result = run_user_turn(&actor, "cross-backend-cc").await;
            result.expect("turn must complete successfully");

            let conversation = actor.chat_state_handle.get_conversation().await;
            // (1) Partial response persisted into conversation history.
            let texts = assistant_texts(&conversation);
            assert!(
                texts.iter().any(|t| t.contains("part one")),
                "partial response must be persisted, got {texts:?}"
            );
            // (2) Continue prompt injected with the synthetic reason.
            assert_eq!(
                truncation_continue_count(&conversation),
                1,
                "exactly one truncation_continue item must be injected"
            );
            // (3) Final output is the concatenation of the partials.
            let joined: String = texts.concat();
            assert!(
                joined.contains("part one") && joined.contains("part two"),
                "final assistant output must contain both partials, got {joined:?}"
            );
            // (4) Two sampling cycles happened on the chat/completions path.
            assert_eq!(chat_completions_request_count(&server), 2);
        }));
    });
}
