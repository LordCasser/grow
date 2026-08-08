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
            actor.image_description_model = Some("missing-vision-model".to_owned());

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
            actor.image_description_model = Some("missing-vision-model".to_owned());
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
