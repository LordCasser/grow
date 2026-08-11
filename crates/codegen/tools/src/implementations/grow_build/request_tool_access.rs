//! Subagent-only control plane for requesting additional runtime capability.
//!
//! The tool owns only the wire contract. The shell injects the backend that
//! validates the session's hard eligibility, asks the shared permission
//! manager, and mutates the child-local grant state.

use std::sync::Arc;

use crate::types::output::{DynamicOutput, ToolOutput};
use crate::types::tool::{ToolKind, ToolNamespace};

pub const REQUEST_TOOL_ACCESS_NAME: &str = "request_tool_access";

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum NativeCapability {
    Execute,
    ReadWrite,
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolAccessTarget {
    Native { capability: NativeCapability },
    McpServer { server: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RequestToolAccessInput {
    pub target: ToolAccessTarget,
    pub purpose: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccessGrantStatus {
    Granted,
    AlreadyGranted,
    Denied,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccessGrantReason {
    Approved,
    AlreadyAvailable,
    OutsideEligibility,
    PolicyDenied,
    UserDenied,
    Cancelled,
    TimedOut,
    FollowupRequired,
    Unresolved,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestToolAccessOutput {
    pub status: ToolAccessGrantStatus,
    pub reason: ToolAccessGrantReason,
    pub target: ToolAccessTarget,
    pub message: String,
}

#[async_trait::async_trait]
pub trait ToolAccessGrantBackend: Send + Sync {
    async fn request(
        &self,
        input: RequestToolAccessInput,
        tool_call_id: &str,
    ) -> Result<RequestToolAccessOutput, tool_runtime::ToolError>;

    fn is_mcp_server_granted(&self, server: &str) -> bool;

    /// Hard eligibility checks used by discovery before grant status is
    /// rendered. Defaults preserve the primary-session backend contract.
    fn is_mcp_server_eligible(&self, _server: &str) -> bool {
        true
    }

    fn is_mcp_tool_eligible(&self, _qualified_tool: &str) -> bool {
        true
    }

    /// Final dispatch-time authorization check. This is intentionally read
    /// again after the ordinary tool permission decision, because the
    /// inherited server transport or child-local grant can change while that
    /// permission request is awaiting a response.
    fn is_mcp_tool_granted(&self, qualified_tool: &str) -> bool {
        let Some((server, tool)) = qualified_tool.split_once("__") else {
            return false;
        };
        !server.is_empty()
            && !tool.is_empty()
            && !tool.contains("__")
            && self.is_mcp_server_granted(server)
            && self.is_mcp_tool_eligible(qualified_tool)
    }

    /// Consume user follow-up text entered instead of approving this request.
    /// The shell promotes it to the normal user-message control flow after the
    /// tool result is recorded.
    fn take_followup(&self, _tool_call_id: &str) -> Option<String> {
        None
    }
}

#[derive(Clone)]
pub struct ToolAccessGrantBackendResource(pub Arc<dyn ToolAccessGrantBackend>);

impl std::fmt::Debug for ToolAccessGrantBackendResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ToolAccessGrantBackendResource")
            .field(&"<backend>")
            .finish()
    }
}

/// Re-read child capability state at the last common dispatch boundary. Root
/// sessions do not install this resource and therefore keep their existing
/// behavior; child sessions fail closed when a granted MCP binding was
/// revoked or replaced while permission UI/classification was in flight.
pub async fn ensure_mcp_tool_granted(
    ctx: &tool_runtime::ToolCallContext,
    qualified_tool: &str,
) -> Result<(), tool_runtime::ToolError> {
    let Ok(resources) = crate::types::tool_metadata::shared_resources(ctx) else {
        return Ok(());
    };
    let backend = resources
        .lock()
        .await
        .get::<ToolAccessGrantBackendResource>()
        .cloned();
    if backend.is_some_and(|backend| !backend.0.is_mcp_tool_granted(qualified_tool)) {
        return Err(tool_runtime::ToolError::custom(
            "subagent_capability_revoked",
            format!(
                "MCP tool '{qualified_tool}' is no longer granted to this subagent. Search the live catalog and request access again if it remains eligible."
            ),
        ));
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct RequestToolAccessTool;

impl crate::types::tool_metadata::ToolMetadata for RequestToolAccessTool {
    fn kind(&self) -> ToolKind {
        ToolKind::CapabilityRequest
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Request one additional capability for this subagent session. The capability catalog in the system prompt is only an eligibility list, not a grant. Provide a concrete purpose tied to the assigned task. Request `execute` for shell execution, `read-write` for workspace mutation, or one eligible MCP server before calling its tools. A successful capability grant does not bypass permission checks on the eventual tool call."
    }
}

impl tool_runtime::Tool for RequestToolAccessTool {
    type Args = RequestToolAccessInput;
    type Output = ToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(REQUEST_TOOL_ACCESS_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            REQUEST_TOOL_ACCESS_NAME,
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            tool_scope: tool_protocol::ToolScope::Read,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        mut input: RequestToolAccessInput,
    ) -> Result<ToolOutput, tool_runtime::ToolError> {
        input.purpose = input.purpose.trim().to_owned();
        if input.purpose.is_empty() {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "`purpose` must explain why the assigned task requires this capability",
            ));
        }
        if let ToolAccessTarget::McpServer { server } = &mut input.target {
            *server = server.trim().to_owned();
            if server.is_empty() {
                return Err(tool_runtime::ToolError::invalid_arguments(
                    "MCP server name must not be empty",
                ));
            }
        }

        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = resources
            .lock()
            .await
            .get::<ToolAccessGrantBackendResource>()
            .cloned()
            .ok_or_else(|| {
                tool_runtime::ToolError::service_unavailable(
                    "tool access requests are unavailable outside a subagent session",
                )
            })?;
        let output = backend.0.request(input, ctx.call_id.as_ref()).await?;
        Ok(ToolOutput::Dynamic(DynamicOutput::from(
            serde_json::to_value(output).unwrap_or(serde_json::Value::Null),
        )))
    }
}
