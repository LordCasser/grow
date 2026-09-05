//! `StopCancelled` hook tests (T2): a cancelled turn emits the observe-only
//! `StopCancelled` hook carrying the shared `CancellationCategory` reason and
//! the cancel trigger, and the hook can never delay or alter the cancel.
//!
//! The cancel path is driven on a live turn parked at the mock server's
//! terminal barrier (`expect_response_blocked`): `cancel_running_task`
//! returns — and the hook notification is already observable — while the turn
//! task is still in flight, which pins the "observe-only, non-blocking"
//! contract.

use super::support::*;
use super::*;
use serde_json::json;
use test_support::{InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer};
use tokio::sync::mpsc;

const END_TURN: &str = "end_turn";

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

/// A text `content_block` (Messages API).
fn text_block(text: &str) -> serde_json::Value {
    json!({ "type": "text", "text": text })
}

/// Assemble a Messages-API SSE turn with one text block and an `end_turn`
/// terminal stop_reason (same wire shape as `truncation_recovery_tests.rs`).
fn messages_turn(
    blocks: &[serde_json::Value],
    stop_reason: &str,
) -> test_support::ScriptedResponse {
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
        let start_block = json!({ "type": "text", "text": "" });
        events.push(
            json!({ "type": "content_block_start", "index": i, "content_block": start_block })
                .to_string(),
        );
        events.push(
            json!({
                "type": "content_block_delta",
                "index": i,
                "delta": { "type": "text_delta", "text": block["text"] }
            })
            .to_string(),
        );
        events.push(json!({ "type": "content_block_stop", "index": i }).to_string());
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
    test_support::ScriptedResponse::sse(
        events
            .into_iter()
            .map(test_support::SseEvent::data)
            .collect(),
    )
}

/// Build a `SessionActor` whose sampler is a real `SamplerActor` pointed at
/// `server` (the same fixture shape as `truncation_recovery_tests.rs`).
async fn actor_with_sampler(
    server: &MockInferenceServer,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    if let Some(mut cfg) = actor.chat_state_handle.get_sampling_config().await {
        cfg.base_url = server.url();
        cfg.api_backend = sampling_types::ApiBackend::Messages;
        actor.chat_state_handle.replace_sampling_route(cfg);
    }
    let sampler_config = sampler::SamplerConfig {
        api_key: Some("test-key".to_string()),
        base_url: server.url(),
        model: "test-model".to_string(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: sampling_types::ApiBackend::Messages,
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

/// Drive one user turn through the real loop, bounded so a broken continue
/// loop fails the test instead of hanging it.
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

/// The `grow/hooks/event` observation payloads drained from the gateway.
#[derive(Debug)]
struct ObservedHook {
    name: String,
    reason: Option<String>,
    trigger: Option<String>,
    prompt_id: Option<String>,
}

fn drain_hook_observations(
    rx: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> Vec<ObservedHook> {
    let mut events = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
            && args.request.method.as_ref() == "grow/hooks/event"
        {
            let params: serde_json::Value =
                serde_json::from_str(args.request.params.get()).unwrap();
            events.push(ObservedHook {
                name: params["hookEventName"].as_str().unwrap_or("").to_string(),
                reason: params["reason"].as_str().map(str::to_string),
                trigger: params["trigger"].as_str().map(str::to_string),
                prompt_id: params["promptId"].as_str().map(str::to_string),
            });
        }
    }
    events
}

/// A cancelled turn emits `StopCancelled` with the shared `CancellationCategory`
/// as `reason` (bare snake_case wire name) plus the cancel trigger, and the
/// observe-only dispatch does not block the cancel: `cancel_running_task`
/// returns while the turn is still parked at the mock server's terminal
/// barrier.
#[test]
fn stop_cancelled_emitted_with_reason_and_never_blocks_cancel() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            // Park the turn right before the terminal event so it stays in
            // flight across the cancel. The matcher must be `auxiliary`: the
            // fixture sampler sends no `x-grow-turn-idx` header, so the mock
            // server classifies real turn requests as `Auxiliary`
            // (`InferenceRequestKind::classify`), and a `foreground` matcher
            // would never claim.
            let mut blocked = server.expect_response_blocked(
                "stop_cancelled turn",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn(&[text_block("partial")], END_TURN),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server).await;
            actor.hooks.client_hooks.borrow_mut().insert(
                ::hooks::event::HookEventName::StopCancelled,
                vec![crate::extensions::hooks::ClientHookGroup {
                    matcher: None,
                    callback_ids: vec!["sc-obs".to_string()],
                    timeout: None,
                }],
            );

            let turn_actor = actor.clone();
            let mut turn = tokio::task::spawn_local(async move {
                run_user_turn(&turn_actor, "sc-cancel").await
            });
            // The sampler request has claimed the (blocked) expectation: the
            // server holds the response open at the terminal barrier, so the
            // turn is parked in flight until `release` below. Fail fast (with
            // the turn's own result) if the turn ends before its request is
            // claimed — that means the turn never reached the sampler.
            {
                let claim = tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    blocked.wait_received(),
                );
                tokio::pin!(claim);
                tokio::select! {
                    outcome = &mut claim => {
                        outcome.expect("the turn's sampler request must claim the expectation");
                    }
                    done = &mut turn => {
                        panic!(
                            "turn ended before its sampler request claimed the expectation: {done:?}"
                        );
                    }
                }
            }

            // Pin the live turn (production admission does this before the
            // task spawn); cancel reads it to attribute the torn-down turn.
            *actor.current_prompt_id.lock().unwrap() = Some("sc-cancel".to_string());
            tokio::time::timeout(
                // Regression guard: cancellation runs inside the Session
                // actor and must never call the public replay flush API,
                // whose acknowledgement is consumed by that same actor. The
                // old self-wait consistently hit its five-second timeout.
                std::time::Duration::from_secs(2),
                actor.cancel_running_task(false, false, false, Some("esc".to_string())),
            )
            .await
            .expect("cancel_running_task must not self-wait on replay persistence")
            .expect("cancel_running_task must succeed");

            // The cancel — including the hook dispatch — completed while the
            // turn task is still parked: an observe-only hook cannot block or
            // wait on the cancelled turn.
            assert!(
                !turn.is_finished(),
                "the turn must still be parked when cancel returns"
            );

            let observed = drain_hook_observations(&mut gateway_rx);
            let cancelled = observed
                .iter()
                .filter(|e| e.name == "stop_cancelled")
                .collect::<Vec<_>>();
            assert_eq!(
                cancelled.len(),
                1,
                "cancel must emit exactly one stop_cancelled observation, got {observed:?}"
            );
            let event = cancelled[0];
            assert_eq!(
                event.reason.as_deref(),
                Some("mid_turn_abort"),
                "reason must be the shared CancellationCategory wire name"
            );
            assert_eq!(event.trigger.as_deref(), Some("esc"));
            assert_eq!(event.prompt_id.as_deref(), Some("sc-cancel"));

            // Release the parked turn so the fixture finishes cleanly.
            blocked.release();
            let result = tokio::time::timeout(std::time::Duration::from_secs(30), turn)
                .await
                .expect("turn task must resolve after release")
                .expect("turn task must not panic");
            let error = result.expect_err(
                "the direct fixture turn no longer owns foreground after cancellation",
            );
            assert!(
                error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("error_kind"))
                    .and_then(serde_json::Value::as_str)
                    == Some("turn_boundary_persistence_failed"),
                "the cancelled turn must resolve with its lost-ownership terminal, got {error:?}"
            );
        }));
    });
}

/// An idle cancel (no running turn, no pinned prompt id) emits nothing: there
/// is no turn to attribute a cancellation to.
#[test]
fn stop_cancelled_not_emitted_without_a_cancelled_turn() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, mut gateway_rx) = actor_with_sampler(&server).await;
            actor.hooks.client_hooks.borrow_mut().insert(
                ::hooks::event::HookEventName::StopCancelled,
                vec![crate::extensions::hooks::ClientHookGroup {
                    matcher: None,
                    callback_ids: vec!["sc-obs".to_string()],
                    timeout: None,
                }],
            );

            actor
                .cancel_running_task(false, false, false, Some("esc".to_string()))
                .await
                .expect("idle cancellation must succeed");

            let observed = drain_hook_observations(&mut gateway_rx);
            assert!(
                !observed.iter().any(|e| e.name == "stop_cancelled"),
                "an idle cancel must not emit StopCancelled, got {observed:?}"
            );
        }));
    });
}
