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

#[tokio::test(flavor = "current_thread")]
async fn unconfigured_image_description_keeps_main_model_multimodal_path() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            let result = run_image_result(&actor).await;

            assert_eq!(
                result.content.as_ref(),
                "Read image file: /workspace/image.png"
            );
            assert_eq!(result.images.len(), 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn configured_image_description_never_silently_falls_back_to_main_model() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            *actor.image_description_model.write() = Some("missing-vision-model".to_owned());

            let result = run_image_result(&actor).await;

            assert!(result.images.is_empty());
            assert!(
                result
                    .content
                    .contains("Configured image description failed")
            );
            assert!(result.content.contains("could not be resolved"));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn configured_image_description_owns_images_extracted_from_file_content() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            *actor.image_description_model.write() = Some("missing-vision-model".to_owned());
            let image = test_image_content();
            let result = ToolRunResult {
                output: ToolOutput::ReadFile(ReadFileOutput::FileContent(FileContent {
                    content: "1→PDF text".to_owned(),
                    content_concise: None,
                    absolute_path: "/workspace/mixed.pdf".into(),
                    offset: None,
                    limit: None,
                    raw_output: "PDF text".to_owned(),
                    total_lines: 1,
                    extracted_images: vec![tools::util::base64_images::ExtractedImage {
                        data: image.data,
                        mime_type: image.mime_type,
                    }],
                })),
                prompt_text: "1→PDF text".to_owned(),
                effective_tool_name: None,
            };

            actor
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

            let conversation = actor.chat_state_handle.get_conversation().await;
            let ConversationItem::ToolResult(result) = conversation.last().unwrap() else {
                panic!("expected tool result");
            };
            assert!(result.images.is_empty());
            assert!(result.content.contains("1→PDF text"));
            assert!(
                result
                    .content
                    .contains("Configured image description failed")
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn live_model_reload_updates_every_next_turn_sampler_knob() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
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
                .await;

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
