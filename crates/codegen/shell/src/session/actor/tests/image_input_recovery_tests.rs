use super::support::*;
use super::*;
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};
use tokio::sync::mpsc;

fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

/// Sampler-backed actor with a controllable chat-state Timeline persistence.
/// The returned gate observes acknowledged/failed image projections; callers
/// that do not care about durability keep the default behaviour (immediate
/// acknowledgement, nothing stored).
async fn actor_with_sampler_and_persistence(
    server: &MockInferenceServer,
    image_description_model: Option<&str>,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    ImageProjectionPersistence,
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
    let (mut actor, image_projection_persistence) =
        create_test_actor_with_image_projection_persistence(
            0,
            256_000,
            85,
            gateway_tx,
            persistence_tx,
        )
        .await;
    if let Some(mut config) = actor.chat_state_handle.get_sampling_config().await {
        config.base_url = server.url();
        config.api_backend = sampling_types::ApiBackend::Messages;
        actor.chat_state_handle.replace_sampling_route(config);
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
    let (goal_usage_tx, mut goal_usage_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.goal_usage_window =
        crate::session::actor::goal_support::GoalUsageWindow::new(goal_usage_tx.clone(), None);
    let actor = std::sync::Arc::new(actor);
    let usage_actor = actor.clone();
    tokio::task::spawn_local(async move {
        let _keepalive = goal_usage_tx;
        while let Some(command) = goal_usage_rx.recv().await {
            let crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                attempt_id,
                respond_to,
            } = command
            else {
                panic!("unexpected Goal accounting command in image recovery test");
            };
            let result = usage_actor
                .settle_claimed_goal_usage_attempt(&attempt_id)
                .await;
            let _ = respond_to.send(result);
        }
    });
    let drainer = actor.clone();
    tokio::task::spawn_local(async move {
        let mut sampler_event_rx = sampler_event_rx;
        while let Some(event) = sampler_event_rx.recv().await {
            drainer.handle_sampling_event(event).await;
        }
    });
    (actor, gateway_rx, image_projection_persistence)
}

async fn actor_with_sampler(
    server: &MockInferenceServer,
    image_description_model: Option<&str>,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (actor, gateway_rx, _) =
        actor_with_sampler_and_persistence(server, image_description_model).await;
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

fn multimodal_capability_rejection() -> ScriptedResponse {
    ScriptedResponse::json(
        400,
        json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "InvalidParameter: test-model is not a multimodal model"
            }
        }),
    )
}

/// Task 4.2 acceptance: the exact GLM-style capability 400 is classified,
/// the unresolved image is durably removed from the current Surface, and the
/// resubmitted request carries the canonical text with zero images. The raw
/// payload stays in the immutable Timeline evidence, and the following
/// text-only turn never repeats the capability rejection.
/// Count image groups carried verbatim by the durable event ledger. The
/// canonical user item is sealed by `Messages`, `Input::Consumed` or
/// `Notification::Consumed` depending on how the turn was admitted, and every
/// one of them is immutable evidence that a projection must not rewrite.
fn sealed_image_groups(events: &[chat_state::TimelineEvent]) -> usize {
    events
        .iter()
        .flat_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Messages(messages) => messages.items.as_slice(),
            chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                item, ..
            }) => std::slice::from_ref(item),
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Consumed {
                    input: Some(item), ..
                },
            ) => std::slice::from_ref(item),
            _ => &[],
        })
        .map(|item| {
            sampling_types::conversation::conversation_image_groups(std::slice::from_ref(item))
                .len()
        })
        .sum()
}

#[test]
fn explicit_multimodal_400_removes_images_and_resubmits_text_only() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Continued without the removed image.", "test-model"),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Plain text follow-up.", "test-model"),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, None).await;
            install_test_foreground(&actor, "image-400-removal").await;

            actor
                .handle_prompt(
                    "image-400-removal",
                    crate::session::actor::tests::support::admit_test_human_input(
                        &actor,
                        "image-400-removal",
                    )
                    .await,
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
                )
                .await
                .expect("a durable removal projection must recover the turn");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 2, "initial image request plus one resubmit");
            assert!(count_wire_images(requests[0].body.as_ref().unwrap()) > 0);
            assert_eq!(count_wire_images(requests[1].body.as_ref().unwrap()), 0);
            assert!(
                requests[1]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains(sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT)
            );

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                sampling_types::conversation::conversation_image_groups(&conversation).is_empty()
            );
            let user = conversation
                .iter()
                .find_map(|item| match item {
                    ConversationItem::User(user) => Some(user),
                    _ => None,
                })
                .expect("the user image turn must stay on the Surface");
            assert!(user.content.iter().any(|part| matches!(
                part,
                sampling_types::ContentPart::Text { text }
                    if text.as_ref() == sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
            )));
            assert!(
                actor
                    .unsupported_current_model_for_images()
                    .await
                    .is_some(),
                "the capability pair must be marked text-only"
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|event| matches!(
                event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )));
            assert_eq!(
                sealed_image_groups(&events),
                1,
                "the original payload must remain immutable Timeline evidence"
            );
            chat_state::Timeline::from_events(events)
                .expect("the repaired Timeline must stay replayable");

            let drained = drain_notifications(&mut gateway_rx);
            assert!(drained.retry_failures.is_empty());
            assert_eq!(drained.image_projected_updates, 1);
            assert_eq!(
                drained.notes,
                vec![
                    "当前模型不支持多模态，1 张图片无法生成文字描述，已从当前会话中删除；原图仍保留在会话历史记录中。"
                        .to_owned()
                ]
            );

            release_settled_foreground(&actor).await;
            install_test_foreground(&actor, "image-400-removal-follow-up").await;
            actor
                .handle_prompt(
                    "image-400-removal-follow-up",
                    crate::session::actor::tests::support::admit_test_human_input(
                        &actor,
                        "image-400-removal-follow-up",
                    )
                    .await,
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![acp::ContentBlock::Text(acp::TextContent::new("plain text"))],
                    tool_types::BehaviorId::Normal,
                    None,
                    None,
                    false,
                    None,
                    None,
                )
                .await
                .expect("a text-only turn on the repaired Surface must succeed");
            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 3, "the follow-up turn must not retry images");
            assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
        }));
    });
}

/// Projection-related notifications the client received during a turn.
#[derive(Default)]
struct DrainedNotifications {
    image_projected_updates: usize,
    image_compressed_updates: usize,
    compressed_messages: Vec<String>,
    image_dropped_updates: usize,
    dropped_notes: Vec<String>,
    notes: Vec<String>,
    retry_failures: Vec<String>,
}

/// Direct `handle_prompt` fixtures stop at `Settling`; production's completion
/// mailbox releases this owner before admitting the next prompt.
async fn release_settled_foreground(actor: &SessionActor) {
    let mut state = actor.state.lock().await;
    assert!(
        matches!(state.foreground, ForegroundState::Settling { .. }),
        "a completed direct turn must settle before the next prompt"
    );
    state.foreground = ForegroundState::Idle;
}

fn drain_notifications(
    gateway_rx: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> DrainedNotifications {
    let mut drained = DrainedNotifications::default();
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
            GrowSessionUpdate::ImageCompressed { message, .. } => {
                drained.image_compressed_updates += 1;
                drained.compressed_messages.push(message);
            }
            GrowSessionUpdate::ImageDropped { notes } => {
                drained.image_dropped_updates += 1;
                drained.dropped_notes.extend(notes);
            }
            GrowSessionUpdate::ImageProjected { notes } => {
                drained.image_projected_updates += 1;
                drained.notes.extend(notes);
            }
            GrowSessionUpdate::RetryState(
                crate::extensions::notification::RetryState::Failed { message, .. },
            ) => drained.retry_failures.push(message),
            _ => {}
        }
    }
    drained
}

fn inline_data_uri(image: &acp::ImageContent) -> String {
    format!("data:{};base64,{}", image.mime_type, image.data)
}

/// A large, flat PNG keeps this fixture small while exceeding the normalizer's
/// side limit, so it deterministically exercises the re-encode path.
fn inline_compressed_image() -> String {
    use base64::Engine as _;
    use image::{ImageBuffer, Rgb};

    let image: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(3000, 2000, Rgb([128, 64, 32]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode oversized inline PNG");
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Keep the payload above the extractor threshold while remaining under all
/// image normalization limits.
fn inline_healthy_image() -> String {
    use base64::Engine as _;
    use image::{ImageBuffer, Rgb};

    let image: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(64, 64, |x, y| {
        Rgb([
            (x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17))) as u8,
            (x.wrapping_mul(13).wrapping_add(y.wrapping_mul(29))) as u8,
            (x.wrapping_mul(7).wrapping_add(y.wrapping_mul(43))) as u8,
        ])
    });
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode healthy inline PNG");
    let content = acp::ImageContent::new(
        base64::engine::general_purpose::STANDARD.encode(bytes),
        "image/png",
    );
    inline_data_uri(&content)
}

fn inline_dropped_image() -> String {
    // The extractor requires at least 1024 base64 characters. This decodes as
    // bytes but is not an image, making normalization drop it with one stable
    // validation reason.
    format!("data:image/png;base64,{}", "A".repeat(1024))
}

async fn admit_inline_query(actor: &std::sync::Arc<SessionActor>, prompt_id: &str, text: String) {
    install_test_foreground(actor, prompt_id).await;
    actor
        .handle_prompt(
            prompt_id,
            crate::session::actor::tests::support::admit_test_human_input(actor, prompt_id).await,
            crate::session::PromptOrigin::User,
            Vec::new(),
            crate::session::TurnKind::User,
            vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
            tool_types::BehaviorId::Normal,
            None,
            None,
            false,
            None,
            None,
        )
        .await
        .expect("inline image prompt should complete");
}

#[test]
fn inline_image_normalization_notices_cover_dropped_compressed_and_mixed_input() {
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
                messages_text_turn("inline image result", "test-model"),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, None).await;
            let dropped = inline_dropped_image();
            let compressed = inline_compressed_image();
            let healthy = inline_healthy_image();
            let query = format!(
                "before {dropped} between {dropped} compressed {compressed} healthy {healthy} after",
            );
            admit_inline_query(&actor, "inline-image-mixed", query).await;

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 1);
            let body = requests[0].body.as_ref().unwrap().to_string();
            assert_eq!(count_wire_images(requests[0].body.as_ref().unwrap()), 2);
            assert_eq!(body.matches("<image_dropped_notice>").count(), 1);
            assert_eq!(body.matches("<image_compression_notice>").count(), 1);
            assert_eq!(body.matches("Images 1 and 2 were dropped").count(), 1);
            assert_eq!(body.matches("Image 3 ").count(), 1);

            let conversation = actor.chat_state_handle.get_conversation().await;
            let groups = sampling_types::conversation::conversation_image_groups(&conversation);
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].image_count(), 2);
            assert_eq!(groups[0].image_urls[1].as_ref(), healthy.as_str());
            let ConversationItem::User(user) = &conversation[groups[0].item_index] else {
                panic!("inline images must belong to the admitted user message");
            };
            let placeholder = "[image content will be provided separately]";
            assert_eq!(
                user.permission_evidence,
                Some(sampling_types::PermissionEvidence::direct_user(format!(
                    "<user_query>\nbefore {placeholder} between {placeholder} compressed {placeholder} healthy {placeholder} after\n</user_query>",
                ))),
                "normalization reminders must not become user permission evidence",
            );
            for uri in [&dropped, &compressed, &healthy] {
                let payload = uri.split_once(',').unwrap().1;
                assert!(!groups[0].source_text.contains(payload));
            }
            assert_eq!(
                groups[0]
                    .source_text
                    .matches("<image_dropped_notice>")
                    .count(),
                1
            );
            assert_eq!(
                groups[0]
                    .source_text
                    .matches("<image_compression_notice>")
                    .count(),
                1
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let replay = chat_state::Timeline::from_events(events).expect("Timeline replay");
            let replay_groups =
                sampling_types::conversation::conversation_image_groups(replay.surface());
            assert_eq!(replay_groups.len(), 1);
            assert_eq!(replay_groups[0].image_count(), 2);
            assert_eq!(replay_groups[0].image_urls[1].as_ref(), healthy.as_str());
            assert_eq!(replay_groups[0].image_urls, groups[0].image_urls);
            assert_eq!(replay_groups[0].source_text, groups[0].source_text);
            let ConversationItem::User(replayed_user) =
                &replay.surface()[replay_groups[0].item_index]
            else {
                panic!("replayed images must retain the user-message owner");
            };
            assert_eq!(replayed_user.permission_evidence, user.permission_evidence);
            assert_eq!(
                replay_groups[0]
                    .source_text
                    .matches("<image_dropped_notice>")
                    .count(),
                1
            );
            assert_eq!(
                replay_groups[0]
                    .source_text
                    .matches("<image_compression_notice>")
                    .count(),
                1
            );
            let drained = drain_notifications(&mut gateway_rx);
            assert_eq!(drained.image_dropped_updates, 1);
            assert_eq!(drained.dropped_notes.len(), 1);
            assert!(drained.dropped_notes[0].contains("Images 1 and 2 were dropped"));
            assert_eq!(drained.image_compressed_updates, 1);
            assert_eq!(drained.compressed_messages.len(), 1);
            assert!(drained.compressed_messages[0].contains("Image 3"));
        }));
    });
}

#[test]
fn inline_healthy_image_has_no_normalization_notice() {
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
                messages_text_turn("healthy inline image result", "test-model"),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, None).await;
            admit_inline_query(
                &actor,
                "inline-image-healthy",
                format!("keep this image {}", inline_healthy_image()),
            )
            .await;

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 1);
            let body = requests[0].body.as_ref().unwrap().to_string();
            assert_eq!(count_wire_images(requests[0].body.as_ref().unwrap()), 1);
            assert!(!body.contains("<image_dropped_notice>"));
            assert!(!body.contains("<image_compression_notice>"));
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                1
            );
            let drained = drain_notifications(&mut gateway_rx);
            assert_eq!(drained.image_dropped_updates, 0);
            assert_eq!(drained.image_compressed_updates, 0);
        }));
    });
}

#[test]
fn active_goal_image_400_uses_auxiliary_description_then_retries_without_images() {
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
            install_test_foreground(&actor, "image-400-aux-recovery").await;
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-image-recovery".into(),
                    "diagnose the screenshot".into(),
                    None,
                    "2026-08-27T00:00:00Z".into(),
                )
                .unwrap();
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);
            actor.sync_goal_usage_window();

            actor
                .handle_prompt(
                    "image-400-aux-recovery",
                    crate::session::actor::tests::support::admit_test_human_input(
                        &actor,
                        "image-400-aux-recovery",
                    )
                    .await,
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![
                        acp::ContentBlock::Text(acp::TextContent::new("diagnose this screenshot")),
                        acp::ContentBlock::Image(test_image_content()),
                    ],
                    tool_types::BehaviorId::Goal,
                    Some("goal-image-recovery".into()),
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
            assert!(
                requests[2]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("code E42")
            );

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                1
            );
            assert!(conversation.iter().any(|item| {
                sampling_types::conversation::item_image_description(item)
                    .is_some_and(|text| text.contains("code E42"))
            }));
            let replay = chat_state::Timeline::from_events(
                actor.chat_state_handle.timeline_events().await.unwrap(),
            )
            .unwrap();
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(replay.surface()).len(),
                1
            );
            let original_id = actor.current_catalog_model_id();
            let config = actor.model_route.snapshot().sampling_config;
            actor.model_route.replace(
                crate::agent::models::ModelId::new("other-provider/test-model"),
                config.clone(),
            );
            assert!(actor.unsupported_current_model_for_images().await.is_none());
            let unknown = actor
                .chat_state_handle
                .build_request_for_image_mode(
                    &actor.session_id_string(),
                    vec![],
                    None,
                    None,
                    None,
                    actor.unsupported_current_model_for_images().await.is_some(),
                )
                .await
                .unwrap();
            assert_eq!(
                unknown.image_count(),
                1,
                "another provider first receives the original image"
            );
            actor
                .record_unsupported_model_image_input(actor.current_catalog_model_id())
                .await
                .unwrap();
            assert_eq!(
                actor
                    .project_images_for_known_text_model()
                    .await
                    .unwrap()
                    .total_images(),
                0,
                "existing description is reused without another auxiliary request"
            );
            let known = actor
                .chat_state_handle
                .build_request_for_image_mode(
                    &actor.session_id_string(),
                    vec![],
                    None,
                    None,
                    None,
                    actor.unsupported_current_model_for_images().await.is_some(),
                )
                .await
                .unwrap();
            assert_eq!(known.image_count(), 0);
            actor.model_route.replace(
                crate::agent::models::ModelId::new(original_id.clone()),
                config,
            );
            assert_eq!(
                actor.unsupported_current_model_for_images().await,
                Some(original_id)
            );
            assert_eq!(
                server
                    .requests()
                    .iter()
                    .filter(|r| r.path == "/v1/messages")
                    .count(),
                3
            );
            let goal = actor.goal_tracker.lock().snapshot().cloned().unwrap();
            assert_eq!(
                goal.status,
                crate::session::goal_tracker::GoalStatus::Active
            );
            assert!(!goal.usage_incomplete);

            let drained = drain_notifications(&mut gateway_rx);
            assert!(
                drained
                    .notes
                    .iter()
                    .any(|note| note.contains("已生成 1 张图片的文字描述"))
            );
            assert!(drained.retry_failures.is_empty());
        }));
    });
}

#[test]
fn auxiliary_image_400_installs_a_durable_removal_projection() {
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
            install_test_foreground(&actor, "image-400-aux-400").await;

            actor
                .handle_prompt(
                    "image-400-aux-400",
                    crate::session::actor::tests::support::admit_test_human_input(
                        &actor,
                        "image-400-aux-400",
                    )
                    .await,
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
                .expect("auxiliary rejection must fall back to a durable removal");

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(requests.len(), 3, "primary, auxiliary, resubmit");
            assert!(count_wire_images(requests[0].body.as_ref().unwrap()) > 0);
            assert_eq!(
                requests[1].body.as_ref().unwrap()["model"],
                "vision-model"
            );
            assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
            assert!(
                requests[2]
                    .body
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains(sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT)
            );

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                sampling_types::conversation::conversation_image_groups(&conversation).is_empty()
            );
            let replacements = conversation
                .iter()
                .flat_map(|item| match item {
                    ConversationItem::User(user) => user.content.as_slice(),
                    ConversationItem::ToolResult(result) => result.images.as_slice(),
                    _ => &[],
                })
                .filter(|part| {
                    matches!(
                        part,
                        sampling_types::ContentPart::Text { text }
                            if text.as_ref()
                                == sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
                    )
                })
                .count();
            assert_eq!(replacements, 1, "one canonical replacement per image group");
            assert!(
                actor
                    .unsupported_current_model_for_images()
                    .await
                    .is_some(),
                "the primary pair must stay marked text-only"
            );

            let mut auxiliary_config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            auxiliary_config.model = "vision-model".to_owned();
            actor.model_route.replace(
                crate::agent::models::ModelId::new("vision"),
                actor.model_route.snapshot().sampling_config,
            );
            actor
                .chat_state_handle
                .replace_sampling_route(auxiliary_config);
            assert_eq!(
                actor
                    .unsupported_current_model_for_images()
                    .await
                    .as_deref(),
                Some("vision"),
                "an auxiliary route that rejects images must be marked as well"
            );

            let drained = drain_notifications(&mut gateway_rx);
            assert!(drained.retry_failures.is_empty());
            assert_eq!(drained.image_projected_updates, 1);
            assert_eq!(
                drained.notes,
                vec![
                    "当前模型不支持多模态，1 张图片无法生成文字描述，已从当前会话中删除；原图仍保留在会话历史记录中。"
                        .to_owned()
                ]
            );
        }));
    });
}

/// A permanently failing durable projection must fail closed: the turn ends
/// with a typed error, the provider sees exactly one (image-bearing) request,
/// no projection is accepted, and the Surface keeps the image.
#[test]
fn failed_image_projection_commit_never_resubmits_a_lossy_request() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            let (actor, mut gateway_rx, image_projection_persistence) =
                actor_with_sampler_and_persistence(&server, None).await;
            image_projection_persistence.fail_projection_commits();
            install_test_foreground(&actor, "image-projection-disk-full").await;

            let error = actor
                .handle_prompt(
                    "image-projection-disk-full",
                    crate::session::actor::tests::support::admit_test_human_input(
                        &actor,
                        "image-projection-disk-full",
                    )
                    .await,
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
                )
                .await
                .expect_err("a failed durable projection must not be retried from memory");
            // The projection error is the terminal failure; the turn boundary
            // cannot be recorded either because the poisoned writer closed the
            // chat-state mailbox. Both are typed, and neither is a resubmission.
            assert!(
                format!("{error:?}").contains("turn_boundary_persistence_failed")
                    || format!("{error:?}")
                        .contains("failed to persist text-only image projection"),
                "{error:?}"
            );

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(
                requests.len(),
                1,
                "only the original image request was sent"
            );
            assert!(count_wire_images(requests[0].body.as_ref().unwrap()) > 0);

            let attempted = image_projection_persistence.attempted_projections();
            assert_eq!(
                attempted.len(),
                1,
                "exactly one commit was attempted; it never became durable"
            );
            let (event, acknowledged) = &attempted[0];
            assert!(!acknowledged, "the commit must have failed permanently");
            let chat_state::TimelineEventKind::ImageProjection(projection) = &event.kind else {
                panic!("expected an image projection commit");
            };
            assert_eq!(projection.shadows.len(), 1);
            assert_eq!(
                projection.shadows[0].replacement,
                sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
            );
            assert!(matches!(
                projection.shadows[0].provenance,
                chat_state::ImageShadowSource::UnsupportedModel
            ));

            let durable_surface = image_projection_persistence.durable_surface();
            assert!(
                !sampling_types::conversation::conversation_image_groups(&durable_surface)
                    .is_empty(),
                "the durable Surface must still hold the rejected image: {durable_surface:#?}"
            );

            let drained = drain_notifications(&mut gateway_rx);
            assert_eq!(drained.image_projected_updates, 0);
            assert!(drained.notes.is_empty());
            // The poisoned writer withholds hooked notifications, so this
            // scenario reports the typed turn error instead of a RetryState
            // notice; either way no success path was published.
            assert!(drained.retry_failures.is_empty());
        }));
    });
}

/// One rejected request with a describable group and an unresolvable group
/// commits a single projection carrying both dispositions.
#[test]
fn mixed_description_and_removal_groups_commit_as_one_projection() {
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
                messages_text_turn("Accepted the first screenshot.", "test-model"),
            );
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("A red build-error dialog.", "vision-model"),
            );
            server.enqueue_response(
                "/v1/messages",
                ScriptedResponse::json(
                    400,
                    json!({
                        "type": "error",
                        "error": {
                            "type": "invalid_request_error",
                            "message": "request body rejected: tool schema is not an object"
                        }
                    }),
                ),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_text_turn("Recovered with a mixed projection.", "test-model"),
            );
            let (actor, mut gateway_rx) = actor_with_sampler(&server, Some("vision")).await;

            for label in ["mixed-first", "mixed-second"] {
                install_test_foreground(&actor, label).await;
                actor
                    .handle_prompt(
                        label,
                        crate::session::actor::tests::support::admit_test_human_input(
                            &actor, label,
                        )
                        .await,
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
                    .unwrap_or_else(|error| panic!("{label} must complete: {error:?}"));
                release_settled_foreground(&actor).await;
            }

            let requests: Vec<_> = server
                .requests()
                .into_iter()
                .filter(|request| request.path == "/v1/messages")
                .collect();
            assert_eq!(
                requests.len(),
                5,
                "two primary turns, two auxiliary calls, one resubmit"
            );
            assert_eq!(count_wire_images(requests[4].body.as_ref().unwrap()), 0);

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let projections = events
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::ImageProjection(projection) => Some(projection),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(projections.len(), 1, "one event carries both groups");
            assert_eq!(projections[0].shadows.len(), 2);
            let described = projections[0]
                .shadows
                .iter()
                .filter(|shadow| {
                    matches!(
                        shadow.provenance,
                        chat_state::ImageShadowSource::Description { .. }
                    )
                })
                .count();
            let removed = projections[0]
                .shadows
                .iter()
                .filter(|shadow| {
                    matches!(
                        shadow.provenance,
                        chat_state::ImageShadowSource::UnsupportedModel
                    )
                })
                .count();
            assert_eq!((described, removed), (1, 1));

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                1,
                "the described group keeps its image"
            );
            assert_eq!(
                conversation
                    .iter()
                    .filter_map(sampling_types::conversation::item_image_description)
                    .count(),
                1
            );
            assert_eq!(
                conversation
                    .iter()
                    .flat_map(|item| match item {
                        ConversationItem::User(user) => user.content.as_slice(),
                        ConversationItem::ToolResult(result) => result.images.as_slice(),
                        _ => &[],
                    })
                    .filter(|part| matches!(
                        part,
                        sampling_types::ContentPart::Text { text }
                            if text.as_ref()
                                == sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
                    ))
                    .count(),
                1
            );

            let drained = drain_notifications(&mut gateway_rx);
            assert!(drained.retry_failures.is_empty());
            assert_eq!(drained.image_projected_updates, 1);
            assert_eq!(
                drained.notes,
                vec![
                    "已生成 1 张图片的文字描述；1 张图片无法生成描述，已从当前会话中删除。"
                        .to_owned()
                ]
            );
        }));
    });
}
