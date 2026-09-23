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
        .chat_state_handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            sampling_types::ToolCall {
                id: "read-image-1".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        ]));
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
    actor
        .record_unsupported_model_image_input(actor.current_catalog_model_id())
        .await
        .unwrap();
}

async fn test_actor() -> std::sync::Arc<SessionActor> {
    let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    std::sync::Arc::new(create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await)
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

#[test]
fn known_text_only_model_removes_read_file_image_before_sampling() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let actor = test_actor().await;
            mark_current_model_as_text_only(&actor).await;

            let raw_result = run_image_result(&actor).await;
            assert_eq!(
                raw_result.images.len(),
                1,
                "read_file must keep ImageContent"
            );

            let report = actor
                .project_images_for_known_text_model()
                .await
                .expect("the pre-sampling gate must repair the Surface, not fail");
            assert_eq!(report.described_images, 0);
            assert_eq!(report.removed_images, 1);

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                sampling_types::conversation::conversation_image_groups(&conversation).is_empty()
            );
            let ConversationItem::ToolResult(result) = conversation.last().unwrap() else {
                panic!("expected tool result");
            };
            assert_eq!(result.tool_call_id, "read-image-1");
            assert!(result.images.is_empty());
            assert_eq!(
                result.content.as_ref(),
                format!(
                    "Read image file.\n\n[Projected image removal]\n{}",
                    sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
                )
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|event| matches!(
                event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )));
            let sealed_message_groups = events
                .iter()
                .filter_map(|event| event.messages())
                .map(|messages| {
                    sampling_types::conversation::conversation_image_groups(&messages.items).len()
                })
                .sum::<usize>();
            assert_eq!(
                sealed_message_groups, 1,
                "the raw read_file payload must stay as Timeline evidence"
            );

            let request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert!(
                sampling_types::conversation::conversation_image_groups(&request.items).is_empty(),
                "the repaired Surface must assemble a request without images"
            );
            assert!(request.items.iter().any(|item| {
                item.text_content()
                    .contains(sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT)
            }));
        }));
    });
}

/// Compaction recursion needs a bigger stack than the default test thread.
fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

#[test]
fn compaction_proceeds_after_an_acknowledged_removal_projection() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
            let actor = Arc::new(test_actor().await);
            mark_current_model_as_text_only(&actor).await;
            run_image_result(&actor).await;

            let report = actor
                .project_images_for_known_text_model()
                .await
                .expect("the image gate must remove the unresolved image durably");
            assert_eq!(report.removed_images, 1);

            let timeline_events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(
                timeline_events.iter().any(|event| matches!(
                    event.kind,
                    chat_state::TimelineEventKind::ImageProjection(_)
                )),
                "the removal must be an accepted Timeline fact"
            );
            assert!(
                timeline_events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Messages(messages)
                        if !sampling_types::conversation::conversation_image_groups(
                            &messages.items
                        )
                        .is_empty()
                )),
                "the original image-bearing message stays as evidence"
            );

            // Compaction is no longer refused because of the image; it proceeds
            // past the gate into its own transaction. This harness has no
            // compaction provider, so any later failure is not image-related.
            if let Err(error) = actor.run_compact(None).await {
                assert!(
                    !format!("{error:?}").contains("failed to persist text-only image projection"),
                    "{error:?}"
                );
            }
            assert!(
                actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .iter()
                    .any(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Compaction(_)
                    )),
                "compaction must reach its transaction boundary after the removal"
            );

            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                sampling_types::conversation::conversation_image_groups(&conversation).is_empty(),
                "no compaction path may resurrect the removed image"
            );
            assert!(conversation.iter().any(|item| {
                item.text_content()
                    .contains(sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT)
            }));
        }));
    });
}

#[test]
fn pdf_extracted_images_stay_one_ordered_group_and_only_the_text_route_is_projected() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        runtime.block_on(local.run_until(async {
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

            actor.chat_state_handle.push_assistant_response(ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                id: "read-pdf-1".into(), name: "read_file".into(), arguments: "{}".into(),
            }]));
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
            let report = actor
                .project_images_for_known_text_model()
                .await
                .expect("PDF images are removed only through a durable projection");
            assert_eq!(report.removed_images, 2);
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                sampling_types::conversation::conversation_image_groups(&conversation).is_empty()
            );
            let replacement_count = conversation
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
            assert_eq!(
                replacement_count, 1,
                "one replacement covers the whole ordered group"
            );
            assert!(conversation.iter().any(|item| {
                matches!(item, ConversationItem::ToolResult(result) if result.content.contains("PDF text"))
            }));

            let text_request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert!(
                sampling_types::conversation::conversation_image_groups(&text_request.items)
                    .is_empty()
            );
            assert!(
                text_request
                    .items
                    .iter()
                    .any(|item| item.text_content().contains(
                        sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
                    ))
            );

            let mut vision_config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            vision_config.model = "vision-model".to_owned();
            actor.chat_state_handle.replace_sampling_route(vision_config);
            let vision_request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert!(
                sampling_types::conversation::conversation_image_groups(&vision_request.items)
                    .is_empty(),
                "another model must not resurrect a durably removed image"
            );
            assert!(
                vision_request
                    .items
                    .iter()
                    .any(|item| item.text_content().contains(
                        sampling_types::conversation::UNSUPPORTED_IMAGE_REPLACEMENT
                    ))
            );
        }));
    });
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

            let catalog = SessionActor::published_catalog_for_test(
                crate::agent::models::ModelId::new("provider/reloaded"),
                sampling,
                Some("provider/vision".into()),
                std::time::Duration::from_secs(77),
                2,
                73,
            );
            let (responds_to, response) = tokio::sync::oneshot::channel();
            actor.admit_model_catalog_reload(catalog, responds_to).await;
            response.await.unwrap().unwrap();

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
            let catalog = SessionActor::published_catalog_for_test(
                crate::agent::models::ModelId::new("provider/deferred"),
                sampling,
                None,
                std::time::Duration::from_secs(88),
                3,
                71,
            );
            let (responds_to, mut response) = tokio::sync::oneshot::channel();

            actor.admit_model_catalog_reload(catalog, responds_to).await;

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
            actor.apply_pending_step_controls_if_idle().await;
            response.await.unwrap().unwrap();
            let applied = actor.chat_state_handle.get_sampling_config().await.unwrap();
            assert_eq!(applied.model, "deferred-model");
            assert_eq!(actor.inference_idle_timeout.get().as_secs(), 88);
            assert_eq!(actor.max_retries.get(), 3);
            assert_eq!(actor.compaction.threshold_percent.get(), 71);
        })
        .await;
}
