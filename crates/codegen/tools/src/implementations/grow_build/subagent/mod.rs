//! Dedicated subagent enumeration, status and completion tools.
mod status;
mod wait;
use crate::implementations::grow_build::task::backend::SubagentBackendResource;
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::ToolMetadata;
pub use status::GetSubagentStatusTool;
use tool_types::{ListSubagentInput, ListSubagentOutput, SubagentWaitResult};
pub use wait::WaitSubagentTool;

#[derive(Debug, Default)]
pub struct ListSubagentTool;
impl ToolMetadata for ListSubagentTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SubagentList
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }
    fn description_template(&self) -> &str {
        "List subagents visible to this session. Omit scope for active children, including initializing, running, suspended, finalizing and unknown children. Set scope=all when you need historical children. Each row includes the child's current observed state, ID, description and times. Unknown state does not prove cleanup. ${%- if tools.by_kind.subagent_status %} Use ${{ tools.by_kind.subagent_status }} with known IDs for details.${%- endif %}${%- if tools.by_kind.subagent_wait %} Use ${{ tools.by_kind.subagent_wait }} to wait for completion.${%- endif %}"
    }
}
impl tool_runtime::Tool for ListSubagentTool {
    type Args = ListSubagentInput;
    type Output = ListSubagentOutput;
    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("list_subagent").unwrap()
    }
    fn description(&self, _: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new("list_subagent", self.sanitized_description_template())
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
        input: ListSubagentInput,
    ) -> Result<Self::Output, tool_runtime::ToolError> {
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        tokio::time::timeout_at(deadline, async {
            let backend = resources
                .lock()
                .await
                .require::<SubagentBackendResource>()?
                .clone();
            let subagents = backend
                .backend()
                .list_subagents(input.scope, deadline)
                .await
                .map_err(observation_tool_error)?;
            Ok(ListSubagentOutput {
                scope: input.scope,
                subagents,
            })
        })
        .await
        .map_err(|_| {
            tool_runtime::ToolError::custom(
                "observation_timed_out",
                "Subagent enumeration timed out; no absence was confirmed",
            )
        })?
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetSubagentResultInput {
    /// The single subagent ID returned by delegation or list_subagent.
    pub subagent_id: String,
}

#[derive(Debug, Default)]
pub struct GetSubagentResultTool;

impl ToolMetadata for GetSubagentResultTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Retrieve the verified final answer of one known subagent by subagent_id, including after resume or an earlier delivery. Returns its observed state and any available answer immediately without waiting for the child to finish."
    }
}

impl tool_runtime::Tool for GetSubagentResultTool {
    type Args = GetSubagentResultInput;
    type Output = SubagentWaitResult;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("get_subagent_result").unwrap()
    }

    fn description(&self, _: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "get_subagent_result",
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
        use crate::implementations::grow_build::task::types::SubagentObservationError;

        let id = input.subagent_id;
        if id.trim().is_empty() {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "Provide one subagent_id.",
            ));
        }
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let backend = resources
                .lock()
                .await
                .require::<SubagentBackendResource>()?
                .clone();
            Ok::<_, tool_runtime::ToolError>(backend.backend().query(&id, false, None).await)
        })
        .await;
        Ok(match observed {
            Ok(result) => match result? {
                Ok(Some(snapshot)) => wait::from_snapshot(snapshot),
                Ok(None) => wait::wait_error(id, SubagentObservationError::Unavailable),
                Err(error) => wait::wait_error(id, error),
            },
            Err(_) => wait::wait_error(id, SubagentObservationError::TimedOut),
        })
    }
}

fn observation_tool_error(
    error: crate::implementations::grow_build::task::types::SubagentObservationError,
) -> tool_runtime::ToolError {
    use crate::implementations::grow_build::task::types::SubagentObservationError::*;
    let code = match error {
        Unavailable => "observation_unavailable",
        TimedOut => "observation_timed_out",
        InvalidEvidence => "observation_invalid_evidence",
        Unauthorized => "unauthorized",
    };
    tool_runtime::ToolError::custom(code, error.to_string())
}

#[cfg(test)]
mod wait_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::grow_build::task::backend::ChannelBackend;
    use crate::types::{resources::Resources, tool_metadata::test_ctx};
    use std::{sync::Arc, time::Duration};
    use tool_types::{SubagentListScope, SubagentStatus, SubagentSummary};

    #[tokio::test(start_paused = true)]
    async fn public_list_queue_and_runtime_share_deadline_without_losing_history() {
        let (tx, _silent_runtime) = tokio::sync::mpsc::unbounded_channel();
        let backend = ChannelBackend::for_session(tx, "parent").with_history_list(|_| async {
            Ok(vec![SubagentSummary {
                subagent_id: "historical".into(),
                direct_parent_session_id: "parent".into(),
                subagent_type: "explore".into(),
                description: "original child".into(),
                status: SubagentStatus::Completed,
                started_at_epoch_ms: Some(1),
                duration_ms: Some(100),
                error: None,
            }])
        });
        let mut resources = Resources::new();
        resources.insert(SubagentBackendResource(Arc::new(backend)));
        let resources = resources.into_shared();
        let guard = resources.lock().await;
        let start = tokio::time::Instant::now();
        let (output, ()) = tokio::join!(
            tool_runtime::Tool::run(
                &ListSubagentTool,
                test_ctx(resources.clone()),
                ListSubagentInput {
                    scope: SubagentListScope::All
                }
            ),
            async {
                tokio::time::sleep(Duration::from_secs(4)).await;
                drop(guard);
            },
        );
        let output =
            output.expect("history survives an unavailable runtime within the original deadline");
        assert_eq!(start.elapsed(), Duration::from_secs(5));
        assert_eq!(output.subagents.len(), 1);
        assert_eq!(output.subagents[0].subagent_id, "historical");
        assert_eq!(output.subagents[0].status, SubagentStatus::Completed);
    }
}
