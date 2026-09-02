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
//! Plus a layering test: pruning appends exactly one Surface replacement,
//! emits no chat-state UI events, and leaves the original Timeline fact
//! available behind the shadowing edge for rewind.
//!
//! Scenario 4 (`context_window_exceeded_triggers_compaction`, compaction
//! success → continue resample) is the existing regression test in
//! `truncation_recovery_tests.rs`.

use super::support::*;
use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};
use tokio::sync::mpsc;

#[test]
fn async_compaction_threshold_tracks_resolved_hard_threshold() {
    assert_eq!(compaction::pre_compact_threshold(80), Some(70));
    assert_eq!(compaction::pre_compact_threshold(75), Some(65));
    assert_eq!(compaction::pre_compact_threshold(11), Some(1));
    assert_eq!(compaction::pre_compact_threshold(10), None);
    assert_eq!(compaction::pre_compact_threshold(0), None);
}

#[test]
fn async_compaction_starts_at_exact_pre_threshold_but_not_at_the_hard_threshold() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            for (main, tokens, should_start) in [
                (80, 69_999, false),
                (80, 70_000, true),
                (80, 79_999, true),
                (80, 80_000, false),
                (65, 54_999, false),
                (65, 55_000, true),
                (10, 10_000, false),
            ] {
                let (actor, _notifications) =
                    actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000)
                        .await;
                actor.compaction.threshold_percent.set(main);
                actor.compaction.pre_prune.set(false);
                seed_closed_compaction_range(&actor, 88_000).await;
                actor
                    .chat_state_handle
                    .record_provider_context_anchor(tokens);
                actor.state.lock().await.foreground =
                    ForegroundState::RegularTurn(running_task_stub("threshold"));
                actor.background_compaction_boundary().await.unwrap();
                assert_eq!(
                    actor.compaction.background.borrow().is_some(),
                    should_start,
                    "main={main}, tokens={tokens}"
                );
                actor
                    .cancel_background_compaction("test_complete")
                    .await
                    .unwrap();
            }
        }));
    });
}

/// Real foreground sampling proceeds while the auxiliary HTTP response is
/// held at its terminal event. No timing-based provider sleeps are involved.
#[test]
fn async_compaction_runs_beside_foreground_and_publishes_only_at_boundary() {
    async_compaction_scenario("publish");
}

#[test]
fn async_compaction_promotes_the_same_provider_request() {
    async_compaction_scenario("promote");
}

#[test]
fn async_compaction_cancel_discards_late_provider_result() {
    async_compaction_scenario("cancel");
}

#[test]
fn async_compaction_goal_concurrency_is_exactly_charged() {
    async_compaction_scenario("goal");
}

#[test]
fn async_compaction_rejects_changed_model_route() {
    async_compaction_scenario("model");
}

#[test]
fn async_compaction_failure_does_not_interrupt_or_restart_in_the_same_turn() {
    async_compaction_scenario("failure");
}

#[test]
fn async_compaction_goal_budget_closes_and_settles_without_a_wait_cycle() {
    async_compaction_scenario("budget");
}

#[test]
fn async_compaction_authority_transition_cancels_before_publication() {
    async_compaction_scenario("control");
}

#[test]
fn async_compaction_promotion_keeps_the_original_deadline() {
    async_compaction_scenario("timeout");
}

#[test]
fn async_compaction_rewind_preview_preserves_but_commit_invalidates_the_job() {
    async_compaction_scenario("rewind");
}

fn async_compaction_scenario(action: &'static str) {
    run_with_session_stack(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(tokio::task::LocalSet::new().run_until(async {
            use test_support::{InferenceEndpoint, InferenceRequestMatcher};
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!("background compacted summary: {}", "Old decisions and verified evidence. ".repeat(35));
            let summary_response = if action == "failure" {
                ScriptedResponse::json(400, json!({"type":"error","error":{"type":"invalid_request_error","message":"unsupported summary parameter"}}))
            } else {
                messages_turn(&[(&summary, END_TURN)], END_TURN)
            };
            let mut auxiliary = server.expect_response_blocked("background summary",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                summary_response);
            let mut tool = server.expect_response("foreground executes tool",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                ScriptedResponse::sse([
                    json!({"type":"message_start","message":{"id":"tool-msg","type":"message","role":"assistant","content":[],"model":"test","usage":{"input_tokens":74_000,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"async-todo","name":"todo_write","input":{}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"todos\":[{\"id\":\"latest\",\"content\":\"latest todo while summary runs\",\"status\":\"in_progress\"}]}"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":74_000,"output_tokens":5}}),
                    json!({"type":"message_stop"}),
                ].into_iter().map(|event| SseEvent::data(event.to_string())).collect()));
            let mut foreground = server.expect_response("foreground continues",
                // This fixture has one tool and no attribution callback, so
                // the mock classifies its foreground requests as auxiliary.
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                messages_turn_with_usage(&[("foreground response after freeze", END_TURN)], END_TURN, 74_000));
            let (actor, mut notifications) = actor_with_sampler_cw_ex(&server, sampling_types::ApiBackend::Messages, 100_000, None, if action == "timeout" { 2 } else { 0 }).await;
            *actor.agent.borrow_mut() = test_grow_build_agent_with_todo().await;
            if matches!(action, "goal" | "budget") {
                actor.goal_tracker.lock().create_goal("async-goal".into(), "verify concurrent accounting".into(), Some(if action == "budget" { 75_000 } else { 500_000 }), "now".into()).unwrap();
                actor.behavior.lock().select_behavior(tool_types::BehaviorId::Goal);
                actor.sync_goal_usage_window();
            }
            actor.compaction.threshold_percent.set(80);
            actor.compaction.pre_prune.set(false);
            replace_test_surface(&actor.chat_state_handle, vec![
                ConversationItem::system("test system"), prompt("old task", 0),
                ConversationItem::assistant("x".repeat(200_000)), prompt("recent task", 1),
                ConversationItem::assistant("y".repeat(88_000)),
            ]).await;
            let turn = tokio::task::spawn_local({ let actor = actor.clone(); async move { run_user_turn(&actor, "async-foreground").await } });
            tokio::time::timeout(std::time::Duration::from_secs(10), auxiliary.wait_blocked()).await.expect("background request starts");
            tokio::time::timeout(std::time::Duration::from_secs(10), turn).await.expect("foreground is not blocked by summary").unwrap().unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), tool.wait_satisfied()).await.expect("tool exchange completed beside the summary");
            tokio::time::timeout(std::time::Duration::from_secs(5), foreground.wait_satisfied()).await.expect("foreground expectation consumed");
            if action == "goal" { assert_eq!(actor.goal_tokens_used(), 148_010); }
            assert_eq!(actor.compaction.background.borrow().is_some(), action != "budget");
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(!events.iter().any(|event| matches!(event.kind, chat_state::TimelineEventKind::Compaction(chat_state::CompactionEvent::Summary { .. }))));
            let mut early_notifications = Vec::new();
            while let Ok(message) = notifications.try_recv() { early_notifications.push(format!("{message:?}")); }
            assert!(!early_notifications.iter().any(|message| message.contains("auto_compact_started")), "background must not display foreground compaction");
            let mut promotion = None;
            if action == "cancel" {
                actor.cancel_background_compaction("test_invalidation").await.unwrap();
            } else if action == "rewind" {
                let target_prompt_index = actor.chat_state_handle.get_prompt_index().await - 1;
                let request = RewindRequest { target_prompt_index, force: false, mode: RewindMode::ConversationOnly };
                actor.handle_rewind(request.clone()).await.unwrap();
                assert!(actor.compaction.background.borrow().is_some(), "read-only rewind preview must not invalidate computation");
                assert!(actor.handle_rewind(RewindRequest { force: true, ..request }).await.unwrap().success);
                assert!(actor.compaction.background.borrow().is_none());
            } else if action == "control" {
                let (behavior, goal) = actor.capture_control_authorities();
                actor.persist_behavior_transition_durably(behavior, goal).await.unwrap();
                assert!(actor.compaction.background.borrow().is_none());
            } else if action == "model" {
                let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                config.context_window = std::num::NonZeroU64::new(110_000).unwrap();
                actor.chat_state_handle.update_sampling_config(config);
            } else if action == "promote" || action == "timeout" {
                if action == "timeout" { tokio::time::sleep(std::time::Duration::from_millis(1_200)).await; }
                actor.chat_state_handle.push_user_message_durably(ConversationItem::user("late input ".repeat(4_000))).await.unwrap();
                promotion = Some(tokio::task::spawn_local({ let actor = actor.clone(); async move {
                    actor.run_compact_only(compaction::AutoCompactTriggerInfo {
                        tokens_used: 85_000, context_window: 100_000, percentage: 85, source: "pre_sampling",
                    }).await
                }}));
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        if actor.chat_state_handle.timeline_events().await.unwrap().iter().any(|event| matches!(event.kind,
                            chat_state::TimelineEventKind::Compaction(chat_state::CompactionEvent::Promoted { .. }))) { break; }
                        tokio::task::yield_now().await;
                    }
                }).await.expect("existing computation is promoted");
                assert!(!promotion.as_ref().unwrap().is_finished());
            }
            if action == "timeout" {
                let error = tokio::time::timeout(std::time::Duration::from_millis(1_100), promotion.take().unwrap()).await.expect("promotion must use the original deadline, not a fresh two seconds").unwrap().unwrap_err();
                assert!(format!("{error:?}").contains("wall-clock budget"));
            }
            auxiliary.release();
            if !matches!(action, "cancel" | "budget" | "control" | "timeout" | "rewind") {
                tokio::time::timeout(std::time::Duration::from_secs(5), auxiliary.wait_satisfied()).await.expect("auxiliary expectation completed");
            }
            if let Some(promotion) = promotion {
                tokio::time::timeout(std::time::Duration::from_secs(10), promotion).await.unwrap().unwrap().unwrap();
            } else if !matches!(action, "cancel" | "budget" | "control" | "timeout" | "rewind") {
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    while actor.compaction.background.borrow().is_some() {
                        tokio::task::yield_now().await;
                        actor.background_compaction_boundary().await.unwrap();
                    }
                }).await.expect("ready result commits at the next boundary");
            }
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let completed = events.iter().filter(|event| matches!(event.kind, chat_state::TimelineEventKind::Compaction(chat_state::CompactionEvent::Completed { .. }))).count();
            let should_commit = matches!(action, "publish" | "promote" | "goal");
            assert_eq!(completed, usize::from(should_commit));
            assert_eq!(server.messages_request_count(), 3, "promotion must not issue another summary request");
            let surface = actor.chat_state_handle.get_conversation().await;
            let text = surface.iter().map(ConversationItem::text_content).collect::<Vec<_>>().join("\n");
            assert_eq!(text.contains("foreground response after freeze"), action != "rewind");
            assert_eq!(surface.iter().any(|item| matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == "async-todo")), action != "rewind");
            assert_eq!(text.contains("background compacted summary"), should_commit);
            if should_commit {
                assert!(surface.iter().any(|item| matches!(item, ConversationItem::User(user) if user.synthetic_reason == Some(sampling_types::SyntheticReason::CompactionMeta)) && item.text_content().contains("latest todo while summary runs")), "reminders must use the Todo state at commit, not at preparation");
            }
            let (notifications, completions) = drain_session_updates(&mut notifications);
            assert_eq!(completions.len(), usize::from(should_commit));
            if should_commit { assert_eq!(completions[0]["update"]["async_compact"], action != "promote"); }
            assert_eq!(notifications.iter().filter(|kind| *kind == "auto_compact_started").count(), usize::from(matches!(action, "promote" | "timeout")));
            if action == "failure" {
                assert!(actor.compaction.background_failed.get());
                actor.state.lock().await.foreground = ForegroundState::RegularTurn(running_task_stub("same-turn"));
                actor.background_compaction_boundary().await.unwrap();
                assert!(actor.compaction.background.borrow().is_none());
                assert_eq!(server.messages_request_count(), 3);
            }
            if action == "promote" { assert!(text.contains("late input")); }
            if action == "goal" { assert_eq!(actor.goal_tokens_used(), 148_025, "each foreground and summary attempt is charged exactly once"); }
            if action == "budget" {
                tokio::time::timeout(std::time::Duration::from_secs(2), actor.goal_usage_window.wait_for_owner_settlements_through(&actor.session_id_string(), 0)).await.expect("cancelled summary accounting must settle without a wait cycle");
                assert_eq!(actor.goal_tokens_used(), 148_025);
                assert!(actor.goal_usage_window.begin_model_attempt(&actor.session_id_string(), 0, Some("async-goal")).is_err());
            }
        }));
    });
}

fn prompt(text: impl Into<String>, index: usize) -> ConversationItem {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(index);
    item
}

/// Wire `stop_reason` values for the Messages API (see
/// `sampler/src/stream/messages.rs` mapping).
const END_TURN: &str = "end_turn";
const CONTEXT_WINDOW_EXCEEDED: &str = "model_context_window_exceeded";

// ─── Messages-API SSE builder ─────────────────────────────────────────────

/// Assemble a Messages-API SSE turn from `(text, stop_reason)` blocks and a
/// terminal `stop_reason`. `input_tokens` seeds the reported usage so tests
/// can pin the chat-state provider anchor recorded by the turn loop.
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
    actor_with_sampler_cw_ex(server, api_backend, context_window, None, 0).await
}

/// [`actor_with_sampler_cw`] variant for fork-scenario tests: sets
/// `startup_hints.inherited_prefix_len` before the actor is shared behind the
/// `Arc` (the field has no interior mutability).
async fn actor_with_sampler_cw_ex(
    server: &MockInferenceServer,
    api_backend: sampling_types::ApiBackend,
    context_window: u64,
    inherited_prefix_len: Option<usize>,
    wall_clock_budget_secs: u64,
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
    let mut actor = create_test_actor(0, context_window, 85, gateway_tx, persistence_tx).await;
    actor.compaction.wall_clock_budget_secs = wall_clock_budget_secs;
    let (goal_usage_tx, mut goal_usage_rx) = mpsc::unbounded_channel();
    actor.goal_usage_window = goal_support::GoalUsageWindow::new(goal_usage_tx.clone(), None);
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
    let usage_actor = actor.clone();
    tokio::task::spawn_local(async move {
        let _mailbox_owner = goal_usage_tx;
        while let Some(SessionCommand::SettleGoalUsageAttempt {
            attempt_id,
            respond_to,
        }) = goal_usage_rx.recv().await
        {
            let _ = respond_to.send(
                usage_actor
                    .settle_claimed_goal_usage_attempt(&attempt_id)
                    .await,
            );
        }
    });
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
    actor.state.lock().await.foreground =
        ForegroundState::RegularTurn(running_task_stub(prompt_id));
    let behavior = actor.behavior.lock().behavior();
    let result = tokio::time::timeout(
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
            behavior,
            None,
            None,
            false,
            None,
            None,
        ),
    )
    .await
    .expect("turn must finish within the timeout (no runaway continue loop)");
    actor.state.lock().await.foreground = ForegroundState::Idle;
    result
}

/// Oversized tool-result payload (200KB → 50K tokens under bytes/4).
fn big_tool_text() -> String {
    "x".repeat(200_000)
}

/// Push `count` well-formed (assistant tool_use → tool result) rounds into the
/// conversation, then barrier on a round-trip so all pushes are processed.
async fn seed_tool_result_rounds(actor: &SessionActor, count: usize) {
    replace_test_surface(
        &actor.chat_state_handle,
        vec![ConversationItem::system("test system prompt")],
    )
    .await;
    for i in 0..count {
        actor
            .chat_state_handle
            .push_user_message(prompt(format!("u{i}"), i));
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

/// Seed one eligible closed turn followed by a large verbatim tail. Partial
/// compaction must summarize the first turn and preserve the second.
async fn seed_closed_compaction_range(actor: &SessionActor, retained_tail_chars: usize) {
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
        .push_assistant_response(ConversationItem::assistant("y".repeat(retained_tail_chars)));
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
            actor
                .chat_state_handle
                .record_provider_context_anchor(86_000);
            // Current turn adds only a small amount of Surface pressure.
            actor
                .chat_state_handle
                .push_user_message(prompt("current turn", 2));
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
                "pruning must reduce projected context pressure"
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

/// Pruning can reduce the Surface while leaving provider-anchored pressure at
/// the trigger threshold. The strict gate must then keep the summary path.
#[test]
fn pre_prune_insufficient_projection_runs_summary() {
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
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            seed_tool_result_rounds(&actor, 2).await;
            actor
                .chat_state_handle
                .push_user_message(prompt("current turn", 2));
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

            // The static Surface is roughly 150K tokens. Pruning the closed
            // results removes roughly 100K while preserving the current turn,
            // so this provider anchor remains above the 85K trigger.
            actor
                .chat_state_handle
                .record_provider_context_anchor(190_000);
            let _ = actor.chat_state_handle.get_conversation_len().await;
            let trigger = compaction::AutoCompactTriggerInfo {
                tokens_used: 190_000,
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
                "the strict gate must retain summary compaction at the threshold"
            );
            assert!(
                actor.chat_state_handle.get_projected_tokens().await >= 85_000,
                "the signed Surface delta must preserve provider overhead"
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                tool_result_texts(&conversation)
                    .iter()
                    .any(|text| text != &big_tool_text()),
                "pre-prune must still persist its Surface reduction"
            );

            actor.compaction.pre_prune.set(false);
            actor
                .run_compact_only(trigger)
                .await
                .expect("summary path must run after the strict gate rejects the skip");
            assert_eq!(
                server.messages_request_count(),
                1,
                "one summary model call must run"
            );
            let full_text = actor
                .chat_state_handle
                .get_conversation()
                .await
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                full_text.contains("compacted summary"),
                "summary output must replace the compacted range"
            );
        }));
    });
}

#[test]
fn workflow_guidance_survives_mid_turn_compaction_once() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        rt.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!(
                "workflow history summary: {}",
                "retained evidence and execution state. ".repeat(30)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[(&summary, END_TURN)], END_TURN),
            );
            let (actor, _gateway_rx) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Workflow);
            *actor.turn_behavior.lock() = tool_types::BehaviorId::Workflow;
            let workflow_context = actor
                .workflow_behavior_context()
                .expect("test Workflow workspace is available");
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::system("test system prompt"),
                    prompt("old closed turn", 0),
                    ConversationItem::assistant("x".repeat(120_000)),
                    ConversationItem::user_meta(format!(
                        "<system-reminder>\n{workflow_context}\n</system-reminder>"
                    )),
                    prompt("current workflow turn", 1),
                    ConversationItem::assistant("partial current response ".repeat(5_000)),
                ],
            )
            .await;
            actor.compaction.pre_prune.set(false);

            actor
                .run_compact_only(compaction::AutoCompactTriggerInfo {
                    tokens_used: 90_000,
                    context_window: 100_000,
                    percentage: 90,
                    source: "workflow_test",
                })
                .await
                .expect("Workflow compaction succeeds");

            let full_text = actor
                .chat_state_handle
                .get_conversation()
                .await
                .iter()
                .map(ConversationItem::text_content)
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                full_text.matches(&workflow_context).count(),
                1,
                "compaction must shadow the old turn reminder and reinstall one authoritative Workflow contract"
            );
            assert!(full_text.contains("current workflow turn"));
        }));
    });
}

// ─── Scenario 3 ────────────────────────────────────────────────────────────

/// `ModelContextWindowExceeded` + a compaction whose projection still exceeds the
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
            // overflow branch fires, and the signed Surface reduction is not
            // large enough to bring provider-anchored pressure under the window.
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
            // Keep the static estimate below the 85% preflight trigger, but
            // make the retained tail large enough that removing the 6K-token
            // source still leaves provider-anchored pressure over the 100K
            // window after applying the signed Surface reduction.
            seed_closed_compaction_range(&actor, 150_000).await;

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
            assert_eq!(
                err.data
                    .as_ref()
                    .and_then(|data| data.get("error_kind"))
                    .and_then(|value| value.as_str()),
                Some(::hooks::event::StopFailureKind::ContextWindowExceeded.as_str()),
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
            let trajectory = actor.chat_state_handle.trajectory().await.unwrap();
            let compaction_terminal = trajectory
                .rows
                .iter()
                .rev()
                .find(|row| row.kind.starts_with("compaction.") && row.state != "started")
                .expect("compaction must have a terminal Timeline fact");
            assert_eq!(
                compaction_terminal.state, "completed",
                "the Surface replacement committed; only the enclosing turn fails"
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

            // Phase A: an already-open compaction transaction rejects the
            // pre-prune replacement deterministically. This exercises the
            // same `PruneError::Timeline` fail-open arm without relying on an
            // unsafe external full-Surface overwrite race.
            let (mut trace_rx, _guard) = capture_trace_events();
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Started {
                        mode: chat_state::CompactionMode::Foreground,
                        id: "pre-prune-conflict".into(),
                        source_items: actor.chat_state_handle.get_conversation_len().await,
                        prompt_index: actor.chat_state_handle.get_prompt_index().await,
                    },
                ))
                .await
                .unwrap();
            let pruned = actor
                .maybe_pre_prune(&trigger_phase_a)
                .await
                .expect("maybe_pre_prune must not error");
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Failed {
                        id: "pre-prune-conflict".into(),
                        duration_ms: 1,
                        error: "test conflict".into(),
                    },
                ))
                .await
                .unwrap();
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
                    e.target == SESSION_LOG
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
            replace_test_surface(
                &actor.chat_state_handle,
                vec![
                    ConversationItem::system("test system prompt"),
                    prompt("u0", 0),
                    ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                        id: "call-0".into(),
                        name: "grep".to_string(),
                        arguments: "{}".into(),
                    }]),
                    ConversationItem::tool_result("call-0", big_tool_text()),
                    prompt("recent retained turn", 1),
                    ConversationItem::assistant("y".repeat(40_000)),
                ],
            )
            .await;
            actor
                .chat_state_handle
                .record_provider_context_anchor(40_000);
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
            actor
                .chat_state_handle
                .record_provider_context_anchor(86_000);
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
            actor.chat_state_handle.push_user_message(prompt("u1", 1));
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
            // Replace the projection with a provider anchor before evaluating
            // the next pruning transaction.
            actor
                .chat_state_handle
                .record_provider_context_anchor(105_000);
            let _ = actor.chat_state_handle.get_conversation_len().await;

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
            actor
                .chat_state_handle
                .record_provider_context_anchor(105_000);
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
        let (persistence, mut recv) = chat_state::MockTimelinePersistence::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<chat_state::ChatStateEvent>();
        let conversation = vec![
            ConversationItem::system("test system prompt"),
            prompt("u0", 0),
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
        // Actor bootstrap is strictly serialized with later commands. Wait for
        // a query to cross that boundary before draining the seed records.
        assert_eq!(handle.get_conversation().await.len(), conversation.len());
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

        // Persistence: exactly one Timeline replacement carrying the pruned
        // content. The original node remains addressable for rewind.
        let records = recv.drain();
        let replacements: Vec<&chat_state::MessageEvent> = records
            .iter()
            .filter_map(|r| match r {
                chat_state::PersistenceRecord::Timeline(event) => event.messages(),
                _ => None,
            })
            .filter(|event| event.cause == chat_state::MessageCause::ToolResultPrune)
            .collect();
        assert_eq!(replacements.len(), 1, "exactly one Surface replacement");
        let snapshot_tool_texts = tool_result_texts(&replacements[0].items);
        assert_eq!(snapshot_tool_texts.len(), 1);
        assert_ne!(
            &snapshot_tool_texts[0],
            &big_tool_text(),
            "snapshot must carry the pruned content"
        );
        assert_eq!(
            records.len(),
            1,
            "pruning writes no duplicate persistence rail"
        );

        // No chat-state UI events from the prune command itself.
        assert!(
            event_rx.try_recv().is_err(),
            "prune must emit no chat-state events"
        );
        // Contrast: a durable context replacement emits reset/token events,
        // proving the channel is live and the silence above is meaningful.
        replace_test_surface(
            &handle,
            vec![
                ConversationItem::system("test system prompt"),
                prompt("rebuilt", 0),
            ],
        )
        .await;
        let mut saw_reset = false;
        let mut saw_tokens = false;
        while let Ok(event) = event_rx.try_recv() {
            match event {
                chat_state::ChatStateEvent::ConversationReset { .. } => saw_reset = true,
                chat_state::ChatStateEvent::ContextPressureUpdated { .. } => saw_tokens = true,
                _ => {}
            }
        }
        assert!(saw_reset, "compaction replace must emit ConversationReset");
        assert!(
            saw_tokens,
            "compaction replace must emit ContextPressureUpdated"
        );
    }));
}
