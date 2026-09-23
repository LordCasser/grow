use super::support::*;
use super::*;
use tools::implementations::use_tool::UseTool;
use tools::registry::types::FinalizedToolset;
use tools::types::output::{MCPOutput, ToolOutput};
use tools::types::tool::{ToolKind, ToolNamespace};
use tools::types::tool_metadata::ToolMetadata;

#[derive(Debug)]
struct ImageMcpTool;

fn valid_mcp_image() -> tools::util::base64_images::ExtractedImage {
    use base64::Engine as _;

    let pixels: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        image::ImageBuffer::from_fn(128, 128, |x, y| {
            let n = x.wrapping_mul(131_071).wrapping_add(y.wrapping_mul(65_537));
            let n = n ^ n.rotate_left(13);
            image::Rgba([n as u8, (n >> 8) as u8, (n >> 16) as u8, 255])
        });
    let mut png = Vec::new();
    pixels
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    tools::util::base64_images::admit_typed_image("image/png", &encoded, 0).unwrap()
}

impl tool_runtime::Tool for ImageMcpTool {
    type Args = serde_json::Value;
    type Output = ToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("server__image").unwrap()
    }

    fn description(&self, _: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new("server__image", "test MCP image")
    }

    async fn run(
        &self,
        _: tool_runtime::ToolCallContext,
        _: serde_json::Value,
    ) -> Result<ToolOutput, tool_runtime::ToolError> {
        Ok(ToolOutput::MCP(
            MCPOutput::okay_output(
                "image".to_owned(),
                "server".to_owned(),
                format!(
                    "before {} [image content will be provided separately] after",
                    "long ".repeat(5_000)
                ),
            )
            .with_extracted_images(vec![valid_mcp_image()]),
        ))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn non_mcp_text_result_still_extracts_inline_data_uri() {
    use tools::types::output::{TextOutput, ToolRunResult};

    let encoded = valid_mcp_image().data;
    assert!(encoded.len() >= 1024);
    let prompt = format!("before ![plot](data:image/png;base64,{encoded}) after");

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "text-image-1".into(),
                    name: "text_tool".into(),
                    arguments: "{}".into(),
                }]),
            );
            let followups = actor
                .handle_bridge_tool_success(
                    &acp::ToolCallId::new("text-image-1"),
                    "text-image-1",
                    "text_tool",
                    "text_tool",
                    ToolRunResult {
                        output: ToolOutput::Text(TextOutput::from(prompt.clone())),
                        prompt_text: prompt,
                        effective_tool_name: None,
                    },
                    0,
                    "test-model",
                    &serde_json::json!({}),
                )
                .await
                .unwrap();
            let conversation = actor.chat_state_handle.get_conversation().await;
            let ConversationItem::ToolResult(result) = conversation.last().unwrap() else {
                panic!("expected tool result");
            };
            assert!(result.content.contains("before"));
            assert!(!result.content.contains("data:image/png;base64"));
            assert_eq!(followups.len(), 1);
            assert!(matches!(
                &followups[0],
                ConversationItem::User(user) if user.content.iter().any(|part| matches!(
                    part,
                    sampling_types::conversation::ContentPart::Image { .. }
                ))
            ));
        })
        .await;
}

impl ToolMetadata for ImageMcpTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::MCP
    }

    fn description_template(&self) -> &str {
        "test MCP image"
    }
}

#[tokio::test(flavor = "current_thread")]
async fn use_tool_image_reaches_shell_timeline_followup_without_raw_base64() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let toolset = std::sync::Arc::new(FinalizedToolset::empty_for_test());
            toolset
                .register_tool("server__image".to_owned(), ImageMcpTool, None)
                .unwrap();
            toolset
                .register_tool("use_tool".to_owned(), UseTool, None)
                .unwrap();
            let run = toolset
                .call(
                    "use_tool",
                    serde_json::json!({"tool_name":"server__image","tool_input":{}}),
                    "mcp-image-1",
                    None,
                )
                .await
                .unwrap();
            let ToolOutput::MCP(ref mcp) = run.output else {
                panic!("use_tool must return MCP output");
            };
            assert_eq!(mcp.extracted_images().len(), 1);
            let image_data = mcp.extracted_images()[0].data.clone();
            assert!(run.prompt_text.contains("truncated"));
            assert!(
                !serde_json::to_string(&run.output)
                    .unwrap()
                    .contains(&image_data)
            );

            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.chat_state_handle.push_assistant_response(
                ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                    id: "mcp-image-1".into(),
                    name: "use_tool".into(),
                    arguments: "{}".into(),
                }]),
            );
            let followups = actor
                .handle_bridge_tool_success(
                    &acp::ToolCallId::new("mcp-image-1"),
                    "mcp-image-1",
                    "use_tool",
                    "server__image",
                    run,
                    0,
                    "test-model",
                    &serde_json::json!({}),
                )
                .await
                .unwrap();
            let conversation = actor.chat_state_handle.get_conversation().await;
            let ConversationItem::ToolResult(result) = conversation.last().unwrap() else {
                panic!("expected tool result");
            };
            assert!(result.content.contains("before"));
            assert!(!result.content.contains("data:image"));
            assert_eq!(followups.len(), 1);
            let ConversationItem::User(image_followup) = &followups[0] else {
                panic!("expected image follow-up");
            };
            assert!(image_followup.content.iter().any(|part| matches!(
                part,
                sampling_types::conversation::ContentPart::Image { .. }
            )));
            for followup in followups {
                actor.chat_state_handle.push_user_message(followup);
            }
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(matches!(
                conversation.last(),
                Some(ConversationItem::User(user)) if user.content.iter().any(|part| matches!(
                    part,
                    sampling_types::conversation::ContentPart::Image { .. }
                ))
            ));
            let replay = chat_state::Timeline::from_events(
                actor.chat_state_handle.timeline_events().await.unwrap(),
            )
            .unwrap();
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(replay.surface()).len(),
                1
            );
            let request = actor
                .chat_state_handle
                .build_request(&actor.session_info.id.to_string(), vec![], None, None, None)
                .await
                .unwrap();
            assert_eq!(request.image_count(), 1);

            // Reopen the canonical on-disk session through the production
            // storage reader, not only an in-memory Timeline clone.
            use crate::session::storage::StorageAdapter as _;
            let root = tempfile::tempdir().unwrap();
            let writer =
                crate::session::storage::JsonlStorageAdapter::with_root(root.path().to_path_buf());
            writer
                .init_session(
                    &actor.session_info,
                    crate::session::persistence::default_model_id(),
                )
                .await
                .unwrap();
            for event in replay.events() {
                writer
                    .append_timeline_event_durable(&actor.session_info, event)
                    .await
                    .unwrap();
            }
            drop(writer);
            let reopened = crate::session::storage::load_timeline_by_id_at(
                actor.session_info.id.0.as_ref(),
                root.path(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                sampling_types::conversation::conversation_image_groups(reopened.surface()).len(),
                1
            );
            let portable =
                sampling_types::conversation::project_portable_history(reopened.surface());
            let portable_images =
                sampling_types::conversation::conversation_image_groups(&portable);
            assert_eq!(portable_images.len(), 1);
            assert_eq!(portable_images[0].image_count(), 1);
        })
        .await;
}
