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

pub const SUBMIT_GOAL_PLAN_SECTION_TOOL_NAME: &str = "submit_goal_plan_section";
pub const FINALIZE_GOAL_PLAN_TOOL_NAME: &str = "finalize_goal_plan";

/// Stage lease token carried by the planner's submit handle. The actor-side
/// lease check (Task 3) compares it against the current planning stage id;
/// the tool layer never interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StageToken(pub u64);

/// Commands a delegated planner stage sends to the session actor through
/// [`GoalStageSubmitHandle`]. This is deliberately its own enum rather than
/// `GoalCommand` variants: `GoalCommand` is consumed by an exhaustive match
/// in the session actor (wired in a follow-up task), and the planner submit
/// channel has its own per-stage lifecycle. The actor owns all validation —
/// stage/lease checks, section aggregation, and final board assembly via the
/// host-side assembler.
#[derive(Debug)]
pub enum GoalPlanCommand {
    SubmitPlanSection {
        stage: StageToken,
        section: tool_types::GoalPlanSectionPayload,
        respond_to: tokio::sync::oneshot::Sender<Result<SubmitGoalPlanSectionOutput, String>>,
    },
    FinalizePlan {
        stage: StageToken,
        respond_to: tokio::sync::oneshot::Sender<Result<FinalizeGoalPlanOutput, String>>,
    },
}

/// Planner-only submit channel handed to a delegated Goal planner stage.
/// Presence of this handle plus a `GoalContextSnapshotResource` whose role is
/// `Planner` is the narrow gate through which plan sections may be submitted;
/// every other session shape fails closed inside the tools below.
#[derive(Clone)]
pub struct GoalStageSubmitHandle(
    pub tokio::sync::mpsc::UnboundedSender<GoalPlanCommand>,
    pub StageToken,
);

impl std::fmt::Debug for GoalStageSubmitHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalStageSubmitHandle").finish()
    }
}

crate::register_resource!("grow_build", "GoalStageSubmitHandle", GoalStageSubmitHandle);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitGoalPlanSectionInput {
    #[schemars(
        description = "One structured plan section: plan_tasks, goal_acceptance, or open_gaps. Task ids, indentation, and Markdown are derived by the host."
    )]
    pub section: tool_types::GoalPlanSectionPayload,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalizeGoalPlanInput {}

/// Result of one plan-section submission: the section kinds the actor has
/// accepted so far for this planning stage, plus every structured issue
/// found in the submitted section. An empty `issues` list means the section
/// was accepted.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SubmitGoalPlanSectionOutput {
    pub accepted_sections: Vec<String>,
    pub issues: Vec<tool_types::GoalPlanAssemblyIssue>,
}

impl tool_runtime::ToolOutput for SubmitGoalPlanSectionOutput {}

/// Result of finalizing a Goal plan: an actor summary plus the re-read Goal
/// view, whose task projection carries the host-assigned ids.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct FinalizeGoalPlanOutput {
    pub summary: String,
    pub view: GoalView,
}

impl tool_runtime::ToolOutput for FinalizeGoalPlanOutput {}

/// Resolve the planner submit channel, failing closed on every session
/// shape that is not a delegated Goal planner stage: other delegated Goal
/// roles keep the `delegated_goal_read_only` boundary, and primary sessions
/// (which never receive a submit handle) are rejected as unavailable.
/// Stage/lease validity is checked by the actor, not here.
async fn stage_submit_sender(
    ctx: &tool_runtime::ToolCallContext,
) -> Result<
    (
        tokio::sync::mpsc::UnboundedSender<GoalPlanCommand>,
        StageToken,
    ),
    tool_runtime::ToolError,
> {
    use crate::implementations::grow_build::task::types::GoalSubagentRole;
    use crate::types::tool_metadata::shared_resources;

    let resources = shared_resources(ctx)?;
    let resources = resources.lock().await;
    match resources
        .get::<GoalContextSnapshotResource>()
        .and_then(|resource| resource.0.as_ref())
    {
        Some(snapshot) if snapshot.role == GoalSubagentRole::Planner => {}
        Some(_) => {
            return Err(tool_runtime::ToolError::custom(
                "delegated_goal_read_only",
                "Delegated Goal agents may read their immutable Goal snapshot; only the planner role may submit plan sections.",
            ));
        }
        None => {
            return Err(tool_runtime::ToolError::custom(
                "goal_stage_submit_unavailable",
                "Goal plan submission is only available to the delegated planner stage.",
            ));
        }
    }
    resources
        .get::<GoalStageSubmitHandle>()
        .map(|handle| (handle.0.clone(), handle.1))
        .ok_or_else(|| {
            tool_runtime::ToolError::custom(
                "goal_stage_submit_unavailable",
                "Goal planner stage submit handle is unavailable.",
            )
        })
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

#[derive(Debug, Default)]
pub struct SubmitGoalPlanSectionTool;

goal_metadata!(
    SubmitGoalPlanSectionTool,
    ToolKind::GoalPlanSubmit,
    "Submit one structured section of the Goal plan (plan_tasks, goal_acceptance, or open_gaps). The host derives task ids, indentation, and all Markdown; fix each reported issue and resubmit."
);

impl tool_runtime::Tool for SubmitGoalPlanSectionTool {
    type Args = SubmitGoalPlanSectionInput;
    type Output = SubmitGoalPlanSectionOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(SUBMIT_GOAL_PLAN_SECTION_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            SUBMIT_GOAL_PLAN_SECTION_TOOL_NAME,
            self.description_template(),
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
        input: SubmitGoalPlanSectionInput,
    ) -> Result<SubmitGoalPlanSectionOutput, tool_runtime::ToolError> {
        let (sender, stage) = stage_submit_sender(&ctx).await?;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(GoalPlanCommand::SubmitPlanSection {
                stage,
                section: input.section,
                respond_to,
            })
            .map_err(|_| channel_error())?;
        response
            .await
            .map_err(|_| channel_error())?
            .map_err(|message| {
                tool_runtime::ToolError::custom("goal_plan_submit_rejected", message)
            })
    }
}

#[derive(Debug, Default)]
pub struct FinalizeGoalPlanTool;

goal_metadata!(
    FinalizeGoalPlanTool,
    ToolKind::GoalPlanSubmit,
    "Finalize the structured Goal plan after every required section (plan_tasks, goal_acceptance; open_gaps optional) has been accepted. Returns the committed board view with host-assigned task ids."
);

impl tool_runtime::Tool for FinalizeGoalPlanTool {
    type Args = FinalizeGoalPlanInput;
    type Output = FinalizeGoalPlanOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(FINALIZE_GOAL_PLAN_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(FINALIZE_GOAL_PLAN_TOOL_NAME, self.description_template())
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
        _input: FinalizeGoalPlanInput,
    ) -> Result<FinalizeGoalPlanOutput, tool_runtime::ToolError> {
        let (sender, stage) = stage_submit_sender(&ctx).await?;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(GoalPlanCommand::FinalizePlan { stage, respond_to })
            .map_err(|_| channel_error())?;
        response
            .await
            .map_err(|_| channel_error())?
            .map_err(|message| {
                tool_runtime::ToolError::custom("goal_plan_finalize_rejected", message)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FINALIZE_GOAL_PLAN_TOOL_NAME, FinalizeGoalPlanInput, FinalizeGoalPlanOutput, GetGoalInput,
        GoalContextSnapshot, GoalContextSnapshotResource, GoalPlanCommand, GoalRuntimeHandle,
        GoalStageSubmitHandle, GoalView, SUBMIT_GOAL_PLAN_SECTION_TOOL_NAME, StageToken,
        SubmitGoalPlanSectionInput, SubmitGoalPlanSectionOutput, SubmitGoalPlanSectionTool,
    };
    use crate::implementations::grow_build::task::types::GoalSubagentRole;
    use tool_runtime::Tool;

    #[test]
    fn get_goal_exports_an_empty_object_schema() {
        let schema = crate::registry::types::generate_schema::<GetGoalInput>();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], serde_json::json!({}));
        assert_eq!(schema["required"], serde_json::json!([]));
    }

    #[test]
    fn plan_submit_inputs_export_strict_object_schemas() {
        let submit = crate::registry::types::generate_schema::<SubmitGoalPlanSectionInput>();
        assert_eq!(submit["type"], "object");
        assert_eq!(submit["required"], serde_json::json!(["section"]));
        assert_eq!(submit["additionalProperties"], serde_json::json!(false));

        let finalize = crate::registry::types::generate_schema::<FinalizeGoalPlanInput>();
        assert_eq!(finalize["type"], "object");
        assert_eq!(finalize["properties"], serde_json::json!({}));
        assert_eq!(finalize["required"], serde_json::json!([]));
        assert_eq!(finalize["additionalProperties"], serde_json::json!(false));
    }

    #[test]
    fn submit_input_is_data_only_and_rejects_document_syntax() {
        let valid: SubmitGoalPlanSectionInput = serde_json::from_str(
            r#"{"section": {"plan_tasks": {"tasks": [{"summary": "Ship"}]}}}"#,
        )
        .unwrap();
        assert!(matches!(
            valid.section,
            tool_types::GoalPlanSectionPayload::PlanTasks { .. }
        ));

        for payload in [
            r##"{"section": {"plan_tasks": {"tasks": []}}, "board_markdown": "# Goal"}"##,
            r#"{"section": {"plan_tasks": {"tasks": [{"summary": "s", "id": "T9"}]}}}"#,
            r#"{"section": {"unknown_section": {}}}"#,
        ] {
            assert!(
                serde_json::from_str::<SubmitGoalPlanSectionInput>(payload).is_err(),
                "submit input must reject document syntax / unknown fields: {payload}"
            );
        }
    }

    fn view() -> GoalView {
        GoalView {
            goal_id: "g1".into(),
            objective: "ship safely".into(),
            objective_revision: 1,
            status: "active".into(),
            phase: "planning".into(),
            token_budget: None,
            tokens_used: 0,
            plan_revision: 1,
            board_revision: 1,
            tasks: Vec::new(),
            plan_markdown: String::new(),
            verifier_feedback: None,
        }
    }

    fn snapshot(role: GoalSubagentRole) -> GoalContextSnapshotResource {
        GoalContextSnapshotResource(Some(GoalContextSnapshot { role, view: view() }))
    }

    fn test_ctx(resources: crate::types::resources::Resources) -> tool_runtime::ToolCallContext {
        let mut ctx = tool_runtime::ToolCallContext::new(tool_protocol::ToolCallId::new_v7());
        ctx.extensions.insert(resources.into_shared());
        ctx
    }

    fn section_input() -> SubmitGoalPlanSectionInput {
        serde_json::from_str(r#"{"section": {"goal_acceptance": {"items": ["tests pass"]}}}"#)
            .unwrap()
    }

    fn error_code(error: &tool_runtime::ToolError) -> String {
        error
            .details
            .as_ref()
            .and_then(|details| details["code"].as_str().map(str::to_string))
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn plan_submit_fails_closed_outside_the_delegated_planner_stage() {
        // Primary session shape: a runtime handle but no delegated snapshot.
        let (runtime_tx, _runtime_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(GoalRuntimeHandle(runtime_tx));
        let error = SubmitGoalPlanSectionTool
            .run(test_ctx(resources), section_input())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "goal_stage_submit_unavailable");

        // Delegated verifier: snapshot present, role is not Planner.
        let (submit_tx, _submit_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(snapshot(GoalSubagentRole::Verifier));
        resources.insert(GoalStageSubmitHandle(submit_tx, StageToken(7)));
        let error = SubmitGoalPlanSectionTool
            .run(test_ctx(resources), section_input())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "delegated_goal_read_only");

        // Delegated planner without a submit handle.
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(snapshot(GoalSubagentRole::Planner));
        let error = SubmitGoalPlanSectionTool
            .run(test_ctx(resources), section_input())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "goal_stage_submit_unavailable");
    }

    #[tokio::test]
    async fn planner_submissions_carry_the_stage_token_and_return_structured_results() {
        let (submit_tx, mut submit_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(snapshot(GoalSubagentRole::Planner));
        resources.insert(GoalStageSubmitHandle(submit_tx, StageToken(42)));
        let ctx = test_ctx(resources);

        tokio::spawn(async move {
            while let Some(GoalPlanCommand::SubmitPlanSection {
                stage,
                section,
                respond_to,
            }) = submit_rx.recv().await
            {
                assert_eq!(stage, StageToken(42));
                assert!(matches!(
                    section,
                    tool_types::GoalPlanSectionPayload::GoalAcceptance { .. }
                ));
                let _ = respond_to.send(Ok(SubmitGoalPlanSectionOutput {
                    accepted_sections: vec!["plan_tasks".into()],
                    issues: vec![tool_types::GoalPlanAssemblyIssue {
                        path: "tasks[0].summary".into(),
                        reason: "task summary must not be empty".into(),
                    }],
                }));
            }
        });

        let output = SubmitGoalPlanSectionTool
            .run(ctx, section_input())
            .await
            .unwrap();
        assert_eq!(output.accepted_sections, ["plan_tasks"]);
        assert_eq!(output.issues.len(), 1);
        assert_eq!(output.issues[0].path, "tasks[0].summary");
    }

    #[tokio::test]
    async fn finalize_returns_the_actor_summary_and_committed_view() {
        let (submit_tx, mut submit_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(snapshot(GoalSubagentRole::Planner));
        resources.insert(GoalStageSubmitHandle(submit_tx, StageToken(42)));
        tokio::spawn(async move {
            while let Some(GoalPlanCommand::FinalizePlan { stage, respond_to }) =
                submit_rx.recv().await
            {
                assert_eq!(stage, StageToken(42));
                let _ = respond_to.send(Ok(FinalizeGoalPlanOutput {
                    summary: "board committed".into(),
                    view: view(),
                }));
            }
        });
        let finalize = super::FinalizeGoalPlanTool
            .run(test_ctx(resources), FinalizeGoalPlanInput {})
            .await
            .unwrap();
        assert_eq!(finalize.summary, "board committed");
        assert_eq!(finalize.view.goal_id, "g1");
    }

    #[tokio::test]
    async fn closed_planner_channel_fails_closed() {
        let error = SubmitGoalPlanSectionTool
            .run(
                test_ctx(planner_resources_with_dropped_channel()),
                section_input(),
            )
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "goal_channel_closed");
    }

    fn planner_resources_with_dropped_channel() -> crate::types::resources::Resources {
        let (submit_tx, submit_rx) = tokio::sync::mpsc::unbounded_channel();
        drop(submit_rx);
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(snapshot(GoalSubagentRole::Planner));
        resources.insert(GoalStageSubmitHandle(submit_tx, StageToken(42)));
        resources
    }

    #[tokio::test]
    async fn actor_rejections_map_to_structured_tool_errors() {
        let (submit_tx, mut submit_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut resources = crate::types::resources::Resources::default();
        resources.insert(snapshot(GoalSubagentRole::Planner));
        resources.insert(GoalStageSubmitHandle(submit_tx, StageToken(9)));
        tokio::spawn(async move {
            while let Some(GoalPlanCommand::SubmitPlanSection { respond_to, .. }) =
                submit_rx.recv().await
            {
                let _ = respond_to.send(Err("planning stage lease expired".into()));
            }
        });
        let error = SubmitGoalPlanSectionTool
            .run(test_ctx(resources), section_input())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "goal_plan_submit_rejected");
        assert!(error.detail.contains("planning stage lease expired"));
    }

    #[test]
    fn tool_names_are_stable_wire_identifiers() {
        assert_eq!(
            SUBMIT_GOAL_PLAN_SECTION_TOOL_NAME,
            "submit_goal_plan_section"
        );
        assert_eq!(FINALIZE_GOAL_PLAN_TOOL_NAME, "finalize_goal_plan");
    }
}
