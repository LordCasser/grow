//! Goal model tools. All three commands share one session-owned runtime
//! handle; the tools contain no Goal state and never run verifier work.

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::ToolMetadata;

pub const GET_GOAL_TOOL_NAME: &str = "get_goal";
pub const UPDATE_GOAL_PLAN_TOOL_NAME: &str = "update_goal_plan";
pub use crate::slash_commands::UPDATE_GOAL_TOOL_NAME;

/// Explicit empty-object input for `get_goal`.
///
/// `serde_json::Value` has no object-shaped root schema, which makes providers
/// that validate function definitions reject the entire sampling request even
/// though the tool itself takes no arguments.
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
pub struct UpdateGoalInput {
    #[schemars(
        description = "candidate_complete requests independent verification; blocked stops autonomous execution."
    )]
    pub action: UpdateGoalAction,
    #[schemars(description = "Concise evidence-backed completion summary or blocking reason.")]
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateGoalPlanInput {
    #[schemars(
        description = "The complete replacement user-visible Markdown blackboard. Include only shared task status, checklist, acceptance criteria, verification evidence, and unresolved gaps; exclude Agent-only instructions, tool directions, and orchestration policy."
    )]
    pub markdown: String,
    #[serde(default)]
    pub reason: Option<String>,
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
    pub plan_markdown: String,
    pub verifier_feedback: Option<String>,
}

impl tool_runtime::ToolOutput for GoalView {}

#[derive(Debug)]
pub enum GoalCommand {
    Get {
        respond_to: tokio::sync::oneshot::Sender<Result<GoalView, String>>,
    },
    ReplacePlan {
        input: UpdateGoalPlanInput,
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
    ($tool:ty, $description:literal) => {
        impl crate::types::tool_metadata::ToolMetadata for $tool {
            fn kind(&self) -> ToolKind {
                ToolKind::GoalUpdate
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

#[derive(Debug, Default)]
pub struct GetGoalTool;

goal_metadata!(
    GetGoalTool,
    "Read the active Goal, phase, budget, Markdown plan, and verifier feedback."
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

#[derive(Debug, Default)]
pub struct UpdateGoalPlanTool;

goal_metadata!(
    UpdateGoalPlanTool,
    "Replace the active Goal's shared, user-visible Markdown blackboard. Keep Agent-only instructions out of it."
);

impl tool_runtime::Tool for UpdateGoalPlanTool {
    type Args = UpdateGoalPlanInput;
    type Output = UpdateGoalOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(UPDATE_GOAL_PLAN_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(UPDATE_GOAL_PLAN_TOOL_NAME, self.description_template())
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            tool_scope: tool_protocol::ToolScope::Write,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: UpdateGoalPlanInput,
    ) -> Result<UpdateGoalOutput, tool_runtime::ToolError> {
        let sender = runtime_sender(&ctx).await?;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(GoalCommand::ReplacePlan { input, respond_to })
            .map_err(|_| channel_error())?;
        let summary = response
            .await
            .map_err(|_| channel_error())?
            .map_err(|message| tool_runtime::ToolError::custom("goal_plan_rejected", message))?;
        Ok(UpdateGoalOutput {
            success: true,
            summary,
        })
    }
}

#[derive(Debug, Default)]
pub struct UpdateGoalTool;

goal_metadata!(
    UpdateGoalTool,
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

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            tool_scope: tool_protocol::ToolScope::Write,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: UpdateGoalInput,
    ) -> Result<UpdateGoalOutput, tool_runtime::ToolError> {
        let sender = runtime_sender(&ctx).await?;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(GoalCommand::Update { input, respond_to })
            .map_err(|_| channel_error())?;
        let summary = response
            .await
            .map_err(|_| channel_error())?
            .map_err(|message| tool_runtime::ToolError::custom("goal_update_rejected", message))?;
        Ok(UpdateGoalOutput {
            success: true,
            summary,
        })
    }
}
