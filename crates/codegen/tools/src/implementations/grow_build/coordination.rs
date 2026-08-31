use std::sync::Arc;

use crate::notification::types::{CoordinationPhase, ToolNotificationHandle};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::NotificationHandle;
use crate::types::tool::{ToolKind, ToolNamespace};

pub const LIST_ACTIVE_SESSIONS_TOOL_NAME: &str = "list_active_sessions";
pub const ASK_SESSION_TOOL_NAME: &str = "ask_session";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSession {
    pub session_id: String,
    pub canonical_cwd: String,
    pub main_agent: String,
    pub activity: String,
    pub active_subagents: usize,
    pub started_at: i64,
    pub process_started_at: i64,
    pub last_heartbeat: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationInquiryResult {
    pub inquiry_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[async_trait::async_trait]
pub trait CoordinationBackend: Send + Sync {
    async fn list_active_sessions(&self) -> Result<Vec<ActiveSession>, String>;

    async fn ask_session(
        &self,
        target_session_id: String,
        question: String,
        progress: tokio::sync::mpsc::UnboundedSender<String>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<CoordinationInquiryResult, String>;
}

#[derive(Clone)]
pub struct CoordinationBackendResource(pub Arc<dyn CoordinationBackend>);

impl std::fmt::Debug for CoordinationBackendResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CoordinationBackendResource")
            .finish_non_exhaustive()
    }
}

crate::register_resource!(
    "grow_build",
    "CoordinationBackendResource",
    CoordinationBackendResource
);

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListActiveSessionsInput {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListActiveSessionsOutput {
    pub sessions: Vec<ActiveSession>,
}

impl tool_runtime::ToolOutput for ListActiveSessionsOutput {}

#[derive(Debug, Default)]
pub struct ListActiveSessionsTool;

impl crate::types::tool_metadata::ToolMetadata for ListActiveSessionsTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "List other online primary Grow sessions owned by the current OS user and GROW_HOME. Use this when concurrent workspace changes suggest another local Agent may be working nearby."
    }
}

impl tool_runtime::Tool for ListActiveSessionsTool {
    type Args = ListActiveSessionsInput;
    type Output = ListActiveSessionsOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(LIST_ACTIVE_SESSIONS_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            LIST_ACTIVE_SESSIONS_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
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
        _input: Self::Args,
    ) -> Result<Self::Output, tool_runtime::ToolError> {
        let backend = coordination_backend(&ctx).await?;
        let sessions = backend
            .list_active_sessions()
            .await
            .map_err(coordination_tool_error)?;
        Ok(ListActiveSessionsOutput { sessions })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskSessionInput {
    #[schemars(description = "The target primary SessionId returned by list_active_sessions.")]
    pub target_session_id: String,
    #[schemars(description = "A concise question about the target session's current work.")]
    pub question: String,
}

impl tool_runtime::ToolOutput for CoordinationInquiryResult {}

#[derive(Debug, Default)]
pub struct AskSessionTool;

impl crate::types::tool_metadata::ToolMetadata for AskSessionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Ask another online primary Grow session a question. The target session records the request, may require one-time user approval for a different workspace, and answers from a single tool-free sideband model call."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::Value(ToolRequirement::Tool {
            namespace: ToolNamespace::Grow.to_string(),
            id: LIST_ACTIVE_SESSIONS_TOOL_NAME.to_owned(),
            if_params: None,
        })
    }
}

impl tool_runtime::Tool for AskSessionTool {
    type Args = AskSessionInput;
    type Output = CoordinationInquiryResult;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(ASK_SESSION_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            ASK_SESSION_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
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
        let backend = coordination_backend(&ctx).await?;
        let (progress, mut phases) = tokio::sync::mpsc::unbounded_channel();
        let notifications = notification_handle(&ctx).await?;
        let tool_call_id = ctx.call_id.to_string();
        let progress_task = tokio::spawn(async move {
            while let Some(phase) = phases.recv().await {
                notifications.send_coordination_phase(CoordinationPhase {
                    tool_call_id: tool_call_id.clone(),
                    phase,
                });
            }
        });
        let cancellation = ctx
            .get::<tool_runtime::Cancellation>()
            .map_or_else(tokio_util::sync::CancellationToken::new, |token| {
                token.0.clone()
            });
        let result = backend
            .ask_session(
                input.target_session_id,
                input.question,
                progress,
                cancellation,
            )
            .await
            .map_err(coordination_tool_error);
        let _ = progress_task.await;
        result
    }
}

async fn coordination_backend(
    ctx: &tool_runtime::ToolCallContext,
) -> Result<Arc<dyn CoordinationBackend>, tool_runtime::ToolError> {
    let resources = crate::types::tool_metadata::shared_resources(ctx)?;
    resources
        .lock()
        .await
        .get::<CoordinationBackendResource>()
        .map(|resource| Arc::clone(&resource.0))
        .ok_or_else(|| {
            tool_runtime::ToolError::custom(
                "coordination_unavailable",
                "Local Grow coordination is unavailable for this session.",
            )
        })
}

async fn notification_handle(
    ctx: &tool_runtime::ToolCallContext,
) -> Result<ToolNotificationHandle, tool_runtime::ToolError> {
    let resources = crate::types::tool_metadata::shared_resources(ctx)?;
    resources
        .lock()
        .await
        .get::<NotificationHandle>()
        .map(|handle| handle.0.clone())
        .ok_or_else(|| {
            tool_runtime::ToolError::custom(
                "coordination_unavailable",
                "Tool notification channel is unavailable.",
            )
        })
}

fn coordination_tool_error(error: String) -> tool_runtime::ToolError {
    tool_runtime::ToolError::custom("coordination_failed", error)
}
