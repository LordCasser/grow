use super::*;
use crate::implementations::grow_build::task::types::SubagentObservationError;
use tool_types::{
    GetSubagentStatusInput, GetSubagentStatusOutput, SubagentDetails, SubagentStatus,
};

#[derive(Debug, Default)]
pub struct GetSubagentStatusTool;
impl ToolMetadata for GetSubagentStatusTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SubagentStatus
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }
    fn description_template(&self) -> &str {
        "Get detailed status for one or more known subagent IDs. Returns execution state, duration, tool-call/turn counts and available usage from the current sample or verified history. Unknown fields are null. Does not wait, read final answers, consume completion notifications or inspect command tasks. ${%- if tools.by_kind.subagent_list %} Use ${{ tools.by_kind.subagent_list }} to find IDs.${%- endif %}${%- if tools.by_kind.subagent_wait %} Use ${{ tools.by_kind.subagent_wait }} to await or retrieve the final result.${%- endif %}"
    }
}
impl tool_runtime::Tool for GetSubagentStatusTool {
    type Args = GetSubagentStatusInput;
    type Output = GetSubagentStatusOutput;
    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("get_subagent_status").unwrap()
    }
    fn description(&self, _: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "get_subagent_status",
            self.sanitized_description_template(),
        )
    }
    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::Read,
            ..Default::default()
        }
    }
    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: Self::Args,
    ) -> Result<Self::Output, tool_runtime::ToolError> {
        let ids = tool_types::resolve_task_ids(&input.subagent_ids);
        if ids.is_empty() || ids.len() > tool_types::MAX_MULTI_WAIT_IDS {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "Provide 1 to 20 subagent_ids.",
            ));
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = match tokio::time::timeout_at(deadline, resources.lock()).await {
            Ok(res) => res.require::<SubagentBackendResource>()?.clone(),
            Err(_) => return Ok(status_error(&ids, SubagentObservationError::TimedOut)),
        };
        match tokio::time::timeout_at(deadline, backend.backend().get_statuses(&ids, deadline))
            .await
        {
            Ok(Ok(subagents)) => Ok(GetSubagentStatusOutput { subagents }),
            Ok(Err(error)) => Ok(status_error(&ids, error)),
            Err(_) => Ok(status_error(&ids, SubagentObservationError::TimedOut)),
        }
    }
}
fn status_error(ids: &[String], error: SubagentObservationError) -> GetSubagentStatusOutput {
    let status = match error {
        SubagentObservationError::Unavailable => SubagentStatus::ObservationUnavailable,
        SubagentObservationError::TimedOut => SubagentStatus::ObservationTimedOut,
        SubagentObservationError::InvalidEvidence => SubagentStatus::ObservationInvalidEvidence,
        SubagentObservationError::Unauthorized => SubagentStatus::Unauthorized,
    };
    GetSubagentStatusOutput {
        subagents: ids
            .iter()
            .map(|id| SubagentDetails::unknown(id.clone(), status, error.to_string()))
            .collect(),
    }
}
