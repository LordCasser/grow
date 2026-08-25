//! Tool argument parse-error and display preparation helpers.

use super::*;

impl SessionActor {
    pub(super) async fn handle_tool_parse_error(
        &self,
        tool_call_id: &acp::ToolCallId,
        call_id: &str,
        function_name: &str,
        err: tool_runtime::ToolError,
        raw_arguments: &str,
        model_id: &str,
    ) -> Result<(), acp::Error> {
        tracing::error!(
            session_id = %self.session_info.id.0,
            tool_name = function_name,
            model_id = model_id,
            error_kind = "parse_failure",
            error_message = %err,
            "tool_error: parse_failure"
        );
        self.signals_handle().record_tool_failure(function_name);
        let message = build_tool_parse_error_message(function_name, &err, raw_arguments);
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                tool_call_id.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(Some(acp::ToolCallStatus::Failed))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(message.clone())),
                    )])),
            )),
            None,
        )
        .await;
        let tool_chat = ConversationItem::tool_result(call_id.to_string(), message);
        self.chat_state_handle.push_tool_result(tool_chat);
        Ok(())
    }
}

/// Execute tool-call display parts. The title peels a redundant leading
/// `cd <cwd>` for chrome only; `raw_input` is serialized separately and stays full.
pub(super) fn execute_tool_call_parts(
    command: &str,
    description: Option<&str>,
    cwd: &std::path::Path,
) -> (
    String,
    acp::ToolKind,
    Vec<acp::ToolCallLocation>,
    Vec<acp::ToolCallContent>,
) {
    let display = tools::util::strip_redundant_session_cd(command, cwd);
    (
        format!("Execute `{display}`"),
        acp::ToolKind::Execute,
        Vec::new(),
        vec![acp::ToolCallContent::from(acp::ContentBlock::Text(
            acp::TextContent::new(description.unwrap_or_default().to_string()),
        ))],
    )
}
