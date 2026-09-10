use super::support::*;
use super::*;
use sampling_types::{ConversationItem, ConversationResponse, TokenUsage, ToolSpec};

fn response_with_usage(total_tokens: u32) -> ConversationResponse {
    ConversationResponse {
        items: vec![ConversationItem::assistant("ok")],
        stop_reason: None,
        usage: Some(TokenUsage {
            prompt_tokens: total_tokens.saturating_sub(50),
            completion_tokens: 50,
            total_tokens,

            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_creation_prompt_tokens: 0,
        }),
        cost_usd_ticks: None,
        message_chunks_emitted: 1,
        doom_loop_signals: Vec::new(),
        stop_message: None,
        message_id: None,
        raw_stop_reason: None,
        stop_sequence: None,
        native_continuation: None,
    }
}

fn response_without_usage() -> ConversationResponse {
    ConversationResponse {
        items: vec![ConversationItem::assistant("ok")],
        stop_reason: None,
        usage: None,
        cost_usd_ticks: None,
        message_chunks_emitted: 1,
        doom_loop_signals: Vec::new(),
        stop_message: None,
        message_id: None,
        raw_stop_reason: None,
        stop_sequence: None,
        native_continuation: None,
    }
}

async fn settle_attempt(
    actor: &SessionActor,
    attempt_key: &str,
    captured_prompt_index: usize,
    model_id: &str,
    response: &ConversationResponse,
) {
    let usage = response
        .usage
        .clone()
        .expect("settlement fixture requires provider usage");
    let sink = actor.sampling_usage_sink(model_id.to_owned(), captured_prompt_index);
    sink(sampler::AttemptUsage::Known {
        attempt_key: attempt_key.to_owned(),
        cost_usd_ticks: response.cost_usd_ticks,
        api_duration_ms: None,
        scope: None,
        usage,
    })
    .await
    .expect("attempt usage settlement");
}

fn usage_with_completion_tokens(completion_tokens: u32) -> TokenUsage {
    TokenUsage {
        prompt_tokens: 0,
        completion_tokens,
        total_tokens: completion_tokens,
        reasoning_tokens: 0,
        cached_prompt_tokens: 0,
        cache_creation_prompt_tokens: 0,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn sampling_usage_sink_deduplicates_known_usage_and_debits_output_grant_once() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let budget = crate::tools::tool_context::TaskOutputTokenBudget::limited(10);
            actor.tool_context.task_output_token_budget = Some(budget.clone());
            record_test_prompt(&actor, "sink-known").await;
            let captured_prompt_index = actor
                .chat_state_handle
                .current_prompt_index()
                .await
                .expect("current prompt index");
            let sink = actor.sampling_usage_sink("test-model".into(), captured_prompt_index);

    sink(sampler::AttemptUsage::Known {
        attempt_key: "same-attempt".into(),
        cost_usd_ticks: None,
        api_duration_ms: None,
        scope: None,
        usage: usage_with_completion_tokens(4),
    })
    .await
    .unwrap();
    sink(sampler::AttemptUsage::Known {
        attempt_key: "second-attempt".into(),
        cost_usd_ticks: None,
        api_duration_ms: None,
        scope: None,
        usage: usage_with_completion_tokens(3),
    })
    .await
    .unwrap();
    assert_eq!(budget.remaining(), Some(3));

    // Replayed settlement is idempotent across both ledgers and the output
    // grant, so it cannot reopen capacity for another provider request.
    sink(sampler::AttemptUsage::Known {
        attempt_key: "same-attempt".into(),
        cost_usd_ticks: None,
        api_duration_ms: None,
        scope: None,
        usage: usage_with_completion_tokens(4),
    })
    .await
    .unwrap();
    assert_eq!(budget.remaining(), Some(3));
    let usage = actor
        .chat_state_handle
        .try_get_session_usage()
        .await
        .unwrap();
    assert_eq!(usage.totals.model_calls, 2);
    assert_eq!(usage.totals.output_tokens, 7);
            assert!(!usage.incomplete);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn sampling_usage_sink_marks_unknown_usage_and_exhausts_output_grant() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let budget = crate::tools::tool_context::TaskOutputTokenBudget::limited(10);
            actor.tool_context.task_output_token_budget = Some(budget.clone());
            record_test_prompt(&actor, "sink-incomplete").await;
            let captured_prompt_index = actor
                .chat_state_handle
                .current_prompt_index()
                .await
                .expect("current prompt index");
            let sink = actor.sampling_usage_sink("test-model".into(), captured_prompt_index);

    sink(sampler::AttemptUsage::Incomplete {
        attempt_key: "unknown-attempt".into(),
        scope: None,
    })
    .await
    .unwrap();
    assert_eq!(budget.remaining(), Some(0));
    assert_eq!(budget.usage(), (10, true));
    let prompt = actor
        .chat_state_handle
        .try_get_prompt_usage()
        .await
        .unwrap()
        .expect("prompt ledger");
    let session = actor
        .chat_state_handle
        .try_get_session_usage()
        .await
        .unwrap();
    assert!(prompt.incomplete);
            assert!(session.incomplete);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn usage_keeps_selected_provider_models_separate_from_wire_aliases() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "usage").await;
            let mut response = response_with_usage(150);
            if let ConversationItem::Assistant(item) = &mut response.items[0] {
                item.model_id = Some("same-wire-model".into());
            }
            response.usage.as_mut().unwrap().cached_prompt_tokens = 80;
            for (attempt, catalog) in [
                "provider-a/shared",
                "provider-b/shared",
                "provider-a/shared",
            ]
            .into_iter()
            .enumerate()
            {
                settle_attempt(
                    &actor,
                    &format!("usage-{attempt}"),
                    captured_prompt_index,
                    catalog,
                    &response,
                )
                .await;
                actor
                    .record_response_token_usage(&response, None, Some(catalog.into()), None, true)
                    .await
                    .unwrap();
            }
            let usage = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();
            assert_eq!(usage.by_model.len(), 2);
            assert!(!usage.by_model.contains_key("same-wire-model"));
            assert_eq!(usage.by_model["provider-a/shared"].total_tokens(), 300);
            assert_eq!(usage.by_model["provider-b/shared"].total_tokens(), 150);
            assert_eq!(usage.totals.total_tokens(), 450);
            assert_eq!(usage.totals.cached_read_tokens, 240);
            assert_eq!(usage.totals.input_tokens, 300);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn quarantined_response_is_billed_without_restoring_its_context_anchor() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "quarantined").await;
            actor
                .chat_state_handle
                .push_response_durably(
                    vec![ConversationItem::assistant_tool_calls(vec![
                        sampling_types::ToolCall {
                            id: "".into(),
                            name: "".into(),
                            arguments: "{}".into(),
                        },
                    ])],
                    None,
                )
                .await
                .unwrap();
            let repaired_tokens = actor.chat_state_handle.get_projected_tokens().await;
            let response = response_with_usage(150_000);
            settle_attempt(
                &actor,
                "quarantined-attempt",
                captured_prompt_index,
                "test",
                &response,
            )
            .await;
            actor
                .record_response_token_usage(&response, None, None, None, false)
                .await
                .unwrap();
            assert_eq!(
                actor.chat_state_handle.get_projected_tokens().await,
                repaired_tokens
            );
            let usage = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();
            assert_eq!(usage.totals.model_calls, 1);
            assert_eq!(usage.totals.input_tokens, 149_950);
        })
        .await;
}

/// A provider response with usage must replace the local projection with the
/// provider's canonical context anchor while recording lifetime usage.
#[tokio::test(flavor = "current_thread")]
async fn anchors_projected_context_from_response_usage() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "anchor").await;
            let _sync = actor.chat_state_handle.get_projected_tokens().await;
            assert!(
                actor.chat_state_handle.get_projected_tokens().await > 0,
                "the immutable System governance head contributes to projected context"
            );

            let response = response_with_usage(150_000);
            settle_attempt(
                &actor,
                "anchor-attempt",
                captured_prompt_index,
                "provider/model",
                &response,
            )
            .await;
            actor
                .record_response_token_usage(
                    &response,
                    None,
                    Some("provider/model".into()),
                    None,
                    true,
                )
                .await
                .unwrap();

            assert_eq!(
                actor.chat_state_handle.get_projected_tokens().await,
                150_000
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .expect("chat-state alive")
                .expect("prompt ledger opened");
            assert_eq!(prompt.totals.model_calls, 1);
            assert_eq!(prompt.totals.input_tokens, 149_950);
            assert!(prompt.totals.cost_usd_ticks.is_none());
            assert_eq!(prompt.by_model["provider/model"].model_calls, 1);
            assert_eq!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .expect("chat-state actor alive")
                    .totals
                    .model_calls,
                1
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn goal_usage_accumulates_model_consumption_when_context_pressure_falls() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-1".into(),
                    "finish the architecture".into(),
                    Some(10_000),
                    "now".into(),
                )
                .unwrap();
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            actor.sync_goal_usage_window();
            let (goal_tx, mut goal_rx) = tokio::sync::mpsc::unbounded_channel();
            let goal_tx_keepalive = goal_tx.clone();
            actor.goal_usage_window =
                crate::session::actor::goal_support::GoalUsageWindow::new(
                    goal_tx,
                    Some("goal-1".into()),
                );
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "goal").await;

            let mut first = response_with_usage(1_080);
            first.usage = Some(TokenUsage {
                prompt_tokens: 1_000,
                completion_tokens: 80,
                total_tokens: 1_080,
                reasoning_tokens: 40,
                cached_prompt_tokens: 700,
                cache_creation_prompt_tokens: 0,
            });
            let scope = actor
                .goal_usage_window
                .begin_model_attempt(&actor.session_id_string(), 0, Some("goal-1"))
                .await
                .unwrap()
                .unwrap();
            let sink = actor.sampling_usage_sink("test".into(), captured_prompt_index);
            let settlement = sink(sampler::AttemptUsage::Known {
                attempt_key: "goal-attempt-1".into(),
                cost_usd_ticks: first.cost_usd_ticks,
                api_duration_ms: None,
                scope: Some(scope),
                usage: first.usage.clone().unwrap(),
            });
            tokio::pin!(settlement);
            let command = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::select! {
                    command = goal_rx.recv() => command.expect("Goal usage command"),
                    result = &mut settlement => panic!("settlement completed before root ack: {result:?}"),
                }
            })
            .await
            .expect("Goal usage command timeout");
            let respond_to = match command {
                crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id,
                    respond_to,
                } => {
                    assert_eq!(actor.goal_usage_window.attempt_goal_id(&attempt_id).as_deref(), Some("goal-1"));
                    let result = actor.settle_claimed_goal_usage_attempt(&attempt_id).await;
                    respond_to.send(result).expect("Goal settlement acknowledgement");
                    true
                }
                _ => panic!("unexpected Goal usage command"),
            };
            assert!(respond_to);
            settlement.await.expect("attempt usage settlement");
            actor
                .record_response_token_usage(&first, None, None, Some("goal-1"), true)
                .await
                .unwrap();
            assert_eq!(actor.goal_tokens_used(), 1_080);

            let mut after_compaction = response_with_usage(400);
            after_compaction.usage = Some(TokenUsage {
                prompt_tokens: 350,
                completion_tokens: 50,
                total_tokens: 400,
                reasoning_tokens: 20,
                cached_prompt_tokens: 300,
                cache_creation_prompt_tokens: 0,
            });
            let scope = actor
                .goal_usage_window
                .begin_model_attempt(&actor.session_id_string(), 0, Some("goal-1"))
                .await
                .unwrap()
                .unwrap();
            let sink = actor.sampling_usage_sink("test".into(), captured_prompt_index);
            let settlement = sink(sampler::AttemptUsage::Known {
                attempt_key: "goal-attempt-2".into(),
                cost_usd_ticks: after_compaction.cost_usd_ticks,
                api_duration_ms: None,
                scope: Some(scope),
                usage: after_compaction.usage.clone().unwrap(),
            });
            tokio::pin!(settlement);
            let command = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::select! {
                    command = goal_rx.recv() => command.expect("Goal usage command"),
                    result = &mut settlement => panic!("settlement completed before root ack: {result:?}"),
                }
            })
            .await
            .expect("Goal usage command timeout");
            match command {
                crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id,
                    respond_to,
                } => {
                    let result = actor.settle_claimed_goal_usage_attempt(&attempt_id).await;
                    respond_to.send(result).expect("Goal settlement acknowledgement");
                }
                _ => panic!("unexpected Goal usage command"),
            }
            settlement.await.expect("attempt usage settlement");
            actor
                .record_response_token_usage(&after_compaction, None, None, Some("goal-1"), true)
                .await
                .unwrap();

            assert_eq!(actor.chat_state_handle.get_projected_tokens().await, 400);
            assert_eq!(actor.goal_tokens_used(), 1_480);
            assert_eq!(actor.goal_tracker.lock().snapshot().unwrap().usage_breakdown.unwrap_or_default(), crate::session::goal_tracker::GoalTokenUsage::new(1_350, 1_000, 130));
            drop(goal_tx_keepalive);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn descendant_model_usage_is_submitted_to_the_root_goal_window() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let (goal_tx, mut goal_rx) = tokio::sync::mpsc::unbounded_channel();
            let goal_tx_keepalive = goal_tx.clone();
            actor.startup_hints.is_subagent = true;
            actor.goal_usage_window = crate::session::actor::goal_support::GoalUsageWindow::new(
                goal_tx,
                Some("goal-1".into()),
            );

            let mut response = response_with_usage(1_080);
            response.usage = Some(TokenUsage {
                prompt_tokens: 1_000,
                completion_tokens: 80,
                total_tokens: 1_080,
                reasoning_tokens: 40,
                cached_prompt_tokens: 700,
                cache_creation_prompt_tokens: 0,
            });
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "descendant").await;
            let epoch = actor
                .goal_usage_window
                .owner_epoch(&actor.session_id_string());
            let scope = actor
                .goal_usage_window
                .begin_model_attempt(&actor.session_id_string(), epoch, Some("goal-1"))
                .await
                .unwrap()
                .unwrap();
            let sink = actor.sampling_usage_sink("test".into(), captured_prompt_index);
            let settlement = sink(sampler::AttemptUsage::Known {
                attempt_key: "descendant-attempt".into(),
                cost_usd_ticks: response.cost_usd_ticks,
                api_duration_ms: None,
                scope: Some(scope),
                usage: response.usage.clone().unwrap(),
            });
            tokio::pin!(settlement);
            let command = tokio::select! {
                command = goal_rx.recv() => command.expect("Goal usage command"),
                result = &mut settlement => panic!("settlement completed before root ack: {result:?}"),
            };
            let respond_to = match command {
                crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id,
                    respond_to,
                } => {
                    assert!(!attempt_id.is_empty());
                    respond_to
                }
                _ => panic!("unexpected Goal usage command"),
            };
            let _ = respond_to.send(Ok(true));
            settlement.await.unwrap();
            assert_eq!(actor.goal_tokens_used(), 0);
            drop(goal_tx_keepalive);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn preserves_projection_when_response_has_no_usage() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(99_999, 256_000, 85, gateway_tx, persistence_tx).await;
            let _sync = actor.chat_state_handle.get_projected_tokens().await;

            actor
                .record_response_token_usage(&response_without_usage(), None, None, None, true)
                .await
                .unwrap();

            assert_eq!(actor.chat_state_handle.get_projected_tokens().await, 99_999);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn final_request_schema_is_visible_to_pre_sampling_pressure() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 100_000, 85, gateway_tx, persistence_tx).await;
            actor
                .chat_state_handle
                .build_request(
                    "test-timeline",
                    vec![ToolSpec {
                        name: "large_schema".into(),
                        description: Some("x".repeat(360_000)),
                        parameters: serde_json::json!({"type": "object"}),
                    }],
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();

            let trigger = actor
                .check_auto_compact_needed()
                .await
                .expect("the final request envelope must cross the 85% threshold");
            assert!(trigger.tokens_used >= 90_000);
            assert_eq!(trigger.source, "pre_sampling");
        })
        .await;
}

/// Wire contract the pager + TUI renderers depend on:
/// `build_session_info().context.used` must reflect the model-reported
/// `total_tokens` after a turn, and `usage_pct` / `free_tokens` /
/// `message_tokens` must be server-computed (not derived by the renderer
/// via subtraction).
#[tokio::test(flavor = "current_thread")]
async fn build_session_info_used_reflects_recorded_response() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            // Push a small non-system fixture (user + assistant + tool
            // result). Without non-system items `message_tokens` would
            // be 0 and the Bug F regression could slip through this
            // assertion. Bytes/4 of these strings is small but >0.
            // The trailing `_and_ack` flushes the actor mailbox so the
            // subsequent query sees the writes.
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("hi there hi there hi there"));
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result(
                    "call-1",
                    "tool result body tool result body",
                ));
            actor
                .chat_state_handle
                .push_user_message_durably(ConversationItem::user("hello hello hello hello"))
                .await
                .unwrap();

            let response = response_with_usage(120_000);
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "session-info").await;
            settle_attempt(
                &actor,
                "session-info-attempt",
                captured_prompt_index,
                "test",
                &response,
            )
            .await;
            actor
                .record_response_token_usage(&response, None, None, None, true)
                .await
                .unwrap();

            let info = actor.build_session_info().await;
            assert_eq!(info.context.used, 120_000);
            assert_eq!(info.context.total, 256_000);
            // Server-computed: renderer no longer derives these.
            assert_eq!(info.context.free_tokens, 256_000 - 120_000);
            // 120_000 / 256_000 = 0.46875 -> 47 after rounding.
            assert_eq!(info.context.usage_pct, 47);
            // Bug F regression guard: bytes/4 of the non-system items
            // must be > 0. Subtraction-based formulas saturated this to
            // zero; the direct sum returns the real estimate.
            assert!(
                info.context.message_tokens > 0,
                "message_tokens should reflect non-system items, got {}",
                info.context.message_tokens,
            );
            assert!(
                info.context.usage_pct > 0,
                "usage_pct should be non-zero when used > 0",
            );
        })
        .await;
}

/// Shell sourcing seam for the fingerprint display gate:
/// `build_session_info` must populate `SessionInfoData.show_model_fingerprint`
/// from the exact catalog identity for the session's current model. Proven on a
/// non-coding model so the catalog flag — not `is_coding_model_slug` — drives the
/// value (the coding-model OR is covered by the `acp_types` / pager tests).
#[tokio::test(flavor = "current_thread")]
async fn build_session_info_sources_show_model_fingerprint_from_catalog() {
    use crate::agent::config::{ModelEntry, ModelInfo};

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            let mut entry = ModelEntry {
                info: ModelInfo::baseline("test"),
                api_key: None,
                env_key: None,
                auth_provider: None,
            };
            entry.info.show_model_fingerprint = false;
            actor
                .models_manager
                .insert_test_entry("test", entry.clone());
            assert!(
                !actor.build_session_info().await.show_model_fingerprint,
                "non-coding slug without the catalog flag must yield false",
            );

            entry.info.show_model_fingerprint = true;
            actor.models_manager.insert_test_entry("test", entry);
            assert!(
                actor.build_session_info().await.show_model_fingerprint,
                "catalog show_model_fingerprint=true must flow to SessionInfoData",
            );
        })
        .await;
}

/// `record_response_token_usage` must also stash the per-turn `TokenUsage`
/// in chat state so the next `PromptResponse._meta` can carry input/output
/// token counts to the bot's diagnostics layer.
#[tokio::test(flavor = "current_thread")]
async fn stashes_per_turn_usage_in_chat_state() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            let captured_prompt_index = actor.chat_state_handle.get_prompt_index().await;
            record_test_prompt(&actor, "metadata").await;

            // Baseline: no stashed usage.
            assert!(
                actor
                    .chat_state_handle
                    .get_last_turn_usage()
                    .await
                    .is_none()
            );

            // Use existing fixture: total=200_000 → prompt=199_950, completion=50.
            let response = response_with_usage(200_000);
            settle_attempt(
                &actor,
                "stash-attempt",
                captured_prompt_index,
                "test",
                &response,
            )
            .await;
            actor
                .record_response_token_usage(&response, None, None, None, true)
                .await
                .unwrap();

            let stashed = actor
                .chat_state_handle
                .get_last_turn_usage()
                .await
                .expect("usage stashed by record_response_token_usage");
            assert_eq!(stashed.prompt_tokens, 199_950);
            assert_eq!(stashed.completion_tokens, 50);
            assert_eq!(stashed.total_tokens, 200_000);
        })
        .await;
}
