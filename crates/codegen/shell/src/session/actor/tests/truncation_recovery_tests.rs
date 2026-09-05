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
/// `content_block_stop` is emitted. An unclosed block now fails the response
/// instead of being silently discarded and reported as a normal completion.
fn thinking_block(text: &str, signature: &str) -> serde_json::Value {
    json!({
        "type": "thinking",
        "thinking": text,
        "signature": signature,
    })
}

/// A `tool_use` block; `complete` controls whether the terminating
/// `content_block_stop` is emitted (incomplete tool_use fails the response).
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
/// assembling the wire), so the stream layer rejects the response — the
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
        // terminating `content_block_stop`; the marker itself is internal
        // and never enters the wire.
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
        actor.chat_state_handle.replace_sampling_route(cfg);
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
            crate::session::actor::tests::support::admit_test_human_input(&actor, prompt_id).await,
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

#[test]
fn rejected_native_continuation_falls_back_silently_and_keeps_session_usable() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[
                        thinking_block("visible first thought", "native_signature_secret"),
                        text_block("first answer"),
                    ],
                    END_TURN,
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                ScriptedResponse::json(
                    400,
                    json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "constructed provider continuation rejection"
                        }
                    }),
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("portable retry answer")], END_TURN),
            );

            let (actor, mut gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            run_user_turn(&actor, "continuation-first")
                .await
                .expect("first turn establishes native continuation");
            actor.state.lock().await.foreground = ForegroundState::Idle;
            run_user_turn(&actor, "continuation-fallback")
                .await
                .expect("portable retry must complete in the same Session");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 3);
            let rejected = requests[1].body.as_ref().unwrap().to_string();
            assert!(rejected.contains("native_signature_secret"));
            assert!(rejected.contains("visible first thought"));
            let portable_retry = requests[2].body.as_ref().unwrap().to_string();
            assert!(!portable_retry.contains("native_signature_secret"));
            assert!(!portable_retry.contains("visible first thought"));
            assert!(portable_retry.contains("first answer"));

            let durable =
                serde_json::to_string(&actor.chat_state_handle.get_conversation().await).unwrap();
            assert!(durable.contains("visible first thought"));
            assert!(!durable.contains("native_signature_secret"));
            assert!(durable.contains("portable retry answer"));
            assert!(!actor.chat_state_handle.is_closed());

            let mut retry_notices = 0;
            while let Ok(message) = gateway_rx.try_recv() {
                let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                    continue;
                };
                if args.request.method.as_ref() != "grow/session_notification" {
                    continue;
                }
                let notification: crate::extensions::notification::SessionNotification =
                    serde_json::from_str(args.request.params.get()).unwrap();
                if matches!(notification.update, GrowSessionUpdate::RetryState(_)) {
                    retry_notices += 1;
                }
            }
            assert_eq!(
                retry_notices, 0,
                "recoverable continuation fallback must stay invisible to the TUI"
            );
        }));
    });
}

#[test]
fn failed_portable_retry_notifies_once_without_poisoning_session() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[
                        thinking_block("visible thought", "native_signature_secret"),
                        text_block("first answer"),
                    ],
                    END_TURN,
                ),
            );
            for message in ["native rejected", "portable request rejected"] {
                server.enqueue_response(
                    "/v1/messages",
                    ScriptedResponse::json(
                        400,
                        json!({
                            "type": "error",
                            "error": {"type": "invalid_request_error", "message": message}
                        }),
                    ),
                );
            }
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("later turn succeeds")], END_TURN),
            );

            let (actor, mut gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            run_user_turn(&actor, "fallback-fails-first").await.unwrap();
            actor.state.lock().await.foreground = ForegroundState::Idle;
            run_user_turn(&actor, "fallback-fails-second")
                .await
                .expect_err("the failed portable retry ends only this turn");
            assert!(!actor.chat_state_handle.is_closed());
            actor.state.lock().await.foreground = ForegroundState::Idle;
            run_user_turn(&actor, "fallback-fails-third")
                .await
                .expect("a later turn must remain usable");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 4);
            assert!(
                requests[1]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("native_signature_secret")
            );
            assert!(
                !requests[2]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("native_signature_secret")
            );

            let mut retry_notices = 0;
            while let Ok(message) = gateway_rx.try_recv() {
                let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                    continue;
                };
                if args.request.method.as_ref() != "grow/session_notification" {
                    continue;
                }
                let notification: crate::extensions::notification::SessionNotification =
                    serde_json::from_str(args.request.params.get()).unwrap();
                if matches!(notification.update, GrowSessionUpdate::RetryState(_)) {
                    retry_notices += 1;
                }
            }
            assert_eq!(retry_notices, 1);
        }));
    });
}

/// A successful transport can still yield unusable tool metadata. Preserve
/// that response and its repair, execute no siblings, and keep the next turn
/// usable without retrying the failed action automatically.
#[test]
fn malformed_tool_response_does_not_poison_the_next_turn() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(tokio::task::LocalSet::new().run_until(async {
            for duplicate_ids in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                server.enqueue_response("/v1/chat/completions", ScriptedResponse::sse(vec![
                    SseEvent::data(json!({
                        "id": "bad-response", "object": "chat.completion.chunk", "model": "test-model", "created": 1234567890,
                        "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [
                            {"index": 0, "id": "healthy", "type": "function",
                             "function": {"name": "todo_write", "arguments": "{}"}},
                            {"index": 1, "id": if duplicate_ids {"healthy"} else {""}, "type": "function",
                             "function": {"name": if duplicate_ids {"todo_write"} else {""}, "arguments": "{\"limit\":50}"}}
                        ]}, "finish_reason": "tool_calls"}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
                    }).to_string()),
                    SseEvent::data("[DONE]"),
                ]));
                server.enqueue_response("/v1/chat/completions", ScriptedResponse::sse(
                    test_support::sse::chat_completion_script_exact("continued safely", "test-model"),
                ));
                let (actor, _gateway_rx) = actor_with_sampler(&server, sampling_types::ApiBackend::ChatCompletions).await;
                let result = run_user_turn(&actor, "polluted").await;
                assert!(result.is_err(), "malformed response must stop before dispatch");
                let error = result.err().unwrap();
                assert!(format!("{error:?}").contains("automatically repaired"), "unexpected error: {error:?}");
                assert_eq!(chat_completions_request_count(&server), 1, "no hidden retry");
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(!events.iter().any(|event| matches!(event.kind, chat_state::TimelineEventKind::Tool(_))),
                    "even the healthy sibling must not enter tool execution");
                let original = events.iter().position(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Messages(message)
                        if message.cause == chat_state::MessageCause::Assistant
                        && message.items.iter().any(|item| matches!(item, ConversationItem::Assistant(a) if a.tool_calls.len() == 2))
                )).expect("raw provider response is retained");
                let repair = events.iter().position(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Messages(message) if message.cause == chat_state::MessageCause::IntegrityRepair
                )).expect("repair is retained");
                assert!(original < repair);
                let timeline = chat_state::Timeline::from_events(events).unwrap();
                assert!(timeline.completed_compaction_unloaded_branch_ids().is_empty());
                assert_eq!(actor.chat_state_handle.try_get_session_usage().await.unwrap().totals.model_calls, 1);

                actor.state.lock().await.foreground = ForegroundState::Idle;
                run_user_turn(&actor, "continue-after-repair").await.expect("same session must remain usable");
                assert_eq!(chat_completions_request_count(&server), 2);
                let requests = server.requests();
                let last = requests.iter().filter(|request| request.path == "/v1/chat/completions").last().unwrap();
                let body = last.body.as_ref().unwrap();
                let messages = body["messages"].as_array().unwrap();
                assert!(messages.iter().all(|message| message.get("tool_calls").is_none() && message.get("tool_call_id").is_none()));
                assert!(body.to_string().contains("untrusted historical evidence"));
                assert!(assistant_texts(&actor.chat_state_handle.get_conversation().await).iter().any(|text| text.contains("continued safely")));
            }
        }));
    });
}

/// Invalid executable output fails locally without poisoning later requests.
#[test]
fn protocol_invalid_tools_do_not_execute_and_the_next_turn_recovers() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(tokio::task::LocalSet::new().run_until(async {
            use sampling_types::ApiBackend;
            for (backend, unfinished_json) in [
                (ApiBackend::ChatCompletions, false),
                (ApiBackend::ChatCompletions, true),
                (ApiBackend::Responses, false),
                (ApiBackend::Responses, true),
            ] {
                let server = MockInferenceServer::start().await.unwrap();
                let (path, bad, good) = if backend == ApiBackend::ChatCompletions {
                    ("/v1/chat/completions", json!({
                        "id":"bad", "object":"chat.completion.chunk", "model":"test-model", "created":0,
                        "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_bad","type":"function",
                            "function":{"name":"todo_write","arguments":if unfinished_json {"{\"x\":"} else {"{}"}}}]},
                            "finish_reason":if unfinished_json {json!("length")} else {serde_json::Value::Null}}],
                        "usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}
                    }), test_support::sse::chat_completion_script_exact("continued safely", "test-model"))
                } else {
                    ("/v1/responses", json!({
                        "type":"response.incomplete","sequence_number":1,"response":{
                            "id":"bad","object":"response","created_at":0,"model":"test-model","status":"incomplete",
                            "incomplete_details":{"reason":"max_output_tokens"},
                            "output":[{"type":"function_call","id":"fc_bad","call_id":"call_bad","name":"todo_write",
                                "arguments":if unfinished_json {"{\"x\":"} else {"{}"},"status":"incomplete"}],
                            "usage":{"input_tokens":10,"output_tokens":20,"total_tokens":30,
                                "input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}
                        }
                    }), test_support::sse::responses_api_script_exact("continued safely", "test-model"))
                };
                server.enqueue_response(path, ScriptedResponse::sse(vec![SseEvent::data(bad.to_string())]));
                server.enqueue_response(path, ScriptedResponse::sse(good));
                let has_terminal_usage = backend == ApiBackend::Responses || unfinished_json;
                let (actor, _gateway_rx) = actor_with_sampler(&server, backend).await;
                let error = run_user_turn(&actor, "bad-tool").await.expect_err("partial tool must fail");
                assert!(format!("{error:?}").contains("protocol:"), "{error:?}");
                assert_eq!(server.requests().iter().filter(|request| request.path == path).count(), 1, "no hidden retry");
                assert_eq!(actor.chat_state_handle.try_get_session_usage().await.unwrap().totals.model_calls, u64::from(has_terminal_usage),
                    "settle known terminal usage, but do not promote a pre-terminal snapshot to a final total");
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(!events.iter().any(|event| matches!(event.kind, chat_state::TimelineEventKind::Tool(_))));
                actor.state.lock().await.foreground = ForegroundState::Idle;
                run_user_turn(&actor, "continue").await.expect("same session remains usable");
                assert_eq!(server.requests().iter().filter(|request| request.path == path).count(), 2);
                assert!(assistant_texts(&actor.chat_state_handle.get_conversation().await).iter().any(|text| text.contains("continued safely")));
            }
        }));
    });
}

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

/// Every thinking block closed before a max-token boundary remains visible in
/// durable history, while provider signatures stay memory-only. The
/// truncation then continues. Unclosed blocks are rejected by the sampler's
/// protocol tests and never reach this session path.
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
            // All complete visible thinking is durable. Provider signatures
            // remain in the in-memory continuation lane.
            let reasoning_texts: Vec<String> = conversation
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::Reasoning(r) => Some(r.text.to_string()),
                    _ => None,
                })
                .collect();
            assert!(
                reasoning_texts
                    .iter()
                    .any(|r| r.contains("complete thinking")),
                "complete visible thinking must be retained, got {reasoning_texts:?}"
            );
            assert!(
                reasoning_texts
                    .iter()
                    .any(|r| r.contains("partial thinking")),
                "the first closed thinking block must also be retained, got {reasoning_texts:?}"
            );
            assert!(
                !serde_json::to_string(&conversation)
                    .unwrap()
                    .contains("sig_complete")
            );
            assert_eq!(truncation_continue_count(&conversation), 1);
            let joined: String = assistant_texts(&conversation).concat();
            assert!(joined.contains("part one") && joined.contains("part two"));
        }));
    });
}

/// Tool-use truncation: an in-progress tool_use block (start + partial JSON,
/// no `content_block_stop`) fails the sample. No tools or partial response
/// enter history and no automatic truncation continuation is authorized.
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
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;

            let result = run_user_turn(&actor, "trunc-tool").await;
            assert!(
                format!("{:?}", result.expect_err("unclosed tool must fail")).contains("protocol:")
            );

            let conversation = actor.chat_state_handle.get_conversation().await;
            let assistant_items: Vec<&sampling_types::AssistantItem> = conversation
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::Assistant(a) => Some(a),
                    _ => None,
                })
                .collect();
            assert!(
                assistant_items
                    .iter()
                    .all(|a| !a.content.contains("part one"))
            );
            assert!(
                assistant_items.iter().all(|a| a.tool_calls.is_empty()),
                "the incomplete tool_use must not surface as a tool call"
            );
            assert_eq!(truncation_continue_count(&conversation), 0);
            assert_eq!(
                server.requests().len(),
                1,
                "no automatic retry/continuation"
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

/// Providers also report input overflow as an HTTP error instead of a normal
/// stop reason. The transport must not retry the unchanged payload, and the
/// Session must compact even when its local projection is below the configured
/// window and the error response carries no model-metadata header.
#[test]
fn api_context_window_error_triggers_session_compaction() {
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
                ScriptedResponse::json(
                    400,
                    json!({
                        "error": {
                            "type": "invalid_request_error",
                            "message": "context window exceeded"
                        }
                    }),
                ),
            );
            let long_summary = format!(
                "compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block(&long_summary)], END_TURN),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("after API-error compact")], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            seed_closed_compaction_range(&actor).await;

            run_user_turn(&actor, "api-ctx-exceeded")
                .await
                .expect("explicit provider overflow must compact and resume");

            let full_text = actor
                .chat_state_handle
                .get_conversation()
                .await
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(full_text.contains("compacted summary"));
            assert!(full_text.contains("after API-error compact"));
            assert_eq!(
                server.messages_request_count(),
                3,
                "the unchanged oversized request must not be retried by the transport"
            );
        }));
    });
}

#[test]
fn repeated_api_context_window_error_after_compaction_is_terminal() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let overflow = || {
                ScriptedResponse::json(
                    400,
                    json!({
                        "error": {
                            "type": "invalid_request_error",
                            "message": "context window exceeded"
                        }
                    }),
                )
            };
            server.enqueue_response("/v1/messages", overflow());
            let long_summary = format!(
                "compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block(&long_summary)], END_TURN),
            );
            server.enqueue_response("/v1/messages", overflow());
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            seed_closed_compaction_range(&actor).await;

            let error = run_user_turn(&actor, "api-ctx-still-exceeded")
                .await
                .expect_err("an immediate post-compaction overflow must stop");
            let detail = error.data.as_ref().map(ToString::to_string).unwrap_or_default();
            assert!(detail.contains("stopped to avoid an unbounded retry loop"));
            assert_eq!(
                server.messages_request_count(),
                3,
                "only one compaction and one rebuilt provider attempt are allowed per recovery cycle"
            );
        }));
    });
}

#[test]
fn successful_tool_step_opens_a_new_context_recovery_attempt() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let overflow = || {
                ScriptedResponse::json(
                    400,
                    json!({
                        "error": {
                            "type": "invalid_request_error",
                            "message": "context window exceeded"
                        }
                    }),
                )
            };
            server.enqueue_response("/v1/messages", overflow());
            let first_summary = format!(
                "first compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block(&first_summary)], END_TURN),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[tool_use_block(
                        "call_between_compactions",
                        "todo_write",
                        r#"{"todos":[{"id":"t1","content":"continue","status":"completed"}]}"#,
                    )],
                    END_TURN,
                ),
            );
            server.enqueue_response("/v1/messages", overflow());
            let (actor, _gateway_rx) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            seed_closed_compaction_range(&actor).await;

            let error = run_user_turn(&actor, "two-context-recovery-cycles")
                .await
                .expect_err("the active turn has no second closed range to summarize");
            let detail = error
                .data
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            assert!(
                detail.contains("no closed Surface range"),
                "the successful tool Step must clear the immediate-overflow guard; got {detail}"
            );
            assert!(!detail.contains("unbounded retry loop"));
            assert_eq!(server.messages_request_count(), 4);
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
