//! Codex-style long-lived Goal tools.
//!
//! These tools expose one durable objective. They do not expose a plan,
//! task board, progress log, planner, or verifier. The Session actor is the
//! sole mutation authority; tools are only typed command adapters.

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::ToolMetadata;

pub const CREATE_GOAL_TOOL_NAME: &str = "create_goal";
pub const GET_GOAL_TOOL_NAME: &str = "get_goal";
pub use crate::slash_commands::UPDATE_GOAL_TOOL_NAME;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetGoalInput {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGoalInput {
    #[schemars(description = "The concrete long-term objective to pursue.")]
    pub objective: String,
    #[schemars(description = "Optional positive token budget for this Goal.")]
    pub token_budget: Option<i64>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalUpdateStatus {
    Complete,
    Blocked,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGoalInput {
    #[schemars(
        description = "Set complete only when current evidence proves the entire objective is achieved. Set blocked only when the same genuine impasse has recurred for at least three consecutive Goal turns and no meaningful progress is possible without user input or an external-state change."
    )]
    pub status: GoalUpdateStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GoalView {
    pub goal_id: String,
    pub objective: String,
    pub status: String,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub elapsed_ms: u64,
    pub created_at: String,
    pub updated_at: String,
    pub status_message: Option<String>,
}

impl tool_runtime::ToolOutput for GoalView {}

/// Immutable Goal snapshot inherited by a Goal-owned child session. It is
/// context, not an independently mutable Goal runtime.
#[derive(Debug, Clone)]
pub struct GoalContextSnapshot {
    pub view: GoalView,
}

#[derive(Debug, Clone, Default)]
pub struct GoalContextSnapshotResource(pub Option<GoalContextSnapshot>);

crate::register_resource!(
    "grow_build",
    "GoalContextSnapshotResource",
    GoalContextSnapshotResource
);

/// Snapshot captured when a Goal turn delegates work to a child.
#[derive(Debug, Clone, Default)]
pub struct GoalDelegationSnapshotResource(pub Option<GoalView>);

crate::register_resource!(
    "grow_build",
    "GoalDelegationSnapshotResource",
    GoalDelegationSnapshotResource
);

#[derive(Debug)]
pub enum GoalCommand {
    Get {
        respond_to: tokio::sync::oneshot::Sender<Result<GoalView, String>>,
    },
    Create {
        input: CreateGoalInput,
        respond_to: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    Update {
        input: UpdateGoalInput,
        respond_to: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
}

pub struct GoalRuntimeHandle(pub tokio::sync::mpsc::UnboundedSender<GoalCommand>);

impl std::fmt::Debug for GoalRuntimeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalRuntimeHandle").finish()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateGoalOutput {
    pub success: bool,
    pub summary: String,
}

impl tool_runtime::ToolOutput for UpdateGoalOutput {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateGoalOutput {
    pub success: bool,
    pub summary: String,
}

impl tool_runtime::ToolOutput for CreateGoalOutput {}

async fn runtime_sender(
    ctx: &tool_runtime::ToolCallContext,
) -> Result<tokio::sync::mpsc::UnboundedSender<GoalCommand>, tool_runtime::ToolError> {
    use crate::types::tool_metadata::shared_resources;
    let resources = shared_resources(ctx)?;
    let resources = resources.lock().await;
    if resources
        .get::<GoalContextSnapshotResource>()
        .is_some_and(|resource| resource.0.is_some())
    {
        return Err(tool_runtime::ToolError::custom(
            "delegated_goal_read_only",
            "A delegated session may read its inherited Goal snapshot but cannot mutate it.",
        ));
    }
    resources
        .get::<GoalRuntimeHandle>()
        .map(|handle| handle.0.clone())
        .ok_or_else(|| {
            tool_runtime::ToolError::custom("goal_not_available", "Goal runtime is unavailable.")
        })
}

fn channel_error() -> tool_runtime::ToolError {
    tool_runtime::ToolError::custom("goal_channel_closed", "Goal runtime channel is closed.")
}

macro_rules! goal_metadata {
    ($tool:ty, $kind:expr, $description:literal) => {
        impl crate::types::tool_metadata::ToolMetadata for $tool {
            fn kind(&self) -> ToolKind {
                $kind
            }
            fn tool_namespace(&self) -> ToolNamespace {
                ToolNamespace::Grow
            }
            fn description_template(&self) -> &str {
                $description
            }
            fn requires_expr(&self) -> Expr<ToolRequirement> {
                Expr::True
            }
        }
    };
}

async fn command_output(
    sender: tokio::sync::mpsc::UnboundedSender<GoalCommand>,
    build: impl FnOnce(tokio::sync::oneshot::Sender<Result<String, String>>) -> GoalCommand,
    code: &'static str,
) -> Result<UpdateGoalOutput, tool_runtime::ToolError> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    sender
        .send(build(respond_to))
        .map_err(|_| channel_error())?;
    let summary = response
        .await
        .map_err(|_| channel_error())?
        .map_err(|message| tool_runtime::ToolError::custom(code, message))?;
    Ok(UpdateGoalOutput {
        success: true,
        summary,
    })
}

#[derive(Debug, Default)]
pub struct GetGoalTool;

goal_metadata!(
    GetGoalTool,
    ToolKind::GoalRead,
    "Read the current long-lived Goal, including status, budget, usage, and elapsed time."
);

impl tool_runtime::Tool for GetGoalTool {
    type Args = GetGoalInput;
    type Output = GoalView;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(GET_GOAL_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(GET_GOAL_TOOL_NAME, self.description_template())
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
        _input: GetGoalInput,
    ) -> Result<GoalView, tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        if let Some(snapshot) = resources
            .lock()
            .await
            .get::<GoalContextSnapshotResource>()
            .and_then(|resource| resource.0.as_ref())
            .cloned()
        {
            return Ok(snapshot.view);
        }
        let sender = runtime_sender(&ctx).await?;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(GoalCommand::Get { respond_to })
            .map_err(|_| channel_error())?;
        response
            .await
            .map_err(|_| channel_error())?
            .map_err(|message| tool_runtime::ToolError::custom("goal_not_active", message))
    }
}

#[derive(Debug, Default)]
pub struct CreateGoalTool;

goal_metadata!(
    CreateGoalTool,
    ToolKind::GoalLifecycleUpdate,
    "Create a long-lived Goal only when the user explicitly asks to start one. Omit token_budget unless explicitly requested."
);

impl tool_runtime::Tool for CreateGoalTool {
    type Args = CreateGoalInput;
    type Output = CreateGoalOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(CREATE_GOAL_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(CREATE_GOAL_TOOL_NAME, self.description_template())
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::WriteExecute,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: CreateGoalInput,
    ) -> Result<CreateGoalOutput, tool_runtime::ToolError> {
        let sender = runtime_sender(&ctx).await?;
        command_output(
            sender,
            |respond_to| GoalCommand::Create { input, respond_to },
            "goal_create_rejected",
        )
        .await
        .map(|output| CreateGoalOutput {
            success: output.success,
            summary: output.summary,
        })
    }
}

#[derive(Debug, Default)]
pub struct UpdateGoalTool;

goal_metadata!(
    UpdateGoalTool,
    ToolKind::GoalLifecycleUpdate,
    "Mark the current Goal complete only when authoritative current evidence proves the entire objective is achieved. Mark it blocked only after the same genuine impasse recurs for at least three consecutive Goal turns and no meaningful progress is possible without user input or an external-state change. Pause, resume, edit, budget, and clear are user-owned controls."
);

impl tool_runtime::Tool for UpdateGoalTool {
    type Args = UpdateGoalInput;
    type Output = UpdateGoalOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(UPDATE_GOAL_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(UPDATE_GOAL_TOOL_NAME, self.description_template())
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::WriteExecute,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: UpdateGoalInput,
    ) -> Result<UpdateGoalOutput, tool_runtime::ToolError> {
        let sender = runtime_sender(&ctx).await?;
        command_output(
            sender,
            |respond_to| GoalCommand::Update { input, respond_to },
            "goal_update_rejected",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_tools_have_small_strict_inputs() {
        assert!(serde_json::from_str::<GetGoalInput>("{}").is_ok());
        assert!(serde_json::from_str::<GetGoalInput>(r#"{"phase":"planning"}"#).is_err());
        assert!(serde_json::from_str::<UpdateGoalInput>(r#"{"status":"complete"}"#).is_ok());
        assert!(
            serde_json::from_str::<UpdateGoalInput>(
                r#"{"status":"candidate_complete","message":"done"}"#
            )
            .is_err()
        );
    }
}
