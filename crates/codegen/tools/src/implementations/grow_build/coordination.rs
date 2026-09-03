use std::sync::Arc;

use crate::notification::types::{CoordinationPhase, ToolNotificationHandle};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::NotificationHandle;
use crate::types::tool::{ToolKind, ToolNamespace};

pub const LIST_ACTIVE_SESSIONS_TOOL_NAME: &str = "list_active_sessions";
pub const ASK_SESSION_TOOL_NAME: &str = "ask_session";
pub const GET_INQUIRY_TOOL_NAME: &str = "get_inquiry";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationErrorCode {
    RuntimeUnavailable,
    DiscoveryError,
    InvalidRequest,
    NotFound,
    Busy,
    PermissionDenied,
    TransportError,
    Conflict,
    TargetRestarted,
    TimedOut,
    Cancelled,
    SourceUnavailable,
    AuditFailure,
    Failed,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationError {
    pub code: CoordinationErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl CoordinationError {
    pub fn new(code: CoordinationErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            retry_after_ms: None,
        }
    }

    pub fn retry(code: CoordinationErrorCode, message: impl Into<String>, delay_ms: u64) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
            retry_after_ms: Some(delay_ms),
        }
    }
}

impl std::fmt::Display for CoordinationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for CoordinationError {}

impl From<String> for CoordinationError {
    fn from(message: String) -> Self {
        Self::new(CoordinationErrorCode::Failed, message)
    }
}

impl From<&str> for CoordinationError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

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
    pub error: Option<CoordinationError>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationInquiryState {
    pub inquiry_id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<CoordinationInquiryResult>,
}

#[async_trait::async_trait]
pub trait CoordinationBackend: Send + Sync {
    async fn list_active_sessions(&self) -> Result<Vec<ActiveSession>, CoordinationError>;

    async fn ask_session(
        &self,
        inquiry_id: Option<String>,
        target_session_id: String,
        question: String,
        progress: tokio::sync::mpsc::UnboundedSender<String>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<CoordinationInquiryResult, CoordinationError>;

    async fn get_inquiry(
        &self,
        inquiry_id: String,
    ) -> Result<CoordinationInquiryState, CoordinationError>;
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
    #[schemars(
        description = "Omit for a new inquiry. Supply an existing InquiryId only to resume the identical request; never generate one yourself."
    )]
    #[serde(default)]
    pub inquiry_id: Option<String>,
    #[schemars(description = "The target primary SessionId returned by list_active_sessions.")]
    pub target_session_id: String,
    #[schemars(description = "A concise question about the target session's current work.")]
    pub question: String,
}

impl tool_runtime::ToolOutput for CoordinationInquiryResult {}
impl tool_runtime::ToolOutput for CoordinationInquiryState {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetInquiryInput {
    pub inquiry_id: String,
}

#[derive(Debug, Default)]
pub struct GetInquiryTool;

impl crate::types::tool_metadata::ToolMetadata for GetInquiryTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }
    fn description_template(&self) -> &str {
        "Look up an inquiry started by this session, including its current phase or terminal answer/error. Does not send another question."
    }
}

impl tool_runtime::Tool for GetInquiryTool {
    type Args = GetInquiryInput;
    type Output = CoordinationInquiryState;
    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(GET_INQUIRY_TOOL_NAME).expect("valid tool id")
    }
    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            GET_INQUIRY_TOOL_NAME,
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
        coordination_backend(&ctx)
            .await?
            .get_inquiry(input.inquiry_id)
            .await
            .map_err(coordination_tool_error)
    }
}

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
        "Ask another online primary Grow session a question. Busy main turns can answer; inquiries queue FIFO. The target records the request, requires one-time user approval for a different workspace, and answers from one tool-free sideband model call. Use get_inquiry to check an existing InquiryId. Reuse inquiry_id only for an identical in-flight retry or cached result; after a retryable terminal failure, wait retryAfterMs and omit inquiry_id to start a new attempt. Subagents are not independently addressable; ask their primary session."
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
                input.inquiry_id,
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

fn coordination_tool_error(error: CoordinationError) -> tool_runtime::ToolError {
    let code = serde_json::to_value(error.code).expect("error code serializes");
    tool_runtime::ToolError::custom(
        code.as_str().unwrap(),
        serde_json::to_string(&error).expect("error serializes"),
    )
}
