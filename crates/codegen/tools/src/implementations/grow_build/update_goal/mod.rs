//! Goal-scoped model tools.
//!
//! Tools are stateless command adapters. The primary Session runtime is the
//! only authority that may commit planner data, progress patches, replans, or
//! lifecycle transitions. Delegated Goal agents receive an immutable snapshot
//! and are read-only at the object boundary.

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::ToolMetadata;

pub const GET_GOAL_TOOL_NAME: &str = "get_goal";
pub const UPDATE_GOAL_PROGRESS_TOOL_NAME: &str = "update_goal_progress";
pub const REQUEST_GOAL_REPLAN_TOOL_NAME: &str = "request_goal_replan";
pub use crate::slash_commands::UPDATE_GOAL_TOOL_NAME;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetGoalInput {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateGoalAction {
    CandidateComplete,
    Blocked,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGoalInput {
    pub expected_plan_revision: u64,
    pub expected_board_revision: u64,
    #[schemars(
        description = "candidate_complete requests independent verification; blocked stops autonomous execution."
    )]
    pub action: UpdateGoalAction,
    #[schemars(description = "Concise evidence-backed completion summary or blocking reason.")]
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGoalProgressInput {
    pub expected_plan_revision: u64,
    pub expected_board_revision: u64,
    #[schemars(
        description = "Typed updates to existing stable task ids. Task structure is immutable."
    )]
    pub updates: Vec<tool_types::GoalProgressUpdate>,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestGoalReplanInput {
    pub expected_plan_revision: u64,
    pub expected_board_revision: u64,
    #[schemars(description = "Planner guidance; this is not replacement Markdown.")]
    pub guidance: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct GoalView {
    pub goal_id: String,
    pub objective: String,
    pub objective_revision: u64,
    pub status: String,
    pub phase: String,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub plan_revision: u64,
    pub board_revision: u64,
    pub tasks: Vec<tool_types::GoalTaskProjection>,
    pub plan_markdown: String,
    pub verifier_feedback: Option<String>,
}

impl tool_runtime::ToolOutput for GoalView {}

/// Immutable Goal data delegated to a child session. A child reads this
/// snapshot instead of consulting the parent's live runtime.
#[derive(Debug, Clone)]
pub struct GoalContextSnapshot {
    pub role: crate::implementations::grow_build::task::types::GoalSubagentRole,
    pub view: GoalView,
}

#[derive(Debug, Clone, Default)]
pub struct GoalContextSnapshotResource(pub Option<GoalContextSnapshot>);

crate::register_resource!(
    "grow_build",
    "GoalContextSnapshotResource",
    GoalContextSnapshotResource
);

/// Parent-turn snapshot consumed by `task` when it creates a Goal-owned
/// worker. Unlike `GoalContextSnapshotResource`, this does not make the
/// primary Agent read-only.
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
    Progress {
        input: UpdateGoalProgressInput,
        respond_to: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    Replan {
        input: RequestGoalReplanInput,
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
            "Delegated Goal agents may read their immutable Goal snapshot but cannot mutate it.",
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
    "Read the active Goal, revisions, structured tasks, Markdown board, and verifier feedback."
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
            tool_scope: tool_protocol::ToolScope::Read,
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
pub struct UpdateGoalProgressTool;

goal_metadata!(
    UpdateGoalProgressTool,
    ToolKind::GoalProgressUpdate,
    "Update status, progress, evidence, or gap fields on existing Goal task ids."
);

impl tool_runtime::Tool for UpdateGoalProgressTool {
    type Args = UpdateGoalProgressInput;
    type Output = UpdateGoalOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(UPDATE_GOAL_PROGRESS_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            UPDATE_GOAL_PROGRESS_TOOL_NAME,
            self.description_template(),
        )
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: UpdateGoalProgressInput,
    ) -> Result<UpdateGoalOutput, tool_runtime::ToolError> {
        let sender = runtime_sender(&ctx).await?;
        command_output(
            sender,
            |respond_to| GoalCommand::Progress { input, respond_to },
            "goal_progress_rejected",
        )
        .await
    }
}

#[derive(Debug, Default)]
pub struct RequestGoalReplanTool;

goal_metadata!(
    RequestGoalReplanTool,
    ToolKind::GoalReplanRequest,
    "Request a structural replan from the background Goal planner. Do not provide replacement Markdown."
);

impl tool_runtime::Tool for RequestGoalReplanTool {
    type Args = RequestGoalReplanInput;
    type Output = UpdateGoalOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(REQUEST_GOAL_REPLAN_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(REQUEST_GOAL_REPLAN_TOOL_NAME, self.description_template())
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: RequestGoalReplanInput,
    ) -> Result<UpdateGoalOutput, tool_runtime::ToolError> {
        let sender = runtime_sender(&ctx).await?;
        command_output(
            sender,
            |respond_to| GoalCommand::Replan { input, respond_to },
            "goal_replan_rejected",
        )
        .await
    }
}

#[derive(Debug, Default)]
pub struct UpdateGoalTool;

goal_metadata!(
    UpdateGoalTool,
    ToolKind::GoalLifecycleUpdate,
    "Submit a completion candidate for independent verification, or report a genuine blocker."
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
    use super::GetGoalInput;

    #[test]
    fn get_goal_exports_an_empty_object_schema() {
        let schema = crate::registry::types::generate_schema::<GetGoalInput>();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], serde_json::json!({}));
        assert_eq!(schema["required"], serde_json::json!([]));
    }
}
