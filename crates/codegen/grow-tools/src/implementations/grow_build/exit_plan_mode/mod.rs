//! `ExitPlanMode` tool — new architecture (`Tool` trait).
//!
//! Signals that the agent has finished planning and is ready for the user to
//! review and approve the plan. The complete plan is supplied as tool input,
//! atomically persisted as a session-owned artifact, and surfaced via:
//!
//! 1. A `PlanModeExited` **notification** sent to the gateway/client, carrying
//!    the plan content so the client can present it for user approval.
//! 2. A structured **`ExitPlanModeOutput`** returned to the model, containing
//!    the plan content (or an empty-plan message).
//!
//! The actual approval flow (yes/no with feedback, context clear, mode
//! transition) happens on the client side — this tool just says "I'm done,
//! here's the plan."
//!
//! The artifact path is provided by the host through `PlanFilePath`. It is
//! control-plane state, not a workspace edit capability.

pub mod types;

pub use types::{ExitPlanModeExtRequest, ExitPlanModeExtResponse};

use crate::notification::types::PlanModeExited;
use crate::types::output::ExitPlanModeOutput;
use crate::types::resources::{NotificationHandle, require_plan_file_path};
use crate::types::tool::{ToolKind, ToolNamespace};
use anyhow::Context as _;

/// Input for the `ExitPlanMode` tool.
///
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ExitPlanModeInput {
    /// Complete, decision-ready plan to persist and present for approval.
    pub plan: String,
}

/// `ExitPlanMode` tool.
///
/// Persists the supplied plan and signals to the orchestration layer that the
/// agent is done planning. The client receives a `PlanModeExited`
/// notification with the plan content and is responsible for presenting the
/// approval UI.
///
/// The plan is required and must contain non-whitespace content.
#[derive(Debug, Default)]
pub struct ExitPlanModeTool;

impl crate::types::tool_metadata::ToolMetadata for ExitPlanModeTool {
    fn kind(&self) -> ToolKind {
        ToolKind::ExitPlan
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn emitted_notifications(&self) -> &'static [&'static str] {
        &["PlanModeExited"]
    }

    fn description_template(&self) -> &str {
        r#"Submit the complete plan, persist it as session state, and present it to the user for approval. The `plan` argument is required and must contain the full decision-ready plan."#
    }
}

impl xai_tool_runtime::Tool for ExitPlanModeTool {
    type Args = ExitPlanModeInput;
    type Output = ExitPlanModeOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("exit_plan_mode").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "exit_plan_mode",
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

    #[tracing::instrument(name = "tool.exit_plan_mode", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: ExitPlanModeInput,
    ) -> Result<ExitPlanModeOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let plan_content = input.plan.trim();
        if plan_content.is_empty() {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "plan must contain non-whitespace content",
            ));
        }
        let plan_content = plan_content.to_owned();

        let (plan_path, plan_file_path) = {
            let res = resources.lock().await;
            require_plan_file_path(&res)?
        };

        persist_plan_artifact_atomic(&plan_path, plan_content.as_bytes())
            .await
            .map_err(|error| {
                xai_tool_runtime::ToolError::custom(
                    "plan_artifact_write_failed",
                    format!("failed to persist plan artifact: {error:#}"),
                )
            })?;

        // Notify only after durable persistence. A failed write must never open
        // approval UI or transition the behavior.
        {
            let res = resources.lock().await;
            if let Some(handle) = res.get::<NotificationHandle>() {
                handle.0.send_plan_mode_exited(PlanModeExited {
                    tool_call_id: ctx.call_id.as_str().to_owned(),
                    plan_content: Some(plan_content.clone()),
                    plan_file_path: plan_file_path.clone(),
                });
            }
        }

        tracing::info!(
            plan_chars = plan_content.len(),
            "Submitted plan for approval"
        );

        Ok(ExitPlanModeOutput::PlanReady {
            message: "The plan was persisted and submitted for user approval.".to_owned(),
            plan_content,
            plan_file_path,
        })
    }
}

async fn persist_plan_artifact_atomic(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let path = path.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let parent = path
            .parent()
            .context("plan artifact path has no parent directory")?;
        std::fs::create_dir_all(parent).context("create plan artifact directory")?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).context("create temp artifact")?;
        use std::io::Write as _;
        temp.write_all(&bytes).context("write plan artifact")?;
        temp.as_file().sync_all().context("fsync plan artifact")?;
        temp.persist(&path)
            .with_context(|| format!("persist plan artifact to {}", path.display()))?;
        Ok(())
    })
    .await
    .context("plan artifact writer panicked")??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keeps_exit_plan_toolkind() {
        let tool = ExitPlanModeTool;
        assert_eq!(
            crate::types::tool_metadata::ToolMetadata::kind(&tool),
            ToolKind::ExitPlan
        );
    }

    #[test]
    fn input_requires_non_optional_plan_field() {
        assert!(serde_json::from_value::<ExitPlanModeInput>(serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn atomic_persistence_replaces_the_session_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.md");
        persist_plan_artifact_atomic(&path, b"first").await.unwrap();
        persist_plan_artifact_atomic(&path, b"second")
            .await
            .unwrap();
        assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "second");
    }

    #[tokio::test]
    async fn atomic_persistence_failure_leaves_no_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let blocked_parent = dir.path().join("not-a-directory");
        std::fs::write(&blocked_parent, "occupied").unwrap();
        let path = blocked_parent.join("plan.md");

        assert!(persist_plan_artifact_atomic(&path, b"plan").await.is_err());
        assert!(!path.exists());
    }
}
