//! Typed control-plane interface for the active Plan Behavior.
//!
//! The tool is intentionally stateless. The shell owns phase validation,
//! candidate persistence, user approval, approved-contract freezing, and
//! Behavior transitions. Keeping those responsibilities in one place prevents
//! a tool notification from becoming a second state-transition path.

pub mod types;

pub use types::{PlanApprovalExtRequest, PlanApprovalExtResponse, PlanApprovalOutcome};

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
#[serde(deny_unknown_fields)]
pub struct PlanControlInput {
    pub action: PlanControlAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
}

impl PlanControlInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self.action {
            PlanControlAction::Submit | PlanControlAction::Amend => {
                if !self
                    .plan
                    .as_deref()
                    .is_some_and(|plan| !plan.trim().is_empty())
                {
                    return Err("`plan` is required for submit/amend");
                }
                if self.report.is_some() {
                    return Err("`report` is forbidden for submit/amend");
                }
            }
            PlanControlAction::Complete => {
                if self.plan.is_some() {
                    return Err("`plan` is forbidden for complete");
                }
                if !self
                    .report
                    .as_deref()
                    .is_some_and(|report| !report.trim().is_empty())
                {
                    return Err("a non-empty `report` is required for complete");
                }
            }
            PlanControlAction::Cancel => {
                if self.plan.is_some() {
                    return Err("`plan` is forbidden for cancel");
                }
                if self.report.is_some() {
                    return Err("`report` is forbidden for cancel");
                }
            }
        }
        Ok(())
    }
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
        r#"Control the active Plan lifecycle. Use action=`submit` with the complete initial plan to request approval; action=`amend` with a complete replacement plan when execution must materially deviate; action=`complete` only after every approved step and verification has finished, with `report` containing the final user-facing result; or action=`cancel` to abandon the Plan. `plan` is required only for submit/amend. `report` is required only for complete. Approval is always explicit and execution remains blocked until the user approves."#
    }

    fn isolates_batch_preflight(&self) -> bool {
        true
    }
}

impl tool_runtime::Tool for PlanControlTool {
    type Args = PlanControlInput;
    type Output = PlanControlOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("plan_control").expect("valid tool id")
    }

    fn description(&self, _ctx: &::tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "plan_control",
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::None,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        _ctx: tool_runtime::ToolCallContext,
        input: PlanControlInput,
    ) -> Result<PlanControlOutput, tool_runtime::ToolError> {
        input
            .validate()
            .map_err(tool_runtime::ToolError::invalid_arguments)?;

        let message = match input.action {
            PlanControlAction::Submit | PlanControlAction::Amend => {
                "The plan was approved and is now the frozen execution contract."
            }
            PlanControlAction::Complete => {
                return Ok(PlanControlOutput {
                    message: format!(
                        "The approved Plan is complete.\n\n{}",
                        input
                            .report
                            .as_deref()
                            .expect("validated complete report")
                            .trim()
                    ),
                });
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

    #[test]
    fn complete_requires_only_a_non_empty_report() {
        let valid: PlanControlInput = serde_json::from_value(serde_json::json!({
            "action": "complete",
            "report": "Implemented and verified."
        }))
        .unwrap();
        assert!(valid.validate().is_ok());

        for invalid in [
            serde_json::json!({"action": "complete"}),
            serde_json::json!({"action": "complete", "report": "  "}),
            serde_json::json!({"action": "complete", "report": "done", "plan": "# stale"}),
        ] {
            let input: PlanControlInput = serde_json::from_value(invalid).unwrap();
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn each_action_rejects_fields_owned_by_another_action() {
        for invalid in [
            serde_json::json!({"action": "submit", "plan": "# Plan", "report": "done"}),
            serde_json::json!({"action": "amend", "plan": "# Plan", "report": "done"}),
            serde_json::json!({"action": "cancel", "plan": "# stale"}),
            serde_json::json!({"action": "cancel", "report": "done"}),
        ] {
            let input: PlanControlInput = serde_json::from_value(invalid).unwrap();
            assert!(input.validate().is_err());
        }
        assert!(
            serde_json::from_value::<PlanControlInput>(serde_json::json!({
                "action": "complete",
                "report": "done",
                "unexpected": true
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn complete_returns_the_final_report_verbatim_after_trimming() {
        let output = tool_runtime::Tool::run(
            &PlanControlTool,
            tool_runtime::ToolCallContext::default(),
            PlanControlInput {
                action: PlanControlAction::Complete,
                plan: None,
                report: Some("  Implemented and verified.  ".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            output.message,
            "The approved Plan is complete.\n\nImplemented and verified."
        );
    }
}
