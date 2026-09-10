//! Direct parent-child communication; caller identity is supplied by ChannelBackend.
use super::backend::SubagentBackendResource;
use crate::types::tool::{ToolKind, ToolNamespace};
use tool_runtime::{ToolCallContext, ToolError};

#[derive(Debug, Clone)]
pub enum AgentInteraction {
    Ask { question: String },
    Send { message: String, interrupt: bool },
}

pub struct AgentInteractionRequest {
    pub source_session_id: String,
    /// None means the caller's immediate parent, never an arbitrary session.
    pub target_child_id: Option<String>,
    pub id: String,
    pub action: AgentInteraction,
    pub cancellation: tokio_util::sync::CancellationToken,
    pub respond_to: tokio::sync::oneshot::Sender<Result<AgentInteractionOutput, String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AgentInteractionOutput {
    pub id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl tool_runtime::ToolOutput for AgentInteractionOutput {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskParentInput {
    pub question: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskSubagentInput {
    pub subagent_id: String,
    pub question: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendSubagentMessageInput {
    pub subagent_id: String,
    pub message: String,
    /// true soft-preempts sampling/interruptible waits; false queues for the next step.
    pub interrupt: bool,
}

async fn interact(
    ctx: ToolCallContext,
    target: Option<String>,
    action: AgentInteraction,
) -> Result<AgentInteractionOutput, ToolError> {
    let body = match &action {
        AgentInteraction::Ask { question } => question,
        AgentInteraction::Send { message, .. } => message,
    };
    if body.trim().is_empty() || body.len() > 16 * 1024 {
        return Err(ToolError::custom(
            "invalid_message",
            "Message must be nonempty and at most 16384 bytes.",
        ));
    }
    let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
    let backend = resources
        .lock()
        .await
        .get::<SubagentBackendResource>()
        .cloned()
        .ok_or_else(|| {
            ToolError::custom("subagent_unavailable", "Subagent runtime unavailable.")
        })?;
    let cancellation = ctx
        .get::<tool_runtime::Cancellation>()
        .map_or_else(tokio_util::sync::CancellationToken::new, |token| {
            token.0.clone()
        });
    backend
        .backend()
        .interact(ctx.call_id.to_string(), target, action, cancellation)
        .await
        .map_err(|error| ToolError::custom("agent_interaction_failed", error))
}

macro_rules! interaction_tool {
    ($tool:ident, $input:ty, $id:literal, $description:literal, $access:ident, $convert:expr) => {
        #[derive(Debug, Default)]
        pub struct $tool;
        impl crate::types::tool_metadata::ToolMetadata for $tool {
            fn kind(&self) -> ToolKind {
                ToolKind::Other
            }
            fn tool_namespace(&self) -> ToolNamespace {
                ToolNamespace::Grow
            }
            fn description_template(&self) -> &str {
                $description
            }
        }
        impl tool_runtime::Tool for $tool {
            type Args = $input;
            type Output = AgentInteractionOutput;
            fn id(&self) -> tool_protocol::ToolId {
                tool_protocol::ToolId::new($id).expect("valid tool id")
            }
            fn description(
                &self,
                _: &tool_runtime::ListToolsContext,
            ) -> tool_types::ToolDescription {
                tool_types::ToolDescription::new($id, $description)
            }
            fn capabilities(&self) -> tool_protocol::ToolCapabilities {
                tool_protocol::ToolCapabilities {
                    max_access: tool_protocol::ToolAccess::$access,
                    ..Default::default()
                }
            }
            async fn run(
                &self,
                ctx: ToolCallContext,
                input: Self::Args,
            ) -> Result<Self::Output, ToolError> {
                let (target, action) = ($convert)(input);
                interact(ctx, target, action).await
            }
        }
    };
}
interaction_tool!(
    AskParentTool,
    AskParentInput,
    "ask_parent",
    "Ask your immediate delegating parent a question. It answers asynchronously from frozen context in one tool-free sideband call, without interrupting its work. This does not send instructions or start parent work.",
    Read,
    |input: AskParentInput| (
        None,
        AgentInteraction::Ask {
            question: input.question
        }
    )
);
interaction_tool!(
    AskSubagentTool,
    AskSubagentInput,
    "ask_subagent",
    "Ask a directly-owned running subagent a question. It answers from frozen context in one tool-free sideband call without interrupting its task. Use send_subagent_message to change its instructions.",
    Read,
    |input: AskSubagentInput| (
        Some(input.subagent_id),
        AgentInteraction::Ask {
            question: input.question
        }
    )
);
interaction_tool!(
    SendSubagentMessageTool,
    SendSubagentMessageInput,
    "send_subagent_message",
    "Send instructions to a directly-owned running subagent. interrupt=true safely preempts its active model request or interruptible wait; interrupt=false queues context for its next step. Existing non-interruptible tool operations finish safely. Success acknowledges durable receipt, not task completion. Cannot send upward or to another session.",
    Write,
    |input: SendSubagentMessageInput| (
        Some(input.subagent_id),
        AgentInteraction::Send {
            message: input.message,
            interrupt: input.interrupt
        }
    )
);
