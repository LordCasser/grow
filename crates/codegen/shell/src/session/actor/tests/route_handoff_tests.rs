//! Cross route handoff at an admitted Step boundary.

use super::*;
use serde_json::json;
use test_support::{
    InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer, ScriptedResponse, SseEvent,
};
use tools::implementations::grow_build::{read_file::ReadFileTool, todo::TodoWriteTool};
use tools::registry::types::ToolConfig;

const HANDOFF_MARKER: &str = "The model route has changed";
const TODO_ARGS: &str = r#"{"actions":[],"todos":[{"id":"handoff-todo","content":"finish original task","status":"completed"}]}"#;

#[path = "route_handoff_tests/boundary_tests.rs"]
mod boundary_tests;

#[test]
fn route_change_hands_off_tool_history_once_across_messages_to_responses() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let mut first = server.expect_response_blocked(
                "first Messages tool call",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_tool_call("first-call", "old_native_reasoning"),
            );
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_reasoning_then_tool_call_events(
                    "new_native_reasoning",
                    "second-call",
                    "todo_write",
                    TODO_ARGS,
                    "selected-model",
                )),
            );
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "original task complete",
                    "selected-model",
                )),
            );

            let (actor, _notifications) = actor_with_sampler_cw(
                &server,
                sampling_types::ApiBackend::Messages,
                100_000,
            )
            .await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![ToolConfig::for_tool::<TodoWriteTool>(), ToolConfig::for_tool::<ReadFileTool>()]).await;

            let turn = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { run_user_turn(&actor, "route-handoff").await }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), first.wait_blocked())
                .await
                .expect("first tool response is held at the provider");

            let mut selected = actor.reconstruct_full_config().await;
            selected.model = "selected-model".into();
            selected.base_url = server.url();
            selected.api_backend = sampling_types::ApiBackend::Responses;
            selected.api_key = Some("test-key".into());
            selected.context_window = 100_000;
            let model_id = crate::agent::models::ModelId::new("test/selected-model");
            let catalog = SessionActor::published_catalog_for_test(
                model_id.clone(),
                selected,
                None,
                std::time::Duration::from_secs(60),
                0,
                85,
            );
            let route = catalog.resolve_session_route(&model_id, None).unwrap();
            let (ack, mut applied) = tokio::sync::oneshot::channel();
            actor.admit_session_model_selection(route, Some(catalog), None, ack).await;
            assert!(matches!(
                applied.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ), "selection remains queued until the blocked Step closes");

            first.release();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                applied.await.unwrap().unwrap();
                turn.await.unwrap().unwrap()
            })
            .await
            .expect("the original turn completes on the selected route");

            let requests = server.request_bodies();
            assert_eq!(requests.len(), 3, "no diagnostic or duplicate sample is issued");
            assert_eq!(requests[0]["model"], "test");
            assert_eq!(requests[1]["model"], "selected-model");
            let second_wire = requests[1].to_string();
            assert!(second_wire.contains("first-call"), "the original call is retained");
            assert!(second_wire.contains("handoff-todo"), "the original tool result is retained");
            assert!(second_wire.contains(HANDOFF_MARKER), "the selected route receives a user handoff");
            assert!(!second_wire.contains("old_native_reasoning"), "old native reasoning is not replayed");
            let third_wire = requests[2].to_string();
            assert_eq!(third_wire.matches(HANDOFF_MARKER).count(), 1, "later Steps reuse the one persisted boundary");
            assert!(third_wire.contains("first-call"));
            assert!(third_wire.contains("second-call"));
            assert_eq!(third_wire.matches("new_native_reasoning").count(), 1, "new-route native reasoning survives the following Step exactly once");
            assert!(!third_wire.contains("old_native_reasoning"));

            let surface = actor.chat_state_handle.get_conversation().await;
            let handoff = surface.iter().find_map(|item| match item {
                ConversationItem::User(user) if item.text_content().contains(HANDOFF_MARKER) => Some(user),
                _ => None,
            }).unwrap();
            assert!(handoff.permission_evidence.is_none());
            assert!(handoff.prompt_index.is_none());
            let handoffs = surface.iter().filter(|item| matches!(item,
                ConversationItem::User(user) if user.synthetic_reason == Some(sampling_types::SyntheticReason::AutoContinue)
                    && item.text_content().contains(HANDOFF_MARKER)
            )).count();
            assert_eq!(handoffs, 1, "the route handoff is durably appended once");
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(events.iter().filter(|event| matches!(event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
            )).count(), 1, "handoff does not create a new turn");
            let input = requests[1]["input"].as_array().unwrap();
            let call = input.iter().position(|item| item["type"] == "function_call" && item["call_id"] == "first-call").unwrap();
            let result = input.iter().position(|item| item["type"] == "function_call_output" && item["call_id"] == "first-call").unwrap();
            let boundary = input.iter().position(|item| item["role"] == "user" && item.to_string().contains(HANDOFF_MARKER)).unwrap();
            assert!(call < result && result < boundary, "handoff follows the complete tool exchange");
            assert_eq!(events.iter().filter(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed { call_id, .. })
                    if call_id == "first-call" || call_id == "second-call"
            )).count(), 2, "each tool executes once; retained history is never re-executed");
        }));
    });
}

#[test]
fn effort_only_control_change_does_not_add_route_handoff() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let mut first = server.expect_response_blocked(
                "first tool call",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_tool_call("effort-call", "effort_reasoning"),
            );
            server.enqueue_response("/v1/messages", messages_turn(&[("done", END_TURN)], END_TURN));
            let (actor, _) = actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![ToolConfig::for_tool::<TodoWriteTool>(), ToolConfig::for_tool::<ReadFileTool>()]).await;
            actor.model_route.replace(
                actor.model_route.snapshot().model_id,
                actor.reconstruct_full_config().await,
            );
            let turn = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { run_user_turn(&actor, "effort-only-route").await }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), first.wait_blocked()).await.unwrap();

            let before = actor.model_route.snapshot();
            let mut selected = before.sampling_config.clone();
            selected.reasoning_effort = Some(sampling_types::ReasoningEffort::High);
            let catalog = SessionActor::published_catalog_for_test(
                before.model_id.clone(), selected, None, std::time::Duration::from_secs(60), 0, 85,
            );
            let route = catalog.resolve_session_route(&before.model_id, None).unwrap();
            let (ack, applied) = tokio::sync::oneshot::channel();
            actor.admit_session_model_selection(route, Some(catalog), None, ack).await;
            first.release();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                applied.await.unwrap().unwrap();
                turn.await.unwrap().unwrap()
            }).await.expect("turn completes after effort-only update");

            let surface = actor.chat_state_handle.get_conversation().await;
            assert!(!surface.iter().any(|item| matches!(item,
                ConversationItem::User(user) if user.synthetic_reason == Some(sampling_types::SyntheticReason::AutoContinue)
                    && item.text_content().contains(HANDOFF_MARKER)
            )), "same route sampling changes keep native continuation behavior");
            let second_wire = server.request_bodies()[1].to_string();
            assert!(!second_wire.contains(HANDOFF_MARKER));
            assert!(second_wire.contains("effort_reasoning"));
        }));
    });
}

#[test]
fn route_change_after_terminal_response_does_not_append_handoff_or_sample_again() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let mut first = server.expect_response_blocked(
                "terminal response",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_turn(&[("task is complete", END_TURN)], END_TURN),
            );
            let (actor, _) = actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![ToolConfig::for_tool::<TodoWriteTool>(), ToolConfig::for_tool::<ReadFileTool>()]).await;
            let turn = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { run_user_turn(&actor, "route-change-terminal").await }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), first.wait_blocked()).await.unwrap();

            let mut selected = actor.reconstruct_full_config().await;
            selected.model = "after-terminal".into();
            selected.base_url = server.url();
            selected.api_backend = sampling_types::ApiBackend::Responses;
            selected.api_key = Some("test-key".into());
            let model_id = crate::agent::models::ModelId::new("test/after-terminal");
            let catalog = SessionActor::published_catalog_for_test(
                model_id.clone(), selected, None, std::time::Duration::from_secs(60), 0, 85,
            );
            let route = catalog.resolve_session_route(&model_id, None).unwrap();
            let (ack, applied) = tokio::sync::oneshot::channel();
            actor.admit_session_model_selection(route, Some(catalog), None, ack).await;
            first.release();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                turn.await.unwrap().unwrap();
                actor.apply_pending_step_controls_if_idle().await;
                applied.await.unwrap().unwrap()
            }).await.expect("terminal response closes without a successor Step");

            assert_eq!(server.request_bodies().len(), 1);
            assert!(!actor.chat_state_handle.get_conversation().await.iter().any(|item| matches!(item,
                ConversationItem::User(user) if user.synthetic_reason == Some(sampling_types::SyntheticReason::AutoContinue)
                    && item.text_content().contains(HANDOFF_MARKER)
            )));
        }));
    });
}

fn messages_tool_call(call_id: &str, reasoning_marker: &str) -> ScriptedResponse {
    let args: serde_json::Value = serde_json::from_str(TODO_ARGS).unwrap();
    ScriptedResponse::sse(vec![
        SseEvent::data(json!({
            "type":"message_start",
            "message":{"id":"handoff-tool","type":"message","role":"assistant","content":[],"model":"test-model","usage":{"input_tokens":10,"output_tokens":0}}
        }).to_string()),
        SseEvent::data(json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}).to_string()),
        SseEvent::data(json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":reasoning_marker}}).to_string()),
        SseEvent::data(json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"fixture-signature"}}).to_string()),
        SseEvent::data(json!({"type":"content_block_stop","index":0}).to_string()),
        SseEvent::data(json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":call_id,"name":"todo_write","input":{}}}).to_string()),
        SseEvent::data(json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":args.to_string()}}).to_string()),
        SseEvent::data(json!({"type":"content_block_stop","index":1}).to_string()),
        SseEvent::data(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5,"input_tokens":10}}).to_string()),
        SseEvent::data(json!({"type":"message_stop"}).to_string()),
    ])
}
