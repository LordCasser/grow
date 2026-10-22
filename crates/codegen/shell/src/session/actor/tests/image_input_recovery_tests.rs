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
    actor_with_sampler_and_storage(server, image_description_model, None).await
}

async fn actor_with_sampler_and_storage(
    server: &MockInferenceServer,
    image_description_model: Option<&str>,
    storage: Option<crate::session::storage::jsonl::JsonlStorageAdapter>,
) -> (
    std::sync::Arc<SessionActor>,
    mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    ImageProjectionPersistence,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    let sideband_storage = storage.clone();
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            use crate::session::storage::StorageAdapter as _;
            let info = SessionInfo {
                id: acp::SessionId::new("test-actor"),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            };
            match message {
                PersistenceMsg::SidebandDurablyAndAck { event, respond_to } => {
                    let result = if let Some(storage) = &sideband_storage {
                        storage.append_sideband_event_durable(&info, &event).await
                    } else {
                        Ok(())
                    };
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::Update(update) => {
                    if let Some(storage) = &sideband_storage {
                        storage
                            .append_update_durable_commit_aware(&info, &update)
                            .await
                            .expect("persist the image fixture's original display fact");
                    }
                }
                PersistenceMsg::AppendUpdateDurablyAndAck { update, respond_to }
                | PersistenceMsg::AppendUpdateBufferedAndAck { update, respond_to } => {
                    let result = if let Some(storage) = &sideband_storage {
                        storage
                            .append_update_durable_commit_aware(&info, &update)
                            .await
                    } else {
                        Ok(())
                    };
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::AuxiliaryGrow {
                    notification,
                    delivery,
                    respond_to,
                    ..
                } => {
                    let result = if let Some(storage) = &sideband_storage {
                        storage
                            .append_update_durable_commit_aware(
                                &info,
                                &crate::session::storage::SessionUpdate::Grow(Box::new(
                                    notification,
                                )),
                            )
                            .await
                    } else {
                        Ok(())
                    };
                    if matches!(
                        &result,
                        Ok(()) | Err(crate::session::storage::AppendUpdateError::Committed(_))
                    ) && let Some(delivery) = delivery
                    {
                        delivery.publish();
                    }
                    if let Some(respond_to) = respond_to {
                        let _ = respond_to.send(result);
                    }
                }
                _ => {}
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
    let downstream = actor.notifications.persistence_tx.clone();
    let (sideband_tx, mut sideband_rx) = mpsc::unbounded_channel();
    actor.notifications.persistence_tx = sideband_tx;
    let sideband_gate = image_projection_persistence.clone();
    tokio::task::spawn_local(async move {
        while let Some(message) = sideband_rx.recv().await {
            let message = match message {
                PersistenceMsg::SidebandDurablyAndAck { event, respond_to }
                    if matches!(event.kind, chat_state::SidebandEventKind::Attempt(_)) =>
                {
                    let (forward_reply, forward_ack) = tokio::sync::oneshot::channel();
                    downstream
                        .send(PersistenceMsg::SidebandDurablyAndAck {
                            event,
                            respond_to: forward_reply,
                        })
                        .unwrap();
                    let result = forward_ack.await.unwrap();
                    if let Some(reply) = sideband_gate.delay_sideband_ack(respond_to) {
                        let _ = reply.send(result);
                    }
                    continue;
                }
                other => other,
            };
            let _ = downstream.send(message);
        }
    });
    if let Some(storage) = storage {
        use crate::session::storage::StorageAdapter as _;
        let sampling_config = actor.chat_state_handle.get_sampling_config().await.unwrap();
        let loaded = storage
            .load_session_without_updates(&actor.session_info)
            .await
            .unwrap();
        image_projection_persistence
            .persist_to_storage(storage.clone(), actor.session_info.clone());
        let (chat_event_tx, _) = mpsc::unbounded_channel();
        actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
            loaded.timeline.events().to_vec(),
            sampling_config,
            Box::new(image_projection_persistence.clone()),
            chat_event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
        actor.session_directory = storage
            .open_session(&actor.session_info)
            .unwrap()
            .directory_handle();
        actor.session_dir = actor.session_directory.display_path().to_path_buf();
    }
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
    let usage_actor = std::sync::Arc::downgrade(&actor);
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
            let Some(usage_actor) = usage_actor.upgrade() else {
                break;
            };
            let result = usage_actor
                .settle_claimed_goal_usage_attempt(&attempt_id)
                .await;
            let _ = respond_to.send(result);
        }
    });
    let drainer = std::sync::Arc::downgrade(&actor);
    tokio::task::spawn_local(async move {
        let mut sampler_event_rx = sampler_event_rx;
        while let Some(event) = sampler_event_rx.recv().await {
            let Some(drainer) = drainer.upgrade() else {
                break;
            };
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
                actor.unsupported_current_model_for_images().await.is_some(),
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
            assert_eq!(drained.notes, vec!["Could not convert 1 image".to_owned()]);

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
            assert_eq!(
                requests.len(),
                3,
                "the follow-up turn must not retry images"
            );
            assert_eq!(count_wire_images(requests[2].body.as_ref().unwrap()), 0);
        }));
    });
}

#[test]
fn switched_chat_text_only_type_rejection_converts_historical_tool_images() {
    assert_chat_tool_image_fallback(true);
}

#[test]
fn current_chat_text_only_type_rejection_converts_new_tool_images() {
    assert_chat_tool_image_fallback(false);
}

fn assert_chat_tool_image_fallback(image_precedes_route_selection: bool) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/chat/completions", ScriptedResponse::json(400, json!({
                "error": {"code": "1210", "message": "messages.content.type 参数非法，取值范围 ['text']"}
            })));
            server.enqueue_response("/v1/messages", messages_text_turn("Historical screenshot shows code E42.", "vision-model"));
            server.enqueue_response("/v1/chat/completions", ScriptedResponse::sse(vec![SseEvent::data(json!({
                "id": "chat-image-recovery", "object": "chat.completion.chunk", "created": 0,
                "model": "glm-5.3", "choices": [{"index": 0, "delta": {"content": "Recovered."}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }).to_string())]));
            let (actor, mut gateway_rx) = actor_with_sampler(&server, Some("vision")).await;
            let add_tool_image = || {
                actor.chat_state_handle.push_user_message(ConversationItem::user("inspect the screenshot"));
                actor.chat_state_handle.push_assistant_response(ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "tool-screenshot".into(), name: "read_file".into(), arguments: "{}".into()
                }]));
                actor.chat_state_handle.push_tool_result(ConversationItem::tool_result_with_images(
                    "tool-screenshot", "Screenshot captured.", vec![sampling_types::ContentPart::Image {
                        url: inline_data_uri(&test_image_content()).into(), description: None
                    }]));
            };
            if image_precedes_route_selection {
                add_tool_image();
            }
            let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            config.api_backend = sampling_types::ApiBackend::ChatCompletions;
            config.model = "glm-5.3".into();
            actor.chat_state_handle.replace_sampling_route(config);
            actor.model_route.replace(crate::agent::models::ModelId::new("bigmodel/glm-5.3"), actor.reconstruct_full_config().await);
            let primary_route_revision = actor.model_route.snapshot().revision;
            if !image_precedes_route_selection {
                // A new tool image arrives while the primary route stays the same.
                add_tool_image();
            }
            install_test_foreground(&actor, "switched-image-recovery").await;
            actor.goal_tracker.lock().create_goal("switched-goal".into(), "continue".into(), None, "2026-10-18T00:00:00Z".into()).unwrap();
            actor.behavior.lock().select_behavior(tool_types::BehaviorId::Goal);
            actor.sync_goal_usage_window();
            actor.handle_prompt(
                "switched-image-recovery", admit_test_human_input(&actor, "switched-image-recovery").await,
                crate::session::PromptOrigin::User, Vec::new(), crate::session::TurnKind::User,
                vec![acp::ContentBlock::Text(acp::TextContent::new("continue"))],
                tool_types::BehaviorId::Goal, Some("switched-goal".into()), None, false, None, None,
            ).await.expect("text-only type rejection must enter the existing image fallback");

            let primary: Vec<_> = server.requests().into_iter().filter(|r| r.path == "/v1/chat/completions").collect();
            assert_eq!(primary.len(), 2, "one rejection followed by one repaired request");
            assert_eq!(count_wire_images(primary[0].body.as_ref().unwrap()), 1);
            assert!(primary[0].body.as_ref().unwrap()["messages"].as_array().unwrap().iter().any(|m|
                m["role"] == "tool" && count_wire_images(m) == 1));
            assert_eq!(count_wire_images(primary[1].body.as_ref().unwrap()), 0);
            assert!(primary[1].body.as_ref().unwrap().to_string().contains("code E42"));
            let auxiliary: Vec<_> = server.requests().into_iter().filter(|r| r.path == "/v1/messages").collect();
            assert_eq!(auxiliary.len(), 1);
            assert_eq!(auxiliary[0].body.as_ref().unwrap()["model"], "vision-model");
            assert_eq!(count_wire_images(auxiliary[0].body.as_ref().unwrap()), 1);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|e| matches!(e.kind, chat_state::TimelineEventKind::ImageProjection(_))));
            assert_eq!(sealed_image_groups(&events), 1, "original tool image remains immutable");
            let replay = chat_state::Timeline::from_events(events).unwrap();
            assert_eq!(sampling_types::conversation::conversation_image_groups(replay.surface()).len(), 1);
            assert!(replay.surface().iter().any(|item| sampling_types::conversation::item_image_description(item)
                .is_some_and(|description| description.contains("code E42"))));
            assert_eq!(actor.unsupported_current_model_for_images().await.as_deref(), Some("bigmodel/glm-5.3"));
            assert_eq!(actor.model_route.snapshot().revision, primary_route_revision, "image recovery must keep the primary route");
            assert!(!actor.goal_tracker.lock().snapshot().unwrap().usage_incomplete,
                "the request validation rejection must settle as exact zero usage");
            let drained = drain_notifications(&mut gateway_rx);
            assert!(drained.retry_failures.is_empty());
            assert_eq!(drained.image_projected_updates, 1);
            assert!(drained.image_processing_messages.is_empty());
            assert!(!drained.image_recovery_operation_ids.is_empty());
            assert!(drained.image_recovery_operation_ids.iter().all(|id|
                id == &drained.image_recovery_operation_ids[0]
            ));
        }));
    });
}

/// Projection-related notifications the client received during a turn.
#[derive(Default)]
struct DrainedNotifications {
    image_projected_updates: usize,
    image_recovery_operation_ids: Vec<String>,
    image_recovery_progress: Vec<String>,
    image_processing_messages: Vec<String>,
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
            GrowSessionUpdate::ImageProcessing { message } => {
                drained.image_processing_messages.push(message);
            }
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
            GrowSessionUpdate::ImageRecoveryOperation {
                operation_id,
                state,
                ..
            } => {
                use crate::extensions::notification::ImageRecoveryOperationState as State;
                drained.image_recovery_operation_ids.push(operation_id);
                match state {
                    State::Started { message }
                    | State::Vision { message }
                    | State::Ocr { message } => {
                        drained.image_recovery_progress.push(message);
                    }
                    State::Completed { message } => {
                        drained.image_projected_updates += 1;
                        drained.notes.push(message);
                    }
                    State::Failed { message } => {
                        // A removal projection is committed successfully even
                        // when image conversion failed; a transaction failure
                        // must not be counted as a projected update.
                        if message.starts_with("Could not convert ")
                            || message.contains(" image could not be converted")
                            || message.contains(" images could not be converted")
                        {
                            drained.image_projected_updates += 1;
                            drained.notes.push(message);
                        }
                    }
                }
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
            let auxiliary_body_text = requests[1].body.as_ref().unwrap().to_string();
            assert!(
                auxiliary_body_text
                    .contains(crate::session::image_describe::DEFAULT_DESCRIBE_PROMPT)
            );
            assert!(!auxiliary_body_text.contains("diagnose this screenshot"));
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
            let original_sampling = actor.chat_state_handle.get_sampling_config().await.unwrap();
            let mut other_sampling = original_sampling.clone();
            other_sampling.model = "other-model".into();
            actor
                .chat_state_handle
                .replace_sampling_route(other_sampling);
            let mut other_config = config.clone();
            other_config.model = "other-model".into();
            actor.model_route.replace(
                crate::agent::models::ModelId::new("other-provider/test-model"),
                other_config,
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
                .record_unsupported_model_image_input(
                    actor.current_model_image_input_key().await.unwrap(),
                )
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
            actor
                .chat_state_handle
                .replace_sampling_route(original_sampling);
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
            assert!(drained.image_processing_messages.is_empty());
            assert!(drained.image_recovery_operation_ids.len() >= 3);
            assert!(
                drained
                    .image_recovery_operation_ids
                    .iter()
                    .all(|id| id == &drained.image_recovery_operation_ids[0])
            );
            assert!(
                drained
                    .notes
                    .iter()
                    .any(|note| note.contains("Converted 1 image"))
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
            assert_eq!(requests[1].body.as_ref().unwrap()["model"], "vision-model");
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
                actor.unsupported_current_model_for_images().await.is_some(),
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
            assert!(drained.image_processing_messages.is_empty());
            assert!(
                drained
                    .image_recovery_operation_ids
                    .iter()
                    .all(|id| id == &drained.image_recovery_operation_ids[0])
            );
            assert_eq!(drained.notes, vec!["Could not convert 1 image".to_owned()]);
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
            assert!(
                drained
                    .image_recovery_operation_ids
                    .iter()
                    .all(|id| id == &drained.image_recovery_operation_ids[0])
            );
            assert_eq!(
                drained.notes,
                vec!["Converted 1 image; 1 image could not be converted".to_owned()]
            );
        }));
    });
}

#[test]
fn read_file_image_fallback_uses_refreshed_auxiliary_and_waits_for_projection_ack() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for known_primary in [false, true] {
                let primary = MockInferenceServer::start().await.unwrap();
                let auxiliary = MockInferenceServer::start().await.unwrap();
                if !known_primary {
                    primary.enqueue_response("/v1/messages", multimodal_capability_rejection());
                }
                primary.enqueue_response("/v1/messages", messages_text_turn("continued", "test-model"));
                let description = "The screenshot shows a blue dialog with code E73.";
                auxiliary.enqueue_response("/v1/responses", ScriptedResponse::sse(vec![SseEvent::data(json!({
                    "type": "response.completed", "sequence_number": 0,
                    "response": {
                        "id": "resp_image", "object": "response", "created_at": 0,
                        "model": "vision-v2", "status": "completed",
                        "output": [{"type": "message", "id": "msg_image", "role": "assistant",
                            "status": "completed", "content": [{"type": "output_text", "text": description, "annotations": []}]}],
                        "usage": {"input_tokens": 10, "input_tokens_details": {"cached_tokens": 0},
                            "output_tokens": 5, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 15}
                    }
                }).to_string())]));
                let (actor, _gateway_rx, persistence) = actor_with_sampler_and_persistence(&primary, Some("vision")).await;
                let mut info = crate::agent::config::ModelInfo::baseline("vision-v2");
                info.base_url = auxiliary.url();
                info.api_backend = sampling_types::ApiBackend::Responses;
                actor.models_manager.insert_test_entry("vision", crate::agent::config::ModelEntry {
                    info, api_key: Some("test-key".into()), env_key: None, auth_provider: None,
                });
                if known_primary {
                    actor.record_unsupported_model_image_input(actor.current_model_image_input_key().await.unwrap()).await.unwrap();
                }
                let history_marker = "LONG_HISTORY_SENTINEL";
                let skill_body_marker = "SKILL_BODY_SENTINEL";
                actor.chat_state_handle.push_user_message(ConversationItem::user(
                    format!(
                        "{history_marker} {skill_body_marker} {}",
                        "historical context ".repeat(1_000)
                    ),
                ));
                let original = super::read_file_image_description_tests::run_image_result(&actor).await;
                assert_eq!(original.images.len(), 1);
                persistence.hold_projection_acknowledgement();
                let prompt_actor = actor.clone();
                let prompt = tokio::task::spawn_local(async move {
                    admit_inline_query(&prompt_actor, "read-image-ack", "Continue from the image.".into()).await;
                });
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while persistence.attempted_projections().is_empty() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }).await.expect("auxiliary result must reach the projection commit");
                assert!(!prompt.is_finished());
                assert_eq!(primary.requests().len(), usize::from(!known_primary), "no image request or retry before durable projection ACK");
                let aux_requests = auxiliary.requests();
                assert_eq!(aux_requests.len(), 1);
                assert_eq!(aux_requests[0].path, "/v1/responses");
                assert_eq!(aux_requests[0].body.as_ref().unwrap()["model"], "vision-v2");
                assert_eq!(count_wire_images(aux_requests[0].body.as_ref().unwrap()), 1);
                let auxiliary_body = aux_requests[0].body.as_ref().unwrap();
                let auxiliary_body_text = auxiliary_body.to_string();
                assert!(auxiliary_body_text.contains(crate::session::image_describe::DEFAULT_DESCRIBE_PROMPT));
                assert!(!auxiliary_body_text.contains(history_marker));
                assert!(!auxiliary_body_text.contains(skill_body_marker));
                assert!(!auxiliary_body_text.contains("/workspace/image.png"));
                assert!(!auxiliary_body_text.contains("[image inline]"));
                assert!(!auxiliary_body_text.contains("read_file"));
                assert!(auxiliary_body.get("tools").is_none(), "visual sideband has no tool or Skill manifest");
                persistence.release_projection_acknowledgement();
                tokio::time::timeout(std::time::Duration::from_secs(5), prompt).await.unwrap().unwrap();
                let requests = primary.requests();
                assert_eq!(requests.len(), if known_primary { 1 } else { 2 });
                if !known_primary {
                    assert_eq!(count_wire_images(requests[0].body.as_ref().unwrap()), 1);
                }
                let repaired = requests.last().unwrap().body.as_ref().unwrap();
                assert_eq!(count_wire_images(repaired), 0);
                assert!(repaired.to_string().contains(description));
                assert!(persistence.attempted_projections().iter().all(|(_, ack)| *ack));
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(events.iter().filter_map(|event| event.messages()).any(|messages| {
                    messages.items.iter().any(|item| matches!(item,
                        ConversationItem::ToolResult(result) if result.tool_call_id == "read-image-1" && result.images.len() == 1))
                }), "raw tool image must remain in Timeline");
            }
        }));
    });
}

#[cfg(unix)]
fn ocr_fixture(body: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("tesseract");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\n/bin/cat > '{}'\n{body}\n",
            dir.path().join("input").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

async fn wait_for_image_projection(persistence: &ImageProjectionPersistence) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while persistence.attempted_projections().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("image fallback must reach projection persistence");
}

#[cfg(unix)]
#[test]
fn image_fallback_ocr_success_empty_and_failure_complete_the_durable_request_chain() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for outcome in ["printf '错误代码 E81\\n'", "exit 0", "exit 1"] {
                let fixture = ocr_fixture(outcome);
                let server = MockInferenceServer::start().await.unwrap();
                server.enqueue_response("/v1/messages", multimodal_capability_rejection());
                server.enqueue_response("/v1/messages", ScriptedResponse::json(400, json!({
                    "error": {"type": "invalid_request_error", "message": "auxiliary unavailable for this request"}
                })));
                server.enqueue_response("/v1/messages", messages_text_turn("continued", "test-model"));
                let (actor, mut gateway, persistence) = actor_with_sampler_and_persistence(&server, Some("vision")).await;
                super::read_file_image_description_tests::run_image_result(&actor).await;
                persistence.hold_projection_acknowledgement();
                let turn_actor = actor.clone();
                let turn = tokio::task::spawn_local(crate::session::image_describe::TEST_LOCAL_OCR_PROGRAM.scope(
                    fixture.path().join("tesseract"), async move {
                        admit_inline_query(&turn_actor, "ocr-fallback", "Continue.".into()).await;
                    },
                ));
                wait_for_image_projection(&persistence).await;
                assert!(std::fs::read(fixture.path().join("input")).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"), "the OCR process must consume the original PNG");
                assert_eq!(server.requests().len(), 2, "no primary retry before OCR projection ACK");
                assert!(!turn.is_finished());
                persistence.release_projection_acknowledgement();
                tokio::time::timeout(std::time::Duration::from_secs(5), turn).await.unwrap().unwrap();
                let requests = server.requests();
                assert_eq!(requests.len(), 3);
                assert_eq!(requests[1].body.as_ref().unwrap()["model"], "vision-model");
                assert_eq!(count_wire_images(requests[0].body.as_ref().unwrap()), 1);
                assert_eq!(count_wire_images(requests[1].body.as_ref().unwrap()), 1);
                let repaired = requests[2].body.as_ref().unwrap();
                assert_eq!(count_wire_images(repaired), 0);
                let projections = persistence.attempted_projections();
                let chat_state::TimelineEventKind::ImageProjection(projection) = &projections[0].0.kind else { panic!("projection"); };
                if outcome.starts_with("printf") {
                    assert!(matches!(projection.shadows[0].provenance, chat_state::ImageShadowSource::LocalOcr { .. }));
                    assert!(repaired.to_string().contains("错误代码 E81"));
                    assert_eq!(sampling_types::conversation::conversation_image_groups(&persistence.durable_surface()).len(), 1);
                } else {
                    assert!(matches!(projection.shadows[0].provenance, chat_state::ImageShadowSource::UnsupportedModel));
                    assert!(repaired.to_string().contains(sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT));
                    assert!(sampling_types::conversation::conversation_image_groups(&persistence.durable_surface()).is_empty());
                }
                assert_eq!(sealed_image_groups(&actor.chat_state_handle.timeline_events().await.unwrap()), 1);
                let drained = drain_notifications(&mut gateway);
                assert!(drained.image_processing_messages.is_empty());
                assert_eq!(drained.image_recovery_progress, vec![
                    "Preparing image recovery…",
                    "Calling the configured vision model…",
                    "Trying local OCR…",
                ]);
                assert!(drained.image_recovery_operation_ids.len() >= 4);
                assert!(drained.image_recovery_operation_ids.iter().all(|id|
                    id == &drained.image_recovery_operation_ids[0]
                ));
            }
        }));
    });
}

#[cfg(unix)]
#[test]
fn cancelling_image_fallback_before_or_during_auxiliary_stops_later_work() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use test_support::{InferenceEndpoint, InferenceRequestMatcher};
            for phase in ["primary", "preparation", "auxiliary"] {
                crate::session::image_describe::TEST_LOCAL_OCR_CALLS.with(|calls| calls.set(0));
                let primary = MockInferenceServer::start().await.unwrap();
                let auxiliary = MockInferenceServer::start().await.unwrap();
                let mut blocked = if phase == "auxiliary" {
                    primary.enqueue_response("/v1/messages", multimodal_capability_rejection());
                    Some(auxiliary.expect_response_blocked(
                        "first image description",
                        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                        messages_text_turn("first image", "vision-model"),
                    ))
                } else if phase == "primary" {
                    Some(primary.expect_response_blocked(
                        "primary image rejection",
                        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                        multimodal_capability_rejection(),
                    ))
                } else {
                    primary.enqueue_response("/v1/messages", multimodal_capability_rejection());
                    None
                };
                let (actor, _gateway, persistence) =
                    actor_with_sampler_and_persistence(&primary, Some("vision")).await;
                if phase == "preparation" {
                    persistence.hold_next_sideband_attempt();
                }
                let mut info = crate::agent::config::ModelInfo::baseline("vision-model");
                info.base_url = auxiliary.url();
                info.api_backend = sampling_types::ApiBackend::Messages;
                actor.models_manager.insert_test_entry(
                    "vision",
                    crate::agent::config::ModelEntry {
                        info,
                        api_key: Some("test-key".into()),
                        env_key: None,
                        auth_provider: None,
                    },
                );
                for label in ["first", "second"] {
                    actor
                        .chat_state_handle
                        .push_user_message(ConversationItem::user_with_parts(vec![
                            sampling_types::ContentPart::Text { text: label.into() },
                            sampling_types::ContentPart::Image {
                                url: inline_data_uri(&test_image_content()).into(),
                                description: None,
                            },
                        ]));
                }
                let (start_tx, start_rx) = tokio::sync::oneshot::channel();
                let (completion_tx, _completion_rx) = mpsc::unbounded_channel();
                let task = AgentTask::new_prompt(
                    actor.clone(),
                    "cancel-image-fallback".into(),
                    admit_test_human_input(&actor, "cancel-image-fallback").await,
                    crate::session::PromptOrigin::User,
                    None,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![acp::ContentBlock::Text(acp::TextContent::new("Continue."))],
                    tool_types::BehaviorId::Normal,
                    None,
                    None,
                    false,
                    None,
                    Some(start_rx),
                    None,
                    completion_tx,
                    None,
                );
                let producer = task.handle.clone();
                actor.state.lock().await.foreground = ForegroundState::RegularTurn(task);
                start_tx.send(()).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    if let Some(blocked) = &mut blocked {
                        blocked.wait_received().await;
                    } else {
                        while !persistence.sideband_ack_is_pending() {
                            tokio::task::yield_now().await;
                        }
                    }
                })
                .await
                .unwrap();
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    actor.cancel_running_task(false, false, false, Some("esc".into())),
                )
                .await
                .unwrap()
                .unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while !producer.is_finished() {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("Stop must abort the actual foreground producer");
                if let Some(blocked) = blocked {
                    blocked.release();
                }
                persistence.release_sideband_ack();
                // Also release the server response and drain cleanup: cancellation must not
                // merely postpone a second group's request until the first response returns.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                assert_eq!(primary.requests().len(), 1);
                assert_eq!(
                    auxiliary.requests().len(),
                    usize::from(phase == "auxiliary")
                );
                crate::session::image_describe::TEST_LOCAL_OCR_CALLS
                    .with(|calls| assert_eq!(calls.get(), 0));
                assert!(persistence.attempted_projections().is_empty());
                let conversation = actor.chat_state_handle.get_conversation().await;
                assert_eq!(
                    sampling_types::conversation::conversation_image_groups(&conversation).len(),
                    2
                );
            }
        }));
    });
}

#[test]
fn image_fallback_cold_restore_reuses_completed_description_before_projection() {
    assert_image_fallback_cold_restore(false, false, true, false);
}

#[test]
fn image_fallback_cold_restore_reuses_acknowledged_projection() {
    assert_image_fallback_cold_restore(true, false, true, false);
}

#[cfg(unix)]
#[test]
fn image_fallback_cold_restore_reuses_acknowledged_ocr_projection() {
    assert_image_fallback_cold_restore(true, true, true, false);
}

#[test]
fn image_fallback_cold_restore_reuses_description_without_auxiliary_route() {
    assert_image_fallback_cold_restore(false, false, false, false);
}

#[test]
fn image_fallback_cold_restore_retries_completed_legacy_prompt() {
    assert_image_fallback_cold_restore(false, false, true, true);
}

fn assert_image_fallback_cold_restore(
    already_projected: bool,
    from_ocr: bool,
    with_auxiliary: bool,
    legacy_prompt: bool,
) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::storage::StorageAdapter as _;
            let root = tempfile::tempdir().unwrap();
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.path().to_path_buf());
            let info = SessionInfo { id: acp::SessionId::new("test-actor"), cwd: std::env::temp_dir().to_string_lossy().into_owned() };
            storage.init_session(&info, crate::agent::models::ModelId::new("test-model")).await.unwrap();
            let seed = chat_state::Timeline::from_seed(vec![ConversationItem::system("test system prompt")]).unwrap();
            for event in seed.events() { storage.append_timeline_event_durable(&info, event).await.unwrap(); }
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            server.enqueue_response("/v1/messages", if from_ocr {
                ScriptedResponse::json(400, json!({"error": {"type": "invalid_request_error", "message": "auxiliary unavailable"}}))
            } else {
                messages_text_turn("Durable screenshot description E91.", "vision-model")
            });
            // A held projection ACK prevents the original foreground retry.
            // In the legacy case its next request is the regenerated Sideband.
            if already_projected || !legacy_prompt {
                server.enqueue_response("/v1/messages", messages_text_turn("continued", "test-model"));
            }
            if already_projected { server.enqueue_response("/v1/messages", messages_text_turn("resumed", "test-model")); }
            if legacy_prompt {
                server.enqueue_response("/v1/messages", messages_text_turn("regenerated under the fixed prompt", "vision-model"));
                server.enqueue_response("/v1/messages", messages_text_turn("resumed after regeneration", "test-model"));
            }
            let (actor, _gateway, persistence) = actor_with_sampler_and_storage(&server, Some("vision"), Some(storage.clone())).await;
            super::read_file_image_description_tests::run_image_result(&actor).await;
            if !already_projected { persistence.hold_projection_acknowledgement(); }
            let turn_actor = actor.clone();
            #[cfg(unix)]
            let ocr = from_ocr.then(|| ocr_fixture("printf 'Durable screenshot description E91.\\n'"));
            let turn = tokio::task::spawn_local(async move {
                let prompt = admit_inline_query(&turn_actor, "before-crash", "Inspect the image.".into());
                #[cfg(unix)]
                if let Some(ocr) = ocr {
                    crate::session::image_describe::TEST_LOCAL_OCR_PROGRAM.scope(ocr.path().join("tesseract"), prompt).await;
                    return;
                }
                prompt.await;
            });
            if already_projected {
                tokio::time::timeout(std::time::Duration::from_secs(5), turn).await.unwrap().unwrap();
            } else {
                wait_for_image_projection(&persistence).await;
                turn.abort();
                assert!(turn.await.unwrap_err().is_cancelled());
            }
            // Wait for the real Sideband ledger, not the test channel's observed ACK.
            let mut last_validation_error = None;
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    match storage
                        .open_session(&info)
                        .unwrap()
                        .validated_timeline(&info.id.to_string())
                    {
                        Ok(validated) => {
                            if validated.sidebands.values().any(|events| {
                                events.last().is_some_and(|event| matches!(&event.kind,
                                    chat_state::SidebandEventKind::End(end) if from_ocr || end.outcome == chat_state::SidebandOutcome::Completed))
                            }) {
                                break;
                            }
                        }
                        Err(error) => last_validation_error = Some(error.to_string()),
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "timed out waiting for the expected Sideband End; last validated_timeline error: {last_validation_error:?}"
                )
            });
            if legacy_prompt {
                // A completed v1 Sideband used to include source text in its
                // prompt. It must not satisfy the fixed-prompt lookup after
                // cold restore; recovery should run the auxiliary again.
                let validated = storage.open_session(&info).unwrap().validated_timeline(&info.id.to_string()).unwrap();
                let (id, mut events) = validated.sidebands.into_iter().find(|(_, events)| matches!(
                    &events[0].kind, chat_state::SidebandEventKind::Request(request)
                        if request.purpose == chat_state::SidebandPurpose::ImageDescription
                )).unwrap();
                let request = events.iter_mut().find_map(|event| match &mut event.kind {
                    chat_state::SidebandEventKind::Request(request) => Some(request),
                    _ => None,
                }).unwrap();
                request.prompt = format!(
                    "Transcribe the attached image into a neutral, standalone textual description for a model that cannot see images. Preserve all visible text exactly where practical, structure, spatial relationships, UI state, diagrams, tables, code, errors, and other concrete details. Do not tailor the description to a presumed task or infer from conversation context. <image_source>ToolResult message with 1 image attachment(s), in attachment order. Existing message text:\n[image inline]\n</image_source>"
                );
                assert_ne!(request.prompt, crate::session::image_describe::DEFAULT_DESCRIBE_PROMPT);
                crate::session::storage::write_jsonl_atomic(
                    &storage.open_session(&info).unwrap().directory_handle().display_path().join(crate::session::storage::SIDEBANDS_DIR).join(id).join(crate::session::storage::TIMELINE_FILE),
                    &events,
                ).unwrap();
            }
            let resources_persistence = tools::persistence::ResourcesPersistence::local(root.path().join("resources_state.json")).unwrap();
            let bridge = actor.agent.borrow().tool_bridge().clone();
            let resources = bridge.shared_resources().await;
            let serialized = resources.lock().await.serialize();
            resources_persistence.save_and_flush(serialized).await.unwrap();
            let previous_actor = std::sync::Arc::downgrade(&actor);
            drop(actor);
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while previous_actor.upgrade().is_some() { tokio::task::yield_now().await; }
            }).await.expect("the original actor and its cache must be dropped");
            let before = server.requests().len();
            let (restored, _gateway, restored_persistence) = actor_with_sampler_and_storage(&server, with_auxiliary.then_some("vision"), Some(storage.clone())).await;
            let bridge = restored.agent.borrow().tool_bridge().clone();
            let resources = bridge.shared_resources().await;
            assert!(resources_persistence.load(&mut *resources.lock().await));
            assert!(restored.unsupported_current_model_for_images().await.is_some());
            admit_inline_query(&restored, "after-crash", "Continue from the stored image.".into()).await;
            let requests = server.requests();
            assert_eq!(
                requests.len(),
                before + if legacy_prompt { 2 } else { 1 },
                "{}",
                if legacy_prompt {
                    "a completed legacy-prompt Sideband must be rejected and regenerated"
                } else {
                    "cold recovery must reuse the completed Sideband without another auxiliary request"
                }
            );
            assert_eq!(
                requests.iter().filter(|request| request.body.as_ref().unwrap()["model"] == "vision-model").count(),
                if legacy_prompt { 2 } else { 1 },
                "only a matching fixed-prompt Sideband can be reused"
            );
            let resumed = requests.last().unwrap().body.as_ref().unwrap();
            assert_eq!(count_wire_images(resumed), 0);
            assert!(resumed.to_string().contains(if legacy_prompt {
                "regenerated under the fixed prompt"
            } else {
                "Durable screenshot description E91."
            }));
            assert_eq!(restored_persistence.attempted_projections().len(), usize::from(!already_projected));
            let loaded = storage.load_session_without_updates(&info).await.unwrap();
            assert_eq!(sealed_image_groups(loaded.timeline.events()), 1);
            if !already_projected {
                let validated = storage.open_session(&info).unwrap().validated_timeline(&info.id.to_string()).unwrap();
                let (id, mut events) = validated.sidebands.into_iter().find(|(id, _)| {
                    loaded.timeline.events().iter().any(|event| {
                        let chat_state::TimelineEventKind::ImageProjection(projection) = &event.kind else { return false; };
                        projection.shadows.iter().any(|shadow| matches!(&shadow.provenance,
                            chat_state::ImageShadowSource::Description { result_ref } if &result_ref.timeline_id == id))
                    })
                }).expect("the Sideband actually referenced by the committed projection");
                for event in &mut events {
                    if let chat_state::SidebandEventKind::Attempt(attempt) = &mut event.kind {
                        attempt.assembly_manifest.source_revision = Some(loaded.timeline.surface_revision() + 1);
                    }
                }
                crate::session::storage::write_jsonl_atomic(
                    &restored.session_dir.join(crate::session::storage::SIDEBANDS_DIR).join(id).join(crate::session::storage::TIMELINE_FILE), &events,
                ).unwrap();
                assert!(storage.load_session_without_updates(&info).await.is_err(), "future revision provenance must fail persisted projection validation");
            }
        }));
    });
}

#[cfg(unix)]
#[test]
fn cancelling_image_fallback_during_ocr_reaps_process_without_resubmission() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            crate::session::image_describe::TEST_LOCAL_OCR_CALLS.with(|calls| calls.set(0));
            let fixture = ocr_fixture("printf '%s' \"$$\" > \"${0%/*}/pid\"\nexec /bin/sleep 60");
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            server.enqueue_response("/v1/messages", ScriptedResponse::json(400, json!({
                "error": {"type": "invalid_request_error", "message": "auxiliary unavailable"}
            })));
            let (actor, _gateway, persistence) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            super::read_file_image_description_tests::run_image_result(&actor).await;
            let turn_actor = actor.clone();
            let turn = tokio::task::spawn_local(
                crate::session::image_describe::TEST_LOCAL_OCR_PROGRAM.scope(
                    fixture.path().join("tesseract"),
                    async move {
                        admit_inline_query(&turn_actor, "cancel-ocr", "Continue.".into()).await;
                    },
                ),
            );
            let pid = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if let Ok(value) = std::fs::read_to_string(fixture.path().join("pid"))
                        && let Ok(pid) = value.parse::<u32>()
                    {
                        break pid;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("OCR child must be running before cancellation");
            {
                let mut state = actor.state.lock().await;
                let ForegroundState::RegularTurn(task) = &mut state.foreground else {
                    panic!("live OCR owner");
                };
                // This direct handle_prompt fixture normally uses a sleeper owner.
                // Stop must abort the actual producer; join it below as well.
                task.handle.abort();
                task.handle = turn.abort_handle();
            }
            actor
                .cancel_running_task(false, false, false, Some("esc".into()))
                .await
                .unwrap();
            assert!(turn.await.unwrap_err().is_cancelled());
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if !tokio::process::Command::new("kill")
                        .args(["-0", &pid.to_string()])
                        .output()
                        .await
                        .unwrap()
                        .status
                        .success()
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("cancelling fallback must terminate and reap its OCR process");
            assert_eq!(server.requests().len(), 2);
            assert_eq!(
                server.requests()[1].body.as_ref().unwrap()["model"],
                "vision-model"
            );
            crate::session::image_describe::TEST_LOCAL_OCR_CALLS
                .with(|calls| assert_eq!(calls.get(), 1));
            assert!(persistence.attempted_projections().is_empty());
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(
                    &actor.chat_state_handle.get_conversation().await
                )
                .len(),
                1
            );
        }));
    });
}

#[tokio::test(flavor = "current_thread")]
async fn side_question_recovers_images_on_all_backends_and_reuses_text() {
    tokio::task::LocalSet::new().run_until(async {
        for backend in [sampling_types::ApiBackend::Messages, sampling_types::ApiBackend::ChatCompletions, sampling_types::ApiBackend::Responses] {
            for tool_image in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let path = match backend {
                    sampling_types::ApiBackend::Messages => "/v1/messages",
                    sampling_types::ApiBackend::ChatCompletions => "/v1/chat/completions",
                    sampling_types::ApiBackend::Responses => "/v1/responses",
                };
                server.enqueue_response(path, ScriptedResponse::json(400, json!({
                    "error": {"type": "invalid_request_error", "message": "1210: messages.content.type 参数非法，取值范围 ['text']"}
                })));
                server.enqueue_response("/v1/messages", messages_text_turn("Screenshot error E42.", "vision-model"));
                server.set_response("The screenshot shows E42.");
                let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, Some("vision")).await;
                let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                config.api_backend = backend.clone();
                actor.chat_state_handle.replace_sampling_route(config);
                let image = inline_data_uri(&test_image_content());
                let history = if tool_image {
                    vec![ConversationItem::user("source context"), ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                        id: "read_image".into(), name: "read_file".into(), arguments: "{}".into(),
                    }]), ConversationItem::tool_result_with_images("read_image", "tool evidence", vec![sampling_types::ContentPart::Image { url: image.into(), description: None }])]
                } else {
                    let mut user = ConversationItem::user("source context");
                    user.add_image(image);
                    vec![user]
                };
                replace_test_surface(&actor.chat_state_handle, history).await;
                assert_eq!(actor.handle_side_question("What happened?").await.unwrap(), "The screenshot shows E42.");
                assert_eq!(actor.handle_side_question("Explain E42").await.unwrap(), "The screenshot shows E42.");
                let requests = server.requests();
                assert_eq!(requests.len(), 4, "{backend:?}, tool={tool_image}");
                assert_eq!(count_wire_images(requests[0].body.as_ref().unwrap()), 1);
                let auxiliary = requests[1].body.as_ref().unwrap();
                assert_eq!(auxiliary["model"], "vision-model");
                assert_eq!(count_wire_images(auxiliary), 1);
                assert!(!auxiliary.to_string().contains("source context"));
                assert!(!auxiliary.to_string().contains("What happened?"));
                for request in &requests[2..] {
                    let body = request.body.as_ref().unwrap();
                    assert_eq!(count_wire_images(body), 0);
                    assert!(body.to_string().contains("Screenshot error E42."));
                    if tool_image { assert!(body.to_string().contains("tool evidence")); }
                }
                let conversation = actor.chat_state_handle.get_conversation().await;
                let wire = serde_json::to_string(&conversation).unwrap();
                assert!(!wire.contains("What happened?"));
                assert!(!wire.contains("The screenshot shows E42."));
                assert_eq!(sampling_types::conversation::conversation_image_groups(&conversation).len(), 1, "description retains original media");
            }
        }
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn side_question_image_failure_does_not_send_unacknowledged_or_unrelated_retries() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for projection_failure in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let message = if projection_failure {
                    "1210: messages.content.type 参数非法，取值范围 ['text']"
                } else {
                    "invalid image dimensions"
                };
                server.enqueue_response(
                    "/v1/messages",
                    ScriptedResponse::json(
                        400,
                        json!({"error": {"type": "invalid_request_error", "message": message}}),
                    ),
                );
                server.enqueue_response(
                    "/v1/messages",
                    messages_text_turn("Screenshot E42", "vision-model"),
                );
                let (actor, _gateway, persistence) =
                    actor_with_sampler_and_persistence(&server, Some("vision")).await;
                let mut user = ConversationItem::user("source context");
                user.add_image(inline_data_uri(&test_image_content()));
                replace_test_surface(&actor.chat_state_handle, vec![user]).await;
                let before =
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap();
                let prepared = actor.prepare_chat_completion_config(false).await;
                let prepared_key = sampling_types::model_image_input_key_from_parts(
                    &prepared.model,
                    &prepared.api_backend,
                    &prepared.base_url,
                    &prepared.query_params,
                );
                if projection_failure {
                    persistence.fail_projection_commits();
                }
                assert!(actor.handle_side_question("What happened?").await.is_err());
                assert_eq!(
                    server.requests().len(),
                    if projection_failure { 2 } else { 1 }
                );
                assert_eq!(
                    actor.model_image_input_is_unsupported(&prepared_key).await,
                    projection_failure
                );
                let conversation = actor.chat_state_handle.get_conversation().await;
                if projection_failure {
                    assert!(
                        conversation.is_empty(),
                        "failed durable write closes chat-state reads"
                    );
                } else {
                    assert_eq!(serde_json::to_value(&conversation).unwrap(), before);
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn side_question_image_rejection_does_not_project_a_newer_surface() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.enqueue_response("/v1/messages", multimodal_capability_rejection());
            let (actor, _gateway, persistence) =
                actor_with_sampler_and_persistence(&server, Some("vision")).await;
            let mut user = ConversationItem::user("first image");
            user.add_image(inline_data_uri(&test_image_content()));
            replace_test_surface(&actor.chat_state_handle, vec![user.clone()]).await;
            persistence.hold_next_sideband_attempt();
            let task_actor = actor.clone();
            let question = tokio::task::spawn_local(async move {
                task_actor.handle_side_question("Explain").await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !persistence.sideband_ack_is_pending() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let mut new_image = ConversationItem::user("new image outside frozen request");
            new_image.add_image(inline_data_uri(&test_image_content()));
            replace_test_surface(&actor.chat_state_handle, vec![user, new_image]).await;
            persistence.release_sideband_ack();
            assert!(question.await.unwrap().is_err());
            assert_eq!(server.requests().len(), 1);
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                2
            );
            assert!(conversation.iter().all(|item| {
                sampling_types::conversation::item_image_description(item).is_none()
            }));
        })
        .await;
}
