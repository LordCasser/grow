//! Typed control-plane interface for the active Plan Behavior.
//!
//! The tool is intentionally stateless. The shell owns phase validation,
//! candidate persistence, user approval, approved-contract freezing, and
//! Behavior transitions. Keeping those responsibilities in one place prevents
//! a tool notification from becoming a second state-transition path.

pub mod types;

pub use types::{PlanApprovalExtRequest, PlanApprovalExtResponse};

use crate::types::output::PlanControlOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanControlAction {
    Submit,
    Amend,
    Complete,
    Cancel,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct PlanControlInput {
    pub action: PlanControlAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

#[derive(Debug, Default)]
pub struct PlanControlTool;

impl crate::types::tool_metadata::ToolMetadata for PlanControlTool {
    fn kind(&self) -> ToolKind {
        ToolKind::PlanControl
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        r#"Control the active Plan lifecycle. Use action=`submit` with the complete initial plan to request approval; action=`amend` with a complete replacement plan when execution must materially deviate; action=`complete` only after every approved step and verification has finished; or action=`cancel` to abandon the Plan. `plan` is required only for submit/amend. Approval is always explicit and execution remains blocked until the user approves."#
    }
}

impl xai_tool_runtime::Tool for PlanControlTool {
    type Args = PlanControlInput;
    type Output = PlanControlOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("plan_control").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "plan_control",
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(xai_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        _ctx: xai_tool_runtime::ToolCallContext,
        input: PlanControlInput,
    ) -> Result<PlanControlOutput, xai_tool_runtime::ToolError> {
        let valid_plan_argument = match input.action {
            PlanControlAction::Submit | PlanControlAction::Amend => input
                .plan
                .as_deref()
                .is_some_and(|plan| !plan.trim().is_empty()),
            PlanControlAction::Complete | PlanControlAction::Cancel => input.plan.is_none(),
        };
        if !valid_plan_argument {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "`plan` is required for submit/amend and forbidden for complete/cancel",
            ));
        }

        let message = match input.action {
            PlanControlAction::Submit | PlanControlAction::Amend => {
                "The plan was approved and is now the frozen execution contract."
            }
            PlanControlAction::Complete => {
                "The approved Plan was completed and the session returned to Normal."
            }
            PlanControlAction::Cancel => {
                "The Plan was cancelled and the session returned to Normal."
            }
        };
        Ok(PlanControlOutput {
            message: message.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_uses_plan_control_kind() {
        assert_eq!(
            crate::types::tool_metadata::ToolMetadata::kind(&PlanControlTool),
            ToolKind::PlanControl
        );
    }

    #[test]
    fn input_requires_action() {
        assert!(serde_json::from_value::<PlanControlInput>(serde_json::json!({})).is_err());
    }
}
