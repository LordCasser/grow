//! Pre-prune integration tests (Task C): the `maybe_pre_prune` ladder inside
//! `run_compact_only`, the post-compact convergence check
//! (`CompactConvergedOverWindow`), the overflow-branch fail-safe, and the
//! display/logging layering guarantees.
//!
//! Scenarios (per the Task C spec):
//! 1. Oversized tool results + over threshold → pre-prune trims them under the
//!    threshold → the summary LLM call is skipped entirely.
//! 2. Pruned but still over threshold → the summary path runs (the pruned
//!    conversation is its input).
//! 3. `ModelContextWindowExceeded` + compaction still over the window → turn
//!    fails with a diagnostic (bounded sampling, no re-loop).
//! 5. `prune_tool_results` errors → fail-open: the summary path still runs.
//! Plus a layering test: pruning rewrites only the history snapshot
//! (`ReplaceHistory`), emits no chat-state UI events, and leaves the append
//! log (updates.jsonl analogue) carrying the original content.
//!
//! Scenario 4 (`context_window_exceeded_triggers_compaction`, compaction
//! success → continue resample) is the existing regression test in
//! `truncation_recovery_tests.rs`.

use super::support::*;
use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};
use tokio::sync::mpsc;

/// Wire `stop_reason` values for the Messages API (see
/// `sampler/src/stream/messages.rs` mapping).
const END_TURN: &str = "end_turn";
const CONTEXT_WINDOW_EXCEEDED: &str = "model_context_window_exceeded";

// ─── Messages-API SSE builder ─────────────────────────────────────────────

/// Assemble a Messages-API SSE turn from `(text, stop_reason)` blocks and a
/// terminal `stop_reason`. `input_tokens` seeds the reported usage so tests
/// can pin the chat-state `total_tokens` the turn loop records.
fn messages_turn_with_usage(
    blocks: &[(&str, &str)],
    stop_reason: &str,
    input_tokens: u64,
) -> ScriptedResponse {
    let mut events: Vec<String> = vec![
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_test", "type": "message", "role": "assistant",
                "content": [], "model": "test-model", "stop_reason": null,
                "usage": {
                    "input_tokens": input_tokens, "output_tokens": 0,
                    "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
                }
            }
        })
        .to_string(),
    ];
    for (i, (text, block_stop)) in blocks.iter().enumerate() {
        events.push(
            json!({ "type": "content_block_start", "index": i, "content_block": { "type": "text", "text": "" } })
                .to_string(),
        );
        events.push(
            json!({
                "type": "content_block_delta",
                "index": i,
                "delta": { "type": "text_delta", "text": text }
            })
            .to_string(),
        );
        if !block_stop.is_empty() {
            events.push(json!({ "type": "content_block_stop", "index": i }).to_string());
        }
    }
    events.push(
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason },
            "usage": { "output_tokens": 5, "input_tokens": input_tokens }
        })
        .to_string(),
    );
    events.push(json!({ "type": "message_stop" }).to_string());
    ScriptedResponse::sse(events.into_iter().map(SseEvent::data).collect())
}

/// Default-usage variant (input_tokens = 10).
fn messages_turn(blocks: &[(&str, &str)], stop_reason: &str) -> ScriptedResponse {
    messages_turn_with_usage(blocks, stop_reason, 10)
}

// ─── Actor fixture ─────────────────────────────────────────────────────────

/// Run a test body on a dedicated thread with an 8MB stack (the turn loop's
/// async state-machine chain needs 2–4MB; see `truncation_recovery_tests.rs`).
fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

/// Build a `SessionActor` whose sampler is a real `SamplerActor` pointed at
/// `server`; `context_window` applies to BOTH the chat-state sampling config
/// and the sampler config so pre-prune/compaction math uses one window.
async fn actor_with_sampler_cw(
    server: &MockInferenceServer,
    api_backend: sampling_types::ApiBackend,
    context_window: u64,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    actor_with_sampler_cw_ex(server, api_backend, context_window, None).await
}

/// [`actor_with_sampler_cw`] variant for fork-scenario tests: sets
/// `startup_hints.inherited_prefix_len` before the actor is shared behind the
/// `Arc` (the field has no interior mutability).
async fn actor_with_sampler_cw_ex(
    server: &MockInferenceServer,
    api_backend: sampling_types::ApiBackend,
    context_window: u64,
    inherited_prefix_len: Option<usize>,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    let mut actor = create_test_actor(0, context_window, 85, gateway_tx, persistence_tx).await;
    if let Some(prefix_len) = inherited_prefix_len {
        actor.startup_hints.inherited_prefix_len = Some(prefix_len);
    }
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
        context_window,
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

/// Drive one user turn through the real loop, bounded so a broken continue
/// loop fails the test instead of hanging it.
async fn run_user_turn(
    actor: &std::sync::Arc<SessionActor>,
    prompt_id: &str,
) -> Result<PromptTurnOk, acp::Error> {
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        actor.handle_prompt(
            prompt_id,
            crate::session::PromptOrigin::User,
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
            None,
        ),
    )
    .await
    .expect("turn must finish within the timeout (no runaway continue loop)")
}

/// Oversized tool-result payload (200KB → 50K tokens under bytes/4).
fn big_tool_text() -> String {
    "x".repeat(200_000)
}

/// Push `count` well-formed (assistant tool_use → tool result) rounds into the
/// conversation, then barrier on a round-trip so all pushes are processed.
async fn seed_tool_result_rounds(actor: &SessionActor, count: usize) {
    actor
        .chat_state_handle
        .replace_system_head("test system prompt")
        .await
        .expect("system head must be replaceable");
    for i in 0..count {
        actor
            .chat_state_handle
            .push_user_message(ConversationItem::user(format!("u{i}")));
        actor
            .chat_state_handle
            .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
                sampling_types::ToolCall {
                    id: format!("call-{i}").into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                },
            ]));
        actor
            .chat_state_handle
            .push_tool_result(ConversationItem::tool_result(
                format!("call-{i}"),
                big_tool_text(),
            ));
    }
    let _ = actor.chat_state_handle.get_conversation_len().await;
}

/// Tool-result text contents in conversation order.
fn tool_result_texts(conversation: &[ConversationItem]) -> Vec<String> {
    conversation
        .iter()
        .filter_map(|item| match item {
            ConversationItem::ToolResult(tr) => Some(tr.content.to_string()),
            _ => None,
        })
        .collect()
}

/// Session-update kinds observed over the gateway (`grow/session_notification`
/// extension notifications' `update.sessionUpdate` tags), plus the raw params
/// of every `auto_compact_completed` notification.
fn drain_session_updates(
    rx: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut kinds = Vec::new();
    let mut completed = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
            && args.request.method.as_ref() == "grow/session_notification"
            && let Ok(params) = serde_json::from_str::<serde_json::Value>(args.request.params.get())
            && let Some(kind) = params
                .get("update")
                .and_then(|u| u.get("sessionUpdate"))
                .and_then(|v| v.as_str())
        {
            kinds.push(kind.to_string());
            if kind == "auto_compact_completed" {
                completed.push(params);
            }
        }
    }
    (kinds, completed)
}

// ─── Diagnostics capture ───────────────────────────────────────────────────

/// Captured `tracing` event fields (target + recorded field values as strings).
#[derive(Debug)]
struct CapturedTraceEvent {
    target: &'static str,
    fields: std::collections::HashMap<String, String>,
}

/// Install a thread-local capture layer for `tracing` events; drop the guard
/// to release it.
fn capture_trace_events() -> (
    mpsc::UnboundedReceiver<CapturedTraceEvent>,
    tracing::subscriber::DefaultGuard,
) {
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;

    #[derive(Default)]
    struct Visitor {
        fields: std::collections::HashMap<String, String>,
    }
    impl tracing::field::Visit for Visitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    struct CaptureLayer {
        tx: mpsc::UnboundedSender<CapturedTraceEvent>,
    }
    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut visitor = Visitor::default();
            event.record(&mut visitor);
            let _ = self.tx.send(CapturedTraceEvent {
                target: event.metadata().target(),
                fields: visitor.fields,
            });
        }
    }

    let (tx, rx) = mpsc::unbounded_channel();
    let subscriber = tracing_subscriber::registry().with(CaptureLayer { tx });
    let guard = tracing::subscriber::set_default(subscriber);
    // The callsite interest cache is process-global: a callsite first
    // registered by a subscriber-less thread (other parallel tests) is cached
    // as `never`, and `set_default` does not invalidate it. Rebuild so this
    // thread's layer actually receives every event.
    tracing::callsite::rebuild_interest_cache();
    (rx, guard)
}

/// Diagnostics events (target `diagnostics`) as `(name, payload)` pairs.
/// Consumes `events` (a vec drained from the capture channel).
fn diagnostic_events(events: &[CapturedTraceEvent]) -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    for event in events {
        if event.target != "diagnostics" {
            continue;
        }
        let Some(name) = event.fields.get("diagnostic_event").cloned() else {
            continue;
        };
        let payload = event
            .fields
            .get("payload")
            .and_then(|p| serde_json::from_str(p).ok())
            .unwrap_or(serde_json::Value::Null);
        out.push((name, payload));
    }
    out
}

// ─── Scenario 1 ────────────────────────────────────────────────────────────

/// Oversized tool results push the estimate over the threshold; pre-prune
/// trims them back under it; the summary LLM call is skipped (exactly one
/// sampling request: the main turn), `AutoCompactPruned` fires, and the UI
/// sees Started→Completed without a Failed notification.
#[test]
fn pre_prune_resolves_pressure_and_skips_summary() {
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
                messages_turn(&[("hello", END_TURN)], END_TURN),
            );
            let (actor, mut gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000)
                    .await;
            let (mut trace_rx, _guard) = capture_trace_events();
            seed_tool_result_rounds(&actor, 2).await;
            // Model truth at the last response: 86K, just over the 85K trigger.
            actor.chat_state_handle.record_token_usage(86_000);
            // Current turn's pushes (small since_model delta).
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("current turn"));
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("partial prior"));
            let _ = actor.chat_state_handle.get_conversation_len().await;

            let (mut trace_rx, _guard) = capture_trace_events();
            let result = run_user_turn(&actor, "pre-prune-1").await;
            result.expect("turn must complete successfully");

            let mut raw_events = Vec::new();
            while let Ok(e) = trace_rx.try_recv() {
                raw_events.push(e);
            }

            // No summary LLM call: only the main turn sample hit the server.
            assert_eq!(
                server.messages_request_count(),
                1,
                "pre-prune success must skip the summary LLM call"
            );

            // The oldest oversized tool result was pruned in place; the newer
            // one is untouched (the plan stops as soon as the target is met).
            let conversation = actor.chat_state_handle.get_conversation().await;
            let tool_texts = tool_result_texts(&conversation);
            assert_eq!(tool_texts.len(), 2);
            let (pruned, kept) = (&tool_texts[0], &tool_texts[1]);
            assert_ne!(pruned, &big_tool_text(), "oldest tool result must be pruned");
            // Budget: 5% of 100K = 5000 tokens → 20000 bytes, never exceeded.
            assert!(pruned.len() <= 20_000, "pruned content must fit the budget");
            assert!(
                pruned.starts_with(&"x".repeat(100)),
                "head prefix must be kept"
            );
            assert!(
                pruned.ends_with(&"x".repeat(100)),
                "tail suffix must be kept"
            );
            assert_eq!(kept, &big_tool_text(), "newer tool result must be untouched");

            // Diagnostics: AutoCompactPruned with the expected fields.
            let events = diagnostic_events(&raw_events);
            let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
            let pruned_event = events.iter().find(|(name, _)| name == "auto_compact_pruned");
            let Some(pruned_event) = pruned_event else {
                panic!(
                    "auto_compact_pruned diagnostics event must fire; captured {names:?}, raw={raw_events:?}"
                );
            };
            let payload = &pruned_event.1;
            assert_eq!(payload["pruned_count"], 1);
            assert_eq!(payload["threshold_percent"], 85);
            assert_eq!(payload["budget_tokens"], 5_000);
            assert_eq!(payload["source"], "pre_sampling");
            assert!(
                payload["tokens_before"].as_u64().unwrap() > payload["tokens_after"].as_u64().unwrap(),
                "pruning must reduce the estimated total"
            );

            // Display/layering: Started→Completed without Failed; the
            // completed payload carries the short pruning description.
            let (kinds, completed) = drain_session_updates(&mut gateway_rx);
            assert!(
                kinds.iter().any(|k| k == "auto_compact_completed"),
                "completed notification must be sent, got {kinds:?}"
            );
            assert!(
                !kinds.iter().any(|k| k == "auto_compact_failed"),
                "no failed notification on the prune path, got {kinds:?}"
            );
            assert_eq!(completed.len(), 1, "exactly one completed notification");
            let preview = completed[0]["update"]["summary_preview"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(
                preview.contains("pruned 1 tool result"),
                "summary_preview must describe the prune, got {preview:?}"
            );
        }));
    });
}

// ─── Scenario 2 ────────────────────────────────────────────────────────────

/// Pruning happens but the estimate stays over the threshold (a large
/// `since_model` delta — fresh tool results since the last model response):
/// the strict gate rejects the skip and the summary path still runs.
///
/// Driven at the `maybe_pre_prune` level because the turn loop's
/// `ensure_prefix_ready` re-bases `total_tokens` to the static estimate and
/// zeroes `since_model` on the first turn, so an e2e turn cannot produce the
/// still-over-threshold state at the pre-sampling trigger.
#[test]
fn pre_prune_insufficient_falls_back_to_summary() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let long_summary = format!(
                "compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[(&long_summary, END_TURN)], END_TURN),
            );
            let (actor, mut gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            seed_tool_result_rounds(&actor, 1).await;
            actor.chat_state_handle.record_token_usage(86_000);
            // A fresh oversized tool result this turn keeps `since_model`
            // high, so the post-prune estimate cannot drop under the threshold.
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("current turn"));
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "call-last".into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                }]),
            );
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result("call-last", big_tool_text()));
            let _ = actor.chat_state_handle.get_conversation_len().await;

            let trigger = compaction::AutoCompactTriggerInfo {
                tokens_used: 136_000,
                context_window: 100_000,
                percentage: 100,
                source: "test",
            };
            let pruned = actor
                .maybe_pre_prune(&trigger)
                .await
                .expect("maybe_pre_prune must not error");
            assert!(
                !pruned,
                "gate must reject the summary skip when the estimate is still over the threshold"
            );
            // The prune itself still ran and trimmed the oldest tool result.
            let conversation = actor.chat_state_handle.get_conversation().await;
            let tool_texts = tool_result_texts(&conversation);
            assert_eq!(tool_texts.len(), 2);
            assert_ne!(
                &tool_texts[0],
                &big_tool_text(),
                "prune must have trimmed it"
            );
            assert_eq!(
                &tool_texts[1],
                &big_tool_text(),
                "the fresh tool result must be untouched"
            );

            // The caller then continues to the summary path (pre-prune off so
            // this phase isolates the summary): the summary LLM call happens.
            actor.compaction.pre_prune.set(false);
            actor
                .run_compact_only(trigger)
                .await
                .expect("summary path must run after the gate rejection");
            assert_eq!(
                server.messages_request_count(),
                1,
                "the summary LLM call must happen"
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            let full_text: String = conversation
                .iter()
                .map(|item| item.text_content())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                full_text.contains("compacted summary"),
                "summary must land in history"
            );
            let (kinds, _) = drain_session_updates(&mut gateway_rx);
            assert!(
                kinds.iter().any(|k| k == "auto_compact_completed"),
                "completed notification must be sent, got {kinds:?}"
            );
        }));
    });
}

// ─── Scenario 3 ────────────────────────────────────────────────────────────

/// `ModelContextWindowExceeded` + a compaction whose reseed still exceeds the
/// context window → the turn fails with a diagnostic message instead of
/// resampling forever. Sampling is bounded (exactly two requests).
#[test]
fn context_window_exceeded_converged_over_window_fails_turn() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            // The model reports 120K input tokens on a 100K window: the
            // overflow branch fires, and the reseed ratio keeps the compacted
            // history's estimated total pinned at the reported count.
            server.enqueue_response(
                "/v1/messages",
                messages_turn_with_usage(
                    &[("partial", CONTEXT_WINDOW_EXCEEDED)],
                    CONTEXT_WINDOW_EXCEEDED,
                    120_000,
                ),
            );
            let long_summary = format!(
                "compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[(&long_summary, END_TURN)], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor
                .chat_state_handle
                .replace_system_head("test system prompt")
                .await
                .expect("system head must be replaceable");

            let result = run_user_turn(&actor, "ctx-converged").await;
            let err = result.expect_err("turn must fail after converged-over-window");
            let message = err
                .data
                .as_ref()
                .and_then(|d| d.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            assert!(
                message.contains("still exceeds"),
                "diagnostic message must explain the overflow, got {message:?}"
            );

            // Bounded sampling: the overflow sample + one summary call, no loop.
            assert_eq!(
                server.messages_request_count(),
                2,
                "sampling must be bounded (no continue loop)"
            );
            // The convergence failure sticky-suppresses AUTO (no re-loop).
            assert_eq!(
                actor
                    .compaction
                    .auto_compact_suppressed
                    .load(std::sync::atomic::Ordering::Relaxed),
                crate::session::compaction_config::SUPPRESS_STICKY,
                "convergence failure must sticky-suppress AUTO"
            );
        }));
    });
}

// ─── Scenario 5 ────────────────────────────────────────────────────────────

/// `prune_tool_results` returning `Err` must fail open: `maybe_pre_prune`
/// returns `false` without touching the suppress state, and the summary path
/// still runs afterwards.
#[test]
fn pre_prune_error_fails_open_to_summary() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let long_summary = format!(
                "compacted summary: {}",
                "filler sentence that keeps the summary above the minimum seed length. ".repeat(20)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[(&long_summary, END_TURN)], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 40_000).await;
            // One oversized tool result: plan selects it (50K static > 34K target).
            seed_tool_result_rounds(&actor, 1).await;

            let trigger = compaction::AutoCompactTriggerInfo {
                tokens_used: 50_000,
                context_window: 40_000,
                percentage: 125,
                source: "test",
            };
            let trigger_phase_a = compaction::AutoCompactTriggerInfo {
                tokens_used: trigger.tokens_used,
                context_window: trigger.context_window,
                percentage: trigger.percentage,
                source: trigger.source,
            };

            // Phase A: force `PruneToolResults` to fail deterministically.
            // Poll-driven handshake on the chat-state command queue: the
            // spawned task's first poll runs synchronously through the
            // ladder's sync checks and queues `GetConversation` before it
            // parks on the reply — and it fires `started` in that same poll,
            // so when the main task wakes, `GetConversation` is already
            // queued. Main then queues an empty `ReplaceConversation`
            // *synchronously* (no await in between), so the FIFO order is
            // `GetConversation → ReplaceConversation → PruneToolResults`:
            // the plan is built from the non-empty snapshot, and the prune
            // hits `PruneError::EmptyConversation`.
            let started = std::sync::Arc::new(tokio::sync::Notify::new());
            let (mut trace_rx, _guard) = capture_trace_events();
            let plan_task = {
                let actor = actor.clone();
                let started = started.clone();
                tokio::task::spawn_local(async move {
                    started.notify_waiters();
                    actor.maybe_pre_prune(&trigger_phase_a).await
                })
            };
            started.notified().await;
            actor.chat_state_handle.replace_conversation(vec![]);

            let pruned = plan_task
                .await
                .expect("maybe_pre_prune must not propagate the prune error")
                .expect("maybe_pre_prune must not error");
            assert!(!pruned, "prune error must fail open (no summary skip)");
            // Pin the Err arm: the fail-open warn must have been logged (if
            // the interleave ever degraded to the empty-plan rung, this fails
            // loudly instead of silently weakening the test).
            let mut raw_events = Vec::new();
            while let Ok(e) = trace_rx.try_recv() {
                raw_events.push(e);
            }
            assert!(
                raw_events.iter().any(|e| {
                    e.target == "shell::session::acp_session::compaction"
                        && e.fields
                            .get("message")
                            .is_some_and(|m| m.contains("pre-prune failed"))
                }),
                "the prune-error fail-open warn must fire"
            );
            // Fail-open must not change the suppress state.
            assert_eq!(
                actor
                    .compaction
                    .auto_compact_suppressed
                    .load(std::sync::atomic::Ordering::Relaxed),
                crate::session::compaction_config::SUPPRESS_NONE,
                "pre-prune failure must not suppress auto-compaction"
            );

            // Phase B: the fallback path still runs the summary. Re-seed the
            // conversation, switch pre-prune off (so the ladder short-circuits
            // and the summary is the only path), and run a real compaction.
            actor
                .chat_state_handle
                .replace_conversation_for_compaction(vec![
                    ConversationItem::system("test system prompt"),
                    ConversationItem::user("u0"),
                    ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                        id: "call-0".into(),
                        name: "grep".to_string(),
                        arguments: "{}".into(),
                    }]),
                    ConversationItem::tool_result("call-0", big_tool_text()),
                ]);
            actor.chat_state_handle.record_token_usage(40_000);
            actor.compaction.pre_prune.set(false);
            let _ = actor.chat_state_handle.get_conversation_len().await;

            actor
                .run_compact_only(trigger)
                .await
                .expect("summary path must succeed after the prune failure");

            assert_eq!(
                server.messages_request_count(),
                1,
                "the summary LLM call must still happen"
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            let full_text: String = conversation
                .iter()
                .map(|item| item.text_content())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                full_text.contains("compacted summary"),
                "summary must land in history"
            );
        }));
    });
}

// ─── Review-fix: suppress gating and fork prefix protection ───────────────

/// Review fix A: sticky suppression (deterministic size failure) must not
/// block pre-prune — pruning is the model-free remedy — and a prune whose
/// strict gate passes clears the sticky bit (a context-budget change is the
/// existing STICKY clear condition). A gate that does NOT pass leaves the
/// suppress state untouched.
#[test]
fn pre_prune_under_sticky_suppress_clears_it_on_success() {
    use crate::session::compaction_config::{SUPPRESS_NONE, SUPPRESS_STICKY};
    use std::sync::atomic::Ordering;
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            let trigger = compaction::AutoCompactTriggerInfo {
                tokens_used: 105_000,
                context_window: 100_000,
                percentage: 105,
                source: "test",
            };
            actor
                .compaction
                .auto_compact_suppressed
                .store(SUPPRESS_STICKY, Ordering::Relaxed);

            // Phase 0: one oversized tool result keeps the conversation under
            // the plan target (≈50K <= 85K), so the plan is empty → `false`,
            // and the failed gate must NOT clear the sticky bit.
            seed_tool_result_rounds(&actor, 1).await;
            actor.chat_state_handle.record_token_usage(86_000);
            let _ = actor.chat_state_handle.get_conversation_len().await;
            let pruned = actor
                .maybe_pre_prune(&trigger)
                .await
                .expect("maybe_pre_prune must not error");
            assert!(!pruned, "an empty plan must return false even under STICKY");
            assert_eq!(
                actor
                    .compaction
                    .auto_compact_suppressed
                    .load(Ordering::Relaxed),
                SUPPRESS_STICKY,
                "a gate that does not pass must leave the suppress state unchanged"
            );

            // Phase 1: a second oversized round pushes the conversation over
            // the plan target. STICKY must now let the prune through, and the
            // passing gate clears the sticky bit to NONE.
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("u1"));
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "call-1".into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                }]),
            );
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result("call-1", big_tool_text()));
            // Re-record model truth so `since_model` is zeroed (otherwise the
            // conservative gate refuses the summary skip; see §2.1).
            actor.chat_state_handle.record_token_usage(105_000);
            let _ = actor.chat_state_handle.get_conversation_len().await;

            let (mut trace_rx, _guard) = capture_trace_events();
            let pruned = actor
                .maybe_pre_prune(&trigger)
                .await
                .expect("maybe_pre_prune must not error");
            assert!(pruned, "STICKY must not block a non-empty prune plan");
            assert_eq!(
                actor
                    .compaction
                    .auto_compact_suppressed
                    .load(Ordering::Relaxed),
                SUPPRESS_NONE,
                "a passing gate must clear the sticky bit"
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            let tool_texts = tool_result_texts(&conversation);
            assert_eq!(tool_texts.len(), 2);
            assert_ne!(
                &tool_texts[0],
                &big_tool_text(),
                "oldest tool result must be pruned"
            );
            assert_eq!(
                &tool_texts[1],
                &big_tool_text(),
                "newer tool result must be untouched"
            );

            let mut raw_events = Vec::new();
            while let Ok(e) = trace_rx.try_recv() {
                raw_events.push(e);
            }
            let events = diagnostic_events(&raw_events);
            let pruned_event = events
                .iter()
                .find(|(name, _)| name == "auto_compact_pruned")
                .expect("auto_compact_pruned diagnostics event must fire");
            assert_eq!(pruned_event.1["pruned_count"], 1);
        }));
    });
}

/// Review fix A: account-state suppression (provider quota / auth) and
/// per-turn suppression keep blocking pre-prune — no model-free remedy
/// applies to them — and the blocked gate must not touch the suppress state
/// or the conversation.
#[test]
fn pre_prune_blocked_by_account_and_turn_suppress() {
    use crate::session::compaction_config::{SUPPRESS_AUTH, SUPPRESS_TURN, SUPPRESS_UNTIL_SUCCESS};
    use std::sync::atomic::Ordering;
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            // Two oversized rounds: the plan would be non-empty (100K > 85K
            // target), so these assertions really exercise the gate.
            seed_tool_result_rounds(&actor, 2).await;
            actor.chat_state_handle.record_token_usage(105_000);
            let _ = actor.chat_state_handle.get_conversation_len().await;
            let trigger = compaction::AutoCompactTriggerInfo {
                tokens_used: 105_000,
                context_window: 100_000,
                percentage: 105,
                source: "test",
            };
            for suppress in [SUPPRESS_TURN, SUPPRESS_UNTIL_SUCCESS, SUPPRESS_AUTH] {
                actor
                    .compaction
                    .auto_compact_suppressed
                    .store(suppress, Ordering::Relaxed);
                let pruned = actor
                    .maybe_pre_prune(&trigger)
                    .await
                    .expect("maybe_pre_prune must not error");
                assert!(
                    !pruned,
                    "suppression class {suppress} must keep blocking pre-prune"
                );
                assert_eq!(
                    actor
                        .compaction
                        .auto_compact_suppressed
                        .load(Ordering::Relaxed),
                    suppress,
                    "a blocked gate must leave the suppress state unchanged"
                );
                let conversation = actor.chat_state_handle.get_conversation().await;
                let tool_texts = tool_result_texts(&conversation);
                assert_eq!(tool_texts.len(), 2);
                assert_eq!(
                    &tool_texts[0],
                    &big_tool_text(),
                    "no tool result may be pruned under suppression class {suppress}"
                );
                assert_eq!(
                    &tool_texts[1],
                    &big_tool_text(),
                    "no tool result may be pruned under suppression class {suppress}"
                );
            }
        }));
    });
}

/// Review fix B: while the fork's inherited parent transcript is still
/// pinned (`prefix_released == false`), pre-prune must never touch items
/// inside `conversation[..inherited_prefix_len]` — `preserve_inherited_prefix`
/// re-pins that region verbatim at compaction time. Only the child's own
/// oversized tool results may be pruned. `prefix_released` takes precedence
/// over `inherited_prefix_len`.
#[test]
fn pre_prune_never_touches_inherited_fork_prefix() {
    use std::sync::atomic::Ordering;
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            // The fork hint covers the four parent items.
            let (actor, _gateway_rx) = actor_with_sampler_cw_ex(
                &server,
                sampling_types::ApiBackend::Messages,
                100_000,
                Some(4),
            )
            .await;
            // 40% threshold → target 40K tokens; default budget 5% = 5K tokens
            // (20KB). The parent's 40KB tool result (≈10K tokens) is a prune
            // candidate by size but lives inside the inherited prefix.
            actor.compaction.threshold_percent.set(40);
            let parent_tool_text = "x".repeat(40_000);
            actor
                .chat_state_handle
                .replace_system_head("parent system prompt")
                .await
                .expect("system head must be replaceable");
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("parent u"));
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "call-parent".into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                }]),
            );
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result(
                    "call-parent",
                    parent_tool_text.clone(),
                ));
            // Child turns (the prunable suffix): one oversized 200KB result.
            actor
                .chat_state_handle
                .push_user_message(ConversationItem::user("child u"));
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "call-child".into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                }]),
            );
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result("call-child", big_tool_text()));
            actor.chat_state_handle.record_token_usage(60_000);
            let _ = actor.chat_state_handle.get_conversation_len().await;

            let trigger = compaction::AutoCompactTriggerInfo {
                tokens_used: 60_000,
                context_window: 100_000,
                percentage: 60,
                source: "test",
            };
            let (mut trace_rx, _guard) = capture_trace_events();
            let pruned = actor
                .maybe_pre_prune(&trigger)
                .await
                .expect("maybe_pre_prune must not error");
            assert!(
                pruned,
                "pruning the child's own oversized tool result must resolve the pressure"
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            let tool_texts = tool_result_texts(&conversation);
            assert_eq!(tool_texts.len(), 2);
            assert_eq!(
                &tool_texts[0], &parent_tool_text,
                "the inherited prefix's tool result must be preserved verbatim"
            );
            assert_ne!(
                &tool_texts[1],
                &big_tool_text(),
                "the child's oversized tool result must be pruned"
            );
            assert!(
                tool_texts[1].len() <= 20_000,
                "pruned content must fit the 5K-token budget (20KB)"
            );
            let mut raw_events = Vec::new();
            while let Ok(e) = trace_rx.try_recv() {
                raw_events.push(e);
            }
            let events = diagnostic_events(&raw_events);
            let pruned_event = events
                .iter()
                .find(|(name, _)| name == "auto_compact_pruned")
                .expect("auto_compact_pruned diagnostics event must fire");
            assert_eq!(
                pruned_event.1["pruned_count"], 1,
                "exactly the child's tool result must be pruned"
            );

            // Phase 2: `prefix_released` wins over `inherited_prefix_len` —
            // after release the whole conversation (parent item included) is
            // prunable again.
            actor
                .compaction
                .prefix_released
                .store(true, Ordering::Relaxed);
            actor
                .chat_state_handle
                .replace_conversation_for_compaction(vec![
                    ConversationItem::system("parent system prompt"),
                    ConversationItem::user("parent u"),
                    ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                        id: "call-parent".into(),
                        name: "grep".to_string(),
                        arguments: "{}".into(),
                    }]),
                    ConversationItem::tool_result("call-parent", parent_tool_text.clone()),
                    ConversationItem::user("child u"),
                    ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                        id: "call-child".into(),
                        name: "grep".to_string(),
                        arguments: "{}".into(),
                    }]),
                    ConversationItem::tool_result("call-child", big_tool_text()),
                ]);
            let _ = actor.chat_state_handle.get_conversation_len().await;
            let pruned = actor
                .maybe_pre_prune(&trigger)
                .await
                .expect("maybe_pre_prune must not error");
            assert!(pruned, "the released prefix must now be prunable");
            let conversation = actor.chat_state_handle.get_conversation().await;
            let tool_texts = tool_result_texts(&conversation);
            assert_ne!(
                &tool_texts[0], &parent_tool_text,
                "after release the former prefix item must be pruned too"
            );
            assert_ne!(
                &tool_texts[1],
                &big_tool_text(),
                "the child's tool result must be pruned too"
            );
        }));
    });
}

// ─── Display / logging layering ────────────────────────────────────────────

/// Pruning appends a content-only Timeline replacement and refreshes the
/// display cache without emitting a pager event. The original tool result
/// remains shadowed in the immutable ledger.
#[test]
fn prune_rewrites_history_snapshot_without_updates_or_ui_events() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    rt.block_on(local.run_until(async {
        let (persistence, mut recv) = chat_state::MockChatPersistence::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<chat_state::ChatStateEvent>();
        let conversation = vec![
            ConversationItem::system("test system prompt"),
            ConversationItem::user("u0"),
            ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                id: "call-0".into(),
                name: "grep".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("call-0", big_tool_text()),
        ];
        let handle = chat_state::ChatStateActor::spawn(
            conversation.clone(),
            sampling_types::SamplingConfig {
                base_url: "http://localhost".to_string(),
                model: "test".to_string(),
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                query_params: Default::default(),
                env_http_headers: Default::default(),
                context_window: std::num::NonZeroU64::new(40_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            Box::new(persistence),
            event_tx,
            tokio_util::sync::CancellationToken::new(),
        );
        // Drain the seed records/events (initial history replacement etc.).
        let _ = recv.drain();
        while event_rx.try_recv().is_ok() {}

        let plan = ::compaction::plan_tool_result_pruning(
            &conversation,
            &chat_state::actor::state::EstimatedItemTokenCounter,
            2_000,  // 5% of a 40K window
            34_000, // 85% of a 40K window
        );
        assert_eq!(plan.items.len(), 1, "one oversized tool result to prune");
        let report = handle
            .prune_tool_results(plan)
            .await
            .expect("prune must succeed");
        assert_eq!(report.pruned_count, 1);

        // Persistence: exactly one snapshot rewrite carrying the pruned
        // content; the append log gained no new records (updates.jsonl keeps
        // the original content for rewind replay).
        let records = recv.drain();
        let replaces: Vec<&Vec<ConversationItem>> = records
            .iter()
            .filter_map(|r| match r {
                chat_state::PersistenceRecord::ReplaceHistory(items) => Some(items),
                _ => None,
            })
            .collect();
        assert_eq!(replaces.len(), 1, "exactly one history snapshot rewrite");
        let snapshot_tool_texts = tool_result_texts(replaces[0]);
        assert_eq!(snapshot_tool_texts.len(), 1);
        assert_ne!(
            &snapshot_tool_texts[0],
            &big_tool_text(),
            "snapshot must carry the pruned content"
        );
        assert!(
            records
                .iter()
                .all(|r| !matches!(r, chat_state::PersistenceRecord::Message(_))),
            "no append-log records may be written by pruning"
        );
        // The original content remains observable through the append log
        // written at push time (the rewind-replay source). Assert via the
        // chat-state event channel contrast below + the fact the snapshot
        // still keeps head/tail: replay semantics are "restore from
        // updates.jsonl, which never saw the prune".

        // No chat-state UI events from the prune command itself.
        assert!(
            event_rx.try_recv().is_err(),
            "prune must emit no chat-state events"
        );
        // Contrast: a compaction replace DOES emit reset/token events, proving
        // the channel is live and the silence above is meaningful. The
        // replace command is fire-and-forget, so barrier on a round-trip
        // before draining events.
        handle.replace_conversation_for_compaction(vec![ConversationItem::system("s")]);
        let _ = handle.get_conversation_len().await;
        let mut saw_reset = false;
        let mut saw_tokens = false;
        while let Ok(event) = event_rx.try_recv() {
            match event {
                chat_state::ChatStateEvent::ConversationReset { .. } => saw_reset = true,
                chat_state::ChatStateEvent::TokensUpdated { .. } => saw_tokens = true,
                _ => {}
            }
        }
        assert!(saw_reset, "compaction replace must emit ConversationReset");
        assert!(saw_tokens, "compaction replace must emit TokensUpdated");
    }));
}
