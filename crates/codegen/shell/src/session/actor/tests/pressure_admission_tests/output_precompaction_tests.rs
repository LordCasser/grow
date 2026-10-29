use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};

const END_TURN: &str = "end_turn";

fn messages_turn(text: &str) -> ScriptedResponse {
    let events = [
        json!({"type":"message_start","message":{"id":"msg_pressure","type":"message","role":"assistant","content":[],"model":"test-model","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}).to_string(),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}).to_string(),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}).to_string(),
        json!({"type":"content_block_stop","index":0}).to_string(),
        json!({"type":"message_delta","delta":{"stop_reason":END_TURN},"usage":{"input_tokens":10,"output_tokens":5}}).to_string(),
        json!({"type":"message_stop"}).to_string(),
    ];
    ScriptedResponse::sse(events.into_iter().map(SseEvent::data).collect())
}

async fn set_output_limit(actor: &SessionActor, limit: u32) {
    let mut route = actor
        .chat_state_handle
        .get_sampling_config()
        .await
        .expect("sampling route exists");
    route.output_limit = Some(limit);
    actor.chat_state_handle.update_sampling_config(route);
}

async fn seed_closed_compaction_range(actor: &SessionActor, tail_chars: usize) {
    replace_test_surface(
        &actor.chat_state_handle,
        vec![ConversationItem::system("test system prompt")],
    )
    .await;
    let mut old = ConversationItem::user("old closed turn");
    old.set_prompt_index(0);
    actor.chat_state_handle.push_user_message(old);
    actor
        .chat_state_handle
        .push_assistant_response(ConversationItem::assistant("x".repeat(24_000)));
    let mut recent = ConversationItem::user("recent retained turn");
    recent.set_prompt_index(1);
    actor.chat_state_handle.push_user_message(recent);
    actor
        .chat_state_handle
        .push_assistant_response(ConversationItem::assistant("y".repeat(tail_chars)));
    let _ = actor.chat_state_handle.get_conversation_len().await;
}

fn notification_kinds(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> Vec<String> {
    let mut kinds = Vec::new();
    while let Ok(message) = rx.try_recv() {
        let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
            continue;
        };
        if args.request.method.as_ref() != "grow/session_notification" {
            continue;
        }
        let Ok(params) = serde_json::from_str::<serde_json::Value>(args.request.params.get())
        else {
            continue;
        };
        if let Some(kind) = params
            .get("update")
            .and_then(|update| update.get("sessionUpdate"))
            .and_then(serde_json::Value::as_str)
        {
            kinds.push(kind.to_owned());
        }
    }
    kinds
}

#[test]
fn explicit_output_overflow_compacts_once_then_dispatches_foreground_request() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!("A durable summary of the completed work: {}", "The verified decision and supporting evidence are retained. ".repeat(12));
            server.enqueue_response("/v1/messages", messages_turn(&summary));
            server.enqueue_response("/v1/messages", messages_turn("foreground answer"));
            let (actor, mut notifications) = actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor.compaction.pre_prune.set(false);
            seed_closed_compaction_range(&actor, 88_000).await;
            set_output_limit(&actor, 25_000).await;
            actor.chat_state_handle.record_provider_context_anchor(79_000);

            run_user_turn(&actor, "output-overflow-compacts").await.expect("summary replacement should leave room for the foreground output limit");

            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            let summary_event = timeline.iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Compaction(chat_state::CompactionEvent::Summary { target, .. }) => Some(target),
                _ => None,
            }).expect("one durable summary replacement");
            assert!(!summary_event.shadowed.is_empty());
            let surface = actor.chat_state_handle.get_conversation().await;
            assert!(surface.iter().any(|item| item.text_content().contains("durable summary")), "replacement summary remains on the current Surface");
            assert!(!surface.iter().any(|item| matches!(item, ConversationItem::Assistant(assistant) if assistant.content.contains(&"x".repeat(1_000)))), "the closed source turn is shadowed by the replacement");
            assert_eq!(server.messages_request_count(), 2, "one summary and one foreground request");
            let kinds = notification_kinds(&mut notifications);
            assert_eq!(kinds.iter().filter(|kind| kind.as_str() == "auto_compact_completed").count(), 1, "one Completed update: {kinds:?}");
        }));
    });
}

#[test]
fn explicit_output_still_overflowing_after_summary_fails_without_retry_loop() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!(
                "A durable summary of the completed work: {}",
                "The verified decision and supporting evidence are retained. ".repeat(12)
            );
            server.enqueue_response("/v1/messages", messages_turn(&summary));
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor.compaction.pre_prune.set(false);
            seed_closed_compaction_range(&actor, 88_000).await;
            set_output_limit(&actor, 40_000).await;
            actor
                .chat_state_handle
                .record_provider_context_anchor(79_000);

            run_user_turn(&actor, "output-overflow-remains")
                .await
                .expect_err("retained input plus output limit remains above the window");

            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                timeline
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Compaction(
                            chat_state::CompactionEvent::Summary { .. }
                        )
                    ))
                    .count(),
                1,
                "the completed summary remains committed"
            );
            assert_eq!(
                server.messages_request_count(),
                1,
                "no ordinary request and no loop"
            );
            let kinds = notification_kinds(&mut notifications);
            assert_eq!(
                kinds
                    .iter()
                    .filter(|kind| kind.as_str() == "auto_compact_completed")
                    .count(),
                1,
                "one Completed update: {kinds:?}"
            );
        }));
    });
}

#[test]
fn reserved_output_pressure_pre_prunes_closed_tool_result_before_summary() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", messages_turn("foreground answer"));
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            set_output_limit(&actor, 27_000).await;
            replace_test_surface(
                &actor.chat_state_handle,
                vec![ConversationItem::system("test system prompt")],
            )
            .await;
            let mut old = ConversationItem::user("old closed tool turn");
            old.set_prompt_index(0);
            actor.chat_state_handle.push_user_message(old);
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "prunable-call".into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                }]),
            );
            let original_tool_text = "tool payload ".repeat(20_000);
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result(
                    "prunable-call",
                    original_tool_text.clone(),
                ));
            let mut recent = ConversationItem::user("recent retained turn");
            recent.set_prompt_index(1);
            actor.chat_state_handle.push_user_message(recent);
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("y".repeat(88_000)));
            let _ = actor.chat_state_handle.get_conversation_len().await;
            actor
                .chat_state_handle
                .build_request("reserve-seed", vec![], None, None, None)
                .await
                .unwrap();
            actor
                .chat_state_handle
                .record_provider_context_anchor(74_000);
            let projected_input = actor.chat_state_handle.get_projected_tokens().await;
            assert!(
                projected_input < 75_000,
                "fixture is below the input-only compaction threshold: {projected_input}"
            );
            assert!(
                projected_input.saturating_add(27_000) > 100_000,
                "only the explicit output reserve crosses the context window: {projected_input}"
            );

            run_user_turn(&actor, "output-overflow-pre-prunes")
                .await
                .expect("pre-pruning the closed tool result should fit input plus output");

            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(
                !timeline.iter().any(|event| matches!(
                    event.kind,
                    chat_state::TimelineEventKind::Compaction(
                        chat_state::CompactionEvent::Summary { .. }
                    )
                )),
                "pre-prune resolves reserved-output pressure without a summary"
            );
            let surface = actor.chat_state_handle.get_conversation().await;
            let tool_result = surface.iter().find_map(|item| match item {
                ConversationItem::ToolResult(result) => Some(result.content.as_ref()),
                _ => None,
            });
            assert!(
                tool_result.is_some_and(|text| text.len() < original_tool_text.len()),
                "the eligible closed result is durably pruned"
            );
            assert_eq!(
                server.messages_request_count(),
                1,
                "only the foreground request runs"
            );
            let kinds = notification_kinds(&mut notifications);
            assert_eq!(
                kinds
                    .iter()
                    .filter(|kind| kind.as_str() == "auto_compact_completed")
                    .count(),
                1,
                "one Completed update: {kinds:?}"
            );
        }));
    });
}

#[test]
fn auth_suppression_blocks_automatic_recovery_for_reserved_output_pressure() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor.compaction.pre_prune.set(false);
            seed_closed_compaction_range(&actor, 88_000).await;
            set_output_limit(&actor, 25_000).await;
            actor
                .chat_state_handle
                .record_provider_context_anchor(79_000);
            actor.compaction.auto_compact_suppressed.store(
                crate::session::compaction_config::SUPPRESS_AUTH,
                std::sync::atomic::Ordering::Relaxed,
            );

            run_user_turn(&actor, "auth-suppressed-output-overflow")
                .await
                .expect_err("suppressed compaction cannot resolve the combined budget");

            assert_eq!(
                server.messages_request_count(),
                0,
                "over-window request is not dispatched"
            );
            assert!(
                !notification_kinds(&mut notifications)
                    .iter()
                    .any(|kind| kind == "auto_compact_started"),
                "auth suppression blocks automatic recovery"
            );
        }));
    });
}
