use super::support::*;
use super::*;
use tools::types::output::{
    FileContent, ImageContent as ToolImageContent, ReadFileOutput, ToolOutput,
};

fn image_tool_result() -> ToolRunResult {
    let image = test_image_content();
    ToolRunResult {
        output: ToolOutput::ReadFile(ReadFileOutput::ImageContent(ToolImageContent {
            data: image.data,
            mime_type: image.mime_type,
            annotations: None,
            uri: None,
            meta: None,
        })),
        prompt_text: "[image inline]".to_owned(),
        effective_tool_name: None,
    }
}

async fn run_image_result(actor: &SessionActor) -> sampling_types::conversation::ToolResultItem {
    actor
        .handle_bridge_tool_success(
            &acp::ToolCallId::new("read-image-1"),
            "read-image-1",
            "read_file",
            "read_file",
            image_tool_result(),
            0,
            "test-model",
            &serde_json::json!({"target_file": "/workspace/image.png"}),
        )
        .await
        .unwrap();
    let conversation = actor.chat_state_handle.get_conversation().await;
    match conversation.last().unwrap() {
        ConversationItem::ToolResult(result) => result.clone(),
        other => panic!("expected tool result, got {other:?}"),
    }
}

async fn mark_current_model_as_text_only(actor: &SessionActor) {
    let key = actor.current_model_image_input_key().await.unwrap();
    actor
        .record_unsupported_model_image_input(key)
        .await
        .unwrap();
}

async fn test_actor() -> SessionActor {
    let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await
}

#[tokio::test(flavor = "current_thread")]
async fn configured_auxiliary_does_not_preempt_unknown_current_model() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = test_actor().await;
            *actor.image_description_model.write() = Some("missing-vision-model".to_owned());

            let result = run_image_result(&actor).await;

            assert_eq!(result.content.as_ref(), "Read image file.");
            assert!(!result.content.contains("/workspace/image.png"));
            assert_eq!(result.images.len(), 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn known_text_only_model_degrades_read_file_image_before_sampling() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = test_actor().await;
            mark_current_model_as_text_only(&actor).await;

            let raw_result = run_image_result(&actor).await;
            assert_eq!(
                raw_result.images.len(),
                1,
                "read_file must keep ImageContent"
            );

            let error = actor
                .project_images_for_known_text_model()
                .await
                .expect_err("a permanent shadow requires a durable description");
            assert!(format!("{error:?}").contains("untranslated"));
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                1
            );
            let ConversationItem::ToolResult(result) = conversation.last().unwrap() else {
                panic!("expected tool result");
            };
            assert_eq!(result.tool_call_id, "read-image-1");
            assert_eq!(result.images.len(), 1);
            assert_eq!(result.content.as_ref(), "Read image file.");
            assert!(!result.content.contains("/workspace/image.png"));

            let request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&request.items).len(),
                1,
                "failed translation must leave the canonical model view untouched"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn compaction_refuses_to_erase_images_when_text_projection_is_unavailable() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = Arc::new(test_actor().await);
            mark_current_model_as_text_only(&actor).await;
            run_image_result(&actor).await;

            let error = actor
                .run_compact(None)
                .await
                .expect_err("compaction must fail before replacing an untranslated image");

            assert!(format!("{error:?}").contains("untranslated"));
            let timeline_events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(
                !timeline_events.iter().any(|event| matches!(
                    event.kind,
                    chat_state::TimelineEventKind::Compaction(_)
                )),
                "ImageProjection is an admission gate and must run before Compaction::Started"
            );
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                1,
                "failed projection must leave the canonical image untouched"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pdf_extracted_images_stay_one_ordered_group_and_only_the_text_route_is_projected() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = test_actor().await;
            let image = test_image_content();
            let result = ToolRunResult {
                output: ToolOutput::ReadFile(ReadFileOutput::FileContent(FileContent {
                    content: "1→PDF text".to_owned(),
                    absolute_path: "/workspace/mixed.pdf".into(),
                    offset: None,
                    limit: None,
                    raw_output: "PDF text".to_owned(),
                    total_lines: 1,
                    extracted_images: vec![
                        tools::util::base64_images::ExtractedImage {
                            data: image.data.clone(),
                            mime_type: image.mime_type.clone(),
                        },
                        tools::util::base64_images::ExtractedImage {
                            data: image.data,
                            mime_type: image.mime_type,
                        },
                    ],
                })),
                prompt_text: "1→PDF text".to_owned(),
                effective_tool_name: None,
            };

            let deferred = actor
                .handle_bridge_tool_success(
                    &acp::ToolCallId::new("read-pdf-1"),
                    "read-pdf-1",
                    "read_file",
                    "read_file",
                    result,
                    0,
                    "test-model",
                    &serde_json::json!({"target_file": "/workspace/mixed.pdf"}),
                )
                .await
                .unwrap();
            assert_eq!(deferred.len(), 1);
            let groups = sampling_types::conversation::conversation_image_groups(&deferred);
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].image_count(), 2);
            actor.chat_state_handle.push_user_message(deferred[0].clone());

            mark_current_model_as_text_only(&actor).await;
            actor
                .project_images_for_known_text_model()
                .await
                .expect_err("PDF images may not be permanently omitted");
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&conversation).len(),
                1
            );
            assert!(conversation.iter().any(|item| {
                matches!(item, ConversationItem::ToolResult(result) if result.content.contains("PDF text"))
            }));

            let text_request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&text_request.items).len(),
                1
            );

            let mut vision_config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            vision_config.model = "vision-model".to_owned();
            actor.chat_state_handle.update_sampling_config(vision_config);
            let vision_request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(&vision_request.items)
                    .len(),
                1
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn live_model_reload_updates_every_next_turn_sampler_knob() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = test_actor().await;
            let mut sampling = sampler::SamplerConfig::default();
            sampling.base_url = "https://reloaded.example/v2".into();
            sampling.model = "reloaded-model".into();
            sampling.context_window = 64_000;
            sampling.max_retries = Some(2);
            sampling
                .query_params
                .insert("deployment".into(), "next".into());

            actor
                .handle_reload_model_config(
                    acp::ModelId::new("provider/reloaded"),
                    sampling,
                    Some("provider/vision".into()),
                    std::time::Duration::from_secs(77),
                    2,
                    73,
                )
                .await
                .unwrap();

            let live = actor.chat_state_handle.get_sampling_config().await.unwrap();
            assert_eq!(live.base_url, "https://reloaded.example/v2");
            assert_eq!(live.model, "reloaded-model");
            assert_eq!(
                live.query_params.get("deployment").map(String::as_str),
                Some("next")
            );
            assert_eq!(
                actor.image_description_model.read().as_deref(),
                Some("provider/vision")
            );
            assert_eq!(actor.inference_idle_timeout.get().as_secs(), 77);
            assert_eq!(actor.max_retries.get(), 2);
            assert_eq!(actor.compaction.threshold_percent.get(), 73);
            let next_turn = actor.reconstruct_full_config().await;
            assert_eq!(next_turn.max_retries, Some(2));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn busy_model_reload_is_applied_before_the_next_idle_consumer() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = test_actor().await;
            actor.state.lock().await.foreground = ForegroundState::Compaction;
            let original_model = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .unwrap()
                .model;
            let mut sampling = sampler::SamplerConfig::default();
            sampling.base_url = "https://deferred.example/v2".into();
            sampling.model = "deferred-model".into();
            sampling.context_window = 32_000;
            let (responds_to, mut response) = tokio::sync::oneshot::channel();

            actor
                .admit_model_config_reload(
                    acp::ModelId::new("provider/deferred"),
                    sampling,
                    None,
                    std::time::Duration::from_secs(88),
                    3,
                    71,
                    responds_to,
                )
                .await;

            assert!(matches!(
                response.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            assert_eq!(
                actor
                    .chat_state_handle
                    .get_sampling_config()
                    .await
                    .unwrap()
                    .model,
                original_model,
                "the admitted foreground must retain its provider route"
            );

            actor.state.lock().await.foreground = ForegroundState::Idle;
            actor.apply_pending_model_reload_if_idle().await;
            response.await.unwrap().unwrap();
            let applied = actor.chat_state_handle.get_sampling_config().await.unwrap();
            assert_eq!(applied.model, "deferred-model");
            assert_eq!(actor.inference_idle_timeout.get().as_secs(), 88);
            assert_eq!(actor.max_retries.get(), 3);
            assert_eq!(actor.compaction.threshold_percent.get(), 71);
        })
        .await;
}
