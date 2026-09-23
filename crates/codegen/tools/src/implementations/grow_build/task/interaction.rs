//! Direct parent-child communication; caller identity is supplied by ChannelBackend.
use super::backend::SubagentBackendResource;
use crate::types::tool::{ToolKind, ToolNamespace};
use tool_runtime::{ToolCallContext, ToolError};

#[derive(Debug, Clone)]
pub enum AgentInteraction {
    Ask {
        question: String,
    },
    Send {
        message: String,
        interrupt: bool,
        reply_to: Option<sampling_types::AgentMessageRef>,
    },
}

pub struct AgentInteractionRequest {
    pub source_session_id: String,
    /// For Ask, None means the caller's immediate parent. For Send replies,
    /// the coordinator derives the candidate parent from `reply_to`.
    pub target_child_id: Option<String>,
    pub id: String,
    pub action: AgentInteraction,
    /// Runtime-owned read-only receipt verification route. The backend starts
    /// every request as `false`; the coordinator sets it only for a known
    /// direct child that is no longer eligible for new delivery.
    pub receipt_only: bool,
    pub cancellation: tokio_util::sync::CancellationToken,
    pub respond_to: tokio::sync::oneshot::Sender<Result<AgentInteractionOutput, String>>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct AgentInteractionOutput {
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    /// Coordinator-owned display identity, independent of caller-supplied text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_task_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AgentInteractionError>,
}
impl tool_runtime::ToolOutput for AgentInteractionOutput {}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct AgentInteractionError {
    pub code: String,
    pub message: String,
}

impl AgentInteractionError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

enum MessageOutcome {
    Received(String),
    Rejected(AgentInteractionError),
    Unconfirmed(AgentInteractionError),
}

impl AgentInteractionOutput {
    fn message_outcome(
        id: impl Into<String>,
        target_session_id: Option<String>,
        subagent_task_name: Option<String>,
        outcome: MessageOutcome,
    ) -> Self {
        let (status, receipt_id, error) = match outcome {
            MessageOutcome::Received(receipt_id) => ("received", Some(receipt_id), None),
            MessageOutcome::Rejected(error) => ("rejected", None, Some(error)),
            MessageOutcome::Unconfirmed(error) => ("unconfirmed", None, Some(error)),
        };
        Self {
            id: id.into(),
            receipt_id,
            status: status.to_owned(),
            subagent_task_name,
            target_session_id,
            answer: None,
            error,
        }
    }

    pub fn message_received(id: impl Into<String>, receipt_id: impl Into<String>) -> Self {
        Self::message_outcome(id, None, None, MessageOutcome::Received(receipt_id.into()))
    }

    pub fn message_rejected(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::message_outcome(
            id,
            None,
            None,
            MessageOutcome::Rejected(AgentInteractionError::new(code, message)),
        )
    }

    pub fn message_unconfirmed(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::message_outcome(
            id,
            None,
            None,
            MessageOutcome::Unconfirmed(AgentInteractionError::new(code, message)),
        )
    }
}

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
    #[serde(default)]
    pub subagent_id: Option<String>,
    #[serde(default)]
    pub reply_to: Option<sampling_types::AgentMessageRef>,
    pub message: String,
    /// true soft-preempts sampling/interruptible waits; false queues for the next step.
    #[serde(default)]
    pub interrupt: bool,
}

async fn interact(
    ctx: ToolCallContext,
    target: Option<String>,
    action: AgentInteraction,
) -> Result<AgentInteractionOutput, ToolError> {
    let is_send = matches!(&action, AgentInteraction::Send { .. });
    let body = match &action {
        AgentInteraction::Ask { question } => question,
        AgentInteraction::Send { message, .. } => message,
    };
    if body.trim().is_empty() || body.len() > 16 * 1024 {
        if is_send {
            return Ok(AgentInteractionOutput::message_rejected(
                ctx.call_id.to_string(),
                "invalid_message",
                "Message must be nonempty and at most 16384 bytes.",
            ));
        }
        return Err(ToolError::custom(
            "invalid_message",
            "Message must be nonempty and at most 16384 bytes.",
        ));
    }
    if let AgentInteraction::Send {
        interrupt,
        reply_to,
        ..
    } = &action
    {
        let has_target = target.is_some();
        let has_reply = reply_to.is_some();
        let invalid = if target.as_deref().is_some_and(|target| target.is_empty()) {
            Some(AgentInteractionError::new(
                "invalid_subagent_id",
                "subagent_id must be nonempty.",
            ))
        } else if reply_to.as_ref().is_some_and(|reference| {
            reference.source_session_id.is_empty() || reference.message_id.is_empty()
        }) {
            Some(AgentInteractionError::new(
                "invalid_reply_reference",
                "reply_to must contain a source session and message ID.",
            ))
        } else {
            match (has_target, has_reply, *interrupt) {
                (true, false, _) | (false, true, false) => None,
                (false, true, true) => Some(AgentInteractionError::new(
                    "invalid_reply_interrupt",
                    "Reply messages cannot request immediate interruption.",
                )),
                _ => Some(AgentInteractionError::new(
                    "invalid_message_route",
                    "Provide exactly one of subagent_id or reply_to.",
                )),
            }
        };
        if let Some(error) = invalid {
            return Ok(AgentInteractionOutput::message_outcome(
                ctx.call_id.to_string(),
                target,
                None,
                MessageOutcome::Rejected(error),
            ));
        }
    }
    let resources = match crate::types::tool_metadata::shared_resources(&ctx) {
        Ok(resources) => resources,
        Err(_error) if is_send => {
            return Ok(AgentInteractionOutput::message_rejected(
                ctx.call_id.to_string(),
                "subagent_unavailable",
                "Subagent runtime unavailable.",
            ));
        }
        Err(error) => return Err(error),
    };
    let backend = resources
        .lock()
        .await
        .get::<SubagentBackendResource>()
        .cloned();
    let Some(backend) = backend else {
        if is_send {
            return Ok(AgentInteractionOutput::message_rejected(
                ctx.call_id.to_string(),
                "subagent_unavailable",
                "Subagent runtime unavailable.",
            ));
        }
        return Err(ToolError::custom(
            "subagent_unavailable",
            "Subagent runtime unavailable.",
        ));
    };
    let cancellation = ctx
        .get::<tool_runtime::Cancellation>()
        .map_or_else(tokio_util::sync::CancellationToken::new, |token| {
            token.0.clone()
        });
    let result = backend
        .backend()
        .interact(ctx.call_id.to_string(), target, action, cancellation)
        .await;
    match result {
        Ok(output) => Ok(output),
        Err(error) if is_send => Ok(AgentInteractionOutput::message_unconfirmed(
            ctx.call_id.to_string(),
            "delivery_unknown",
            error,
        )),
        Err(error) => Err(ToolError::custom("agent_interaction_failed", error)),
    }
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
    "Send a message to a directly-owned subagent or reply to a received agent message. Provide exactly one of subagent_id (new parent instruction) or reply_to (reply to an existing message); replies cannot interrupt. interrupt=true safely preempts an active model request or interruptible wait for a new instruction, while interrupt=false queues context for the next step. Success acknowledges durable receipt, not task completion.",
    Write,
    |input: SendSubagentMessageInput| (
        input.subagent_id,
        AgentInteraction::Send {
            message: input.message,
            interrupt: input.interrupt,
            reply_to: input.reply_to,
        }
    )
);
