//! `EnterPlanMode` tool — new architecture (`Tool` trait).
//!
//! Gateway tool that the agent calls when it decides a task is complex enough
//! to warrant a planning phase before writing code. This is the
//! **agent-initiated** entry path into plan mode.
//!
//! On success it only notifies orchestration (`PlanModeEntered`). Plan content
//! is submitted through `exit_plan_mode`; entering the behavior never writes
//! to either the workspace or session storage.
//!
//! ## User Consent
//!
//! This tool requires user approval before executing. The UI should present a
//! confirmation dialog. If the user declines, the tool result is rejected and
//! the model receives `"User declined to enter plan mode."`.

use crate::notification::types::PlanModeEntered;
use crate::types::output::{EnterPlanModeOutput, EnterPlanModeToolHints};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::NotificationHandle;
use crate::types::template_renderer::TemplateRenderer;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Input for the `EnterPlanMode` tool.
///
/// Empty object — no parameters. The decision to enter plan mode is a binary
/// gate. All configuration (workflow variant, explore agent count, etc.) comes
/// from feature flags and environment variables, not from the tool call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EnterPlanModeInput {}

/// `EnterPlanMode` tool: signals plan behavior entry without mutating storage.
///
/// Params: `()` — no per-tool configuration.
#[derive(Debug, Default)]
pub struct EnterPlanModeTool;

impl crate::types::tool_metadata::ToolMetadata for EnterPlanModeTool {
    fn kind(&self) -> ToolKind {
        ToolKind::EnterPlan
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn emitted_notifications(&self) -> &'static [&'static str] {
        &["PlanModeEntered"]
    }

    fn description_template(&self) -> &str {
        r#"Request Plan behavior when the work needs an explicit, reviewable approach before execution. Plan blocks ordinary file-edit calls; other already-authorized tools remain available for investigation under their normal permissions. Submit the complete plan through the exit-plan tool."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        // EnterPlanMode can only exist if ExitPlanMode is also registered —
        // entering plan mode without the ability to exit would be a dead-end.
        use crate::implementations::grow_build::exit_plan_mode::ExitPlanModeTool;
        Expr::Value(ToolRequirement::Tool {
            namespace: crate::types::tool_metadata::ToolMetadata::tool_namespace(&ExitPlanModeTool)
                .to_string(),
            id: xai_tool_runtime::Tool::id(&ExitPlanModeTool).to_string(),
            if_params: None,
        })
    }
}

impl xai_tool_runtime::Tool for EnterPlanModeTool {
    type Args = EnterPlanModeInput;
    type Output = EnterPlanModeOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("enter_plan_mode").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "enter_plan_mode",
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

    #[tracing::instrument(name = "tool.enter_plan_mode", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        _input: EnterPlanModeInput,
    ) -> Result<EnterPlanModeOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let tool_hints = {
            let res = resources.lock().await;

            let renderer = res.get::<TemplateRenderer>().ok_or_else(|| {
                xai_tool_runtime::ToolError::custom(
                    "plan_behavior_unavailable",
                    "Plan behavior requires a finalized exit-plan tool.",
                )
            })?;
            if renderer.tool_for_kind(ToolKind::ExitPlan).is_none() {
                return Err(xai_tool_runtime::ToolError::custom(
                    "plan_behavior_unavailable",
                    "Plan behavior cannot be activated because this Agent has no ExitPlan capability.",
                ));
            }

            if let Some(handle) = res.get::<NotificationHandle>() {
                handle.0.send_plan_mode_entered(PlanModeEntered {
                    tool_call_id: ctx.call_id.as_str().to_owned(),
                });
            }

            // Resolve client-facing tool names via TemplateRenderer.
            EnterPlanModeToolHints {
                ask_user: renderer
                    .render("${{ tools.by_kind.ask_user }}")
                    .unwrap_or_else(|_| "ask_user_question".to_owned()),
                exit_plan: renderer
                    .render("${{ tools.by_kind.exit_plan }}")
                    .unwrap_or_else(|_| "exit_plan_mode".to_owned()),
                task: renderer
                    .render("${{ tools.by_kind.task }}")
                    .unwrap_or_default(),
            }
        };

        tracing::info!("Entered plan behavior");

        Ok(EnterPlanModeOutput::Entered {
            message: "You have entered plan behavior. Investigate the request without making \
                      file edits, then submit the complete plan through the exit-plan tool."
                .to_string(),
            tool_hints,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keeps_enter_plan_toolkind() {
        let tool = EnterPlanModeTool;
        assert_eq!(
            crate::types::tool_metadata::ToolMetadata::kind(&tool),
            ToolKind::EnterPlan
        );
    }

    #[test]
    fn output_has_no_plan_artifact_path() {
        let output = EnterPlanModeOutput::Entered {
            message: "entered".to_string(),
            tool_hints: EnterPlanModeToolHints::default(),
        };
        let json = serde_json::to_value(output).unwrap();
        assert!(json.to_string().contains("entered"));
        assert!(!json.to_string().contains("plan_file"));
    }
}
