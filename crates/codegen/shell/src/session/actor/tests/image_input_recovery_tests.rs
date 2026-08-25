use super::support::*;
use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};
use tokio::sync::mpsc;

fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

async fn actor_with_sampler(
    server: &MockInferenceServer,
    image_description_model: Option<&str>,
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
    if let Some(mut config) = actor.chat_state_handle.get_sampling_config().await {
        config.base_url = server.url();
        config.api_backend = sampling_types::ApiBackend::Messages;
        actor.chat_state_handle.update_sampling_config(config);
    }
    if let Some(auxiliary_slug) = image_description_model {
        let mut info = crate::agent::config::ModelInfo::baseline("vision-model");
        info.base_url = server.url();
        info.api_backend = sampling_types::ApiBackend::Messages;
        actor.models_manager.insert_test_entry(
            auxiliary_slug,
            crate::agent::config::ModelEntry {
                info,
                api_key: Some("test-key".to_owned()),
                env_key: None,
                auth_provider: None,
            },
        );
        *actor.image_description_model.write() = Some(auxiliary_slug.to_owned());
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
    let (sampler_event_tx, sampler_event_rx) = mpsc::unbounded_channel();
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

fn messages_text_turn(text: &str, model: &str) -> ScriptedResponse {
    let events = vec![
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_image_recovery", "type": "message", "role": "assistant",
                "content": [], "model": model, "stop_reason": null,
                "usage": {
                    "input_tokens": 10, "output_tokens": 0,
                    "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
                }
            }
        }),
        json!({
            "type": "content_block_start", "index": 0,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({
            "type": "content_block_delta", "index": 0,
            "delta": {"type": "text_delta", "text": text}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        json!({
            "type": "message_delta", "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 5, "input_tokens": 10}
        }),
        json!({"type": "message_stop"}),
    ];
    ScriptedResponse::sse(
        events
            .into_iter()
            .map(|event| SseEvent::data(event.to_string()))
            .collect(),
    )
}

fn count_wire_images(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(values) => values.iter().map(count_wire_images).sum(),
        serde_json::Value::Object(object) => {
            usize::from(
                object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| matches!(kind, "image" | "image_url" | "input_image")),
            ) + object.values().map(count_wire_images).sum::<usize>()
        }
        _ => 0,
    }
}

#[test]
fn explicit_image_400_retries_once_without_images_and_completes_turn() {
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
                ScriptedResponse::json(
                    400,
                    json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "Failed to deserialize messages[18]: unknown variant `image_url`, expected `text`"
                        }
                    }),
                ),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, None).await;

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                actor.handle_prompt(
                    "image-400-recovery",
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("describe this image")),
                        acp::ContentBlock::Image(test_image_content()),
                    ],
                    tool_types::BehaviorId::Normal,
                    None,
                    None,
                    false,
                    None,
                    None,
                ),
            )
            .await
            .expect("recovery must not loop");
            result.expect("turn must complete after the text-only resubmission");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 2);
            assert!(count_wire_images(requests[0].body.as_ref().unwrap()) > 0);
            assert_eq!(count_wire_images(requests[1].body.as_ref().unwrap()), 0);

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(conversation.iter().any(|item| {
                matches!(item, ConversationItem::User(user) if user.content.iter().any(|part| matches!(part, ContentPart::Image { .. })))
            }));
            assert!(actor.unsupported_current_model_for_images().await.is_some());

            let mut image_projected_count = 0;
            let mut image_projected_notes = Vec::new();
            let mut terminal_retry_failure = false;
            while let Ok(message) = gateway_rx.try_recv() {
                let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                    continue;
                };
                if args.request.method.as_ref() != "grow/session_notification" {
                    continue;
                }
                let notification: crate::extensions::notification::SessionNotification =
                    serde_json::from_str(args.request.params.get()).unwrap();
                match notification.update {
                    GrowSessionUpdate::ImageProjected { notes } => {
                        image_projected_count += 1;
                        image_projected_notes.extend(notes);
                    }
                    GrowSessionUpdate::RetryState(
                        crate::extensions::notification::RetryState::Failed { .. },
                    ) => terminal_retry_failure = true,
                    _ => {}
                }
            }
            assert_eq!(image_projected_count, 1);
            assert!(
                image_projected_notes
                    .iter()
                    .any(|note| note.contains("当前模型投影省略了 1 张图片"))
            );
            assert!(!terminal_retry_failure);
        }));
    });
}

#[test]
fn explicit_image_400_uses_auxiliary_description_then_retries_without_images() {
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
                ScriptedResponse::json(
                    400,
                    json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "unknown variant `image_url`, expected `text`"
                        }
                    }),
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("A red build-error dialog with code E42.", "vision-model"),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Recovered from the image context.", "test-model"),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, Some("vision")).await;

            actor
                .handle_prompt(
                    "image-400-aux-recovery",
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("diagnose this screenshot")),
                        acp::ContentBlock::Image(test_image_content()),
                    ],
                    tool_types::BehaviorId::Normal,
                    None,
                    None,
                    false,
                    None,
                    None,
                )
                .await
                .expect("auxiliary conversion recovery must complete the turn");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 3);
            assert!(count_wire_images(requests[0].body.as_ref().unwrap()) > 0);
            assert!(count_wire_images(requests[1].body.as_ref().unwrap()) > 0);
            assert_eq!(requests[1].body.as_ref().unwrap()["model"], "vision-model");
            assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
            assert!(requests[2]
                .body
                .as_ref()
                .unwrap()
                .to_string()
                .contains("code E42"));

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(conversation.iter().any(|item| {
                matches!(item, ConversationItem::User(user) if user.content.iter().any(|part| matches!(part, ContentPart::Image { .. })))
            }));

            let mut notes = Vec::new();
            let mut terminal_retry_failure = false;
            while let Ok(message) = gateway_rx.try_recv() {
                let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                    continue;
                };
                if args.request.method.as_ref() != "grow/session_notification" {
                    continue;
                }
                let notification: crate::extensions::notification::SessionNotification =
                    serde_json::from_str(args.request.params.get()).unwrap();
                match notification.update {
                    GrowSessionUpdate::ImageProjected { notes: current } => notes.extend(current),
                    GrowSessionUpdate::RetryState(
                        crate::extensions::notification::RetryState::Failed { .. },
                    ) => terminal_retry_failure = true,
                    _ => {}
                }
            }
            assert!(notes.iter().any(|note| note.contains("用辅助描述替代 1 张图片")));
            assert!(!terminal_retry_failure);
        }));
    });
}

#[test]
fn auxiliary_image_400_is_cached_for_aux_runtime_while_original_images_remain() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            for message in [
                "unknown variant `image_url`, expected `text`",
                "input_image is not supported by this model",
            ] {
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
                messages_text_turn("Continued without visual context.", "test-model"),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, Some("vision")).await;

            actor
                .handle_prompt(
                    "image-400-aux-400",
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("inspect")),
                        acp::ContentBlock::Image(test_image_content()),
                    ],
                    tool_types::BehaviorId::Normal,
                    None,
                    None,
                    false,
                    None,
                    None,
                )
                .await
                .expect("auxiliary rejection must degrade to removal and continue");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 3);
            assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(
                    &actor.chat_state_handle.get_conversation().await
                )
                .len(),
                1
            );

            let mut auxiliary_config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            auxiliary_config.model = "vision-model".to_owned();
            actor
                .chat_state_handle
                .update_sampling_config(auxiliary_config);
            assert_eq!(
                actor
                    .unsupported_current_model_for_images()
                    .await
                    .as_deref(),
                Some("vision-model")
            );

            let mut notes = Vec::new();
            while let Ok(message) = gateway_rx.try_recv() {
                let acp_transport::AcpClientMessage::ExtNotification(args) = message else {
                    continue;
                };
                if args.request.method.as_ref() != "grow/session_notification" {
                    continue;
                }
                let notification: crate::extensions::notification::SessionNotification =
                    serde_json::from_str(args.request.params.get()).unwrap();
                if let GrowSessionUpdate::ImageProjected { notes: current } = notification.update {
                    notes.extend(current);
                }
            }
            assert!(
                notes
                    .iter()
                    .any(|note| note.contains("当前模型投影省略了 1 张图片"))
            );
        }));
    });
}
