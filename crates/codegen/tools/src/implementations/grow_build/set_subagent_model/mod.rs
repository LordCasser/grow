//! Parent-bound sampling control for an active directly delegated child.

use crate::implementations::grow_build::task::backend::SubagentBackendResource;
use crate::types::tool::{ToolKind, ToolNamespace};
use sampling_types::ReasoningEffort;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub const SET_SUBAGENT_MODEL_TOOL_NAME: &str = "set_subagent_model";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSubagentModelInput {
    /// Exact task ID of an active child directly delegated by this agent.
    pub subagent_id: String,
    /// Selectable catalog model ID. Omission or null keeps the child's desired model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Canonical effort: none, minimal, low, medium, high, xhigh, or max.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl SetSubagentModelInput {
    pub fn validate(&self) -> Result<Option<ReasoningEffort>, tool_runtime::ToolError> {
        if self.subagent_id.trim().is_empty() {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "subagent_id must be nonempty",
            ));
        }
        if self.model.is_none() && self.effort.is_none() {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "at least one of model or effort is required",
            ));
        }
        self.effort
            .as_ref()
            .map(|effort| {
                serde_json::from_value(serde_json::Value::String(effort.clone())).map_err(|_| {
                    tool_runtime::ToolError::invalid_arguments(
                        "effort must be a canonical string: none, minimal, low, medium, high, xhigh, or max",
                    )
                })
            })
            .transpose()
    }
}

/// The backend stamps the source identity; the coordinator supplies the exact
/// active child's runtime context after checking direct ownership.
#[derive(Debug)]
pub struct SubagentModelControlRequest {
    pub source_session_id: String,
    pub subagent_id: String,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub cancellation: CancellationToken,
    /// Target actor admission only, independent of selection application.
    pub respond_to: oneshot::Sender<Result<(), String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentModelStatus {
    Queued,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SetSubagentModelOutput {
    pub subagent_id: String,
    pub status: SubagentModelStatus,
}

impl tool_runtime::ToolOutput for SetSubagentModelOutput {}

#[derive(Debug, Default)]
pub struct SetSubagentModelTool;

impl crate::types::tool_metadata::ToolMetadata for SetSubagentModelTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Queue a model and/or reasoning effort selection for an active subagent directly delegated by this agent. Supply its exact subagent_id and at least one of model or effort. Model-only uses that model's user-selection default effort; effort-only patches the child's newest desired model. Effort must be canonical: none, minimal, low, medium, high, xhigh, or max. Null means omitted. A queued result confirms target actor admission; the selection applies at the child's next closed-Step boundary without restarting its current request."
    }
}

impl tool_runtime::Tool for SetSubagentModelTool {
    type Args = SetSubagentModelInput;
    type Output = SetSubagentModelOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(SET_SUBAGENT_MODEL_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            SET_SUBAGENT_MODEL_TOOL_NAME,
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
        ctx: tool_runtime::ToolCallContext,
        input: Self::Args,
    ) -> Result<Self::Output, tool_runtime::ToolError> {
        let effort = input.validate()?;
        let backend = {
            let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
            let resources = resources.lock().await;
            resources.require::<SubagentBackendResource>()?.clone()
        };
        let cancellation = ctx
            .get::<tool_runtime::Cancellation>()
            .map_or_else(CancellationToken::new, |token| token.0.clone());
        backend
            .backend()
            .set_model(input.subagent_id.clone(), input.model, effort, cancellation)
            .await
            .map_err(|error| tool_runtime::ToolError::custom("subagent_model_rejected", error))?;
        Ok(SetSubagentModelOutput {
            subagent_id: input.subagent_id,
            status: SubagentModelStatus::Queued,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::grow_build::task::backend::ChannelBackend;
    use crate::implementations::grow_build::task::types::SubagentEvent;
    use crate::types::{resources::Resources, tool_metadata::test_ctx};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn queued_requires_target_admission_and_closed_receipt_fails() {
        for admission in [Some(Ok(())), Some(Err("actor rejected".to_owned())), None] {
            let (sender, mut receiver) = mpsc::unbounded_channel();
            let mut resources = Resources::new();
            resources.insert(ChannelBackend::for_session(sender, "host-parent").into_resource());
            let cancellation = CancellationToken::new();
            let mut ctx = test_ctx(resources.into_shared());
            ctx.insert(tool_runtime::Cancellation(cancellation.clone()));
            let call = tool_runtime::Tool::run(
                &SetSubagentModelTool,
                ctx,
                SetSubagentModelInput {
                    subagent_id: "exact-child".into(),
                    model: None,
                    effort: Some("none".into()),
                },
            );
            tokio::pin!(call);
            let SubagentEvent::SetModel(request) = (tokio::select! {
                request = receiver.recv() => request.unwrap(),
                result = &mut call => panic!("send alone cannot confirm admission: {result:?}"),
            }) else {
                panic!("expected model control request");
            };
            assert_eq!(request.source_session_id, "host-parent");
            assert_eq!(request.subagent_id, "exact-child");
            assert_eq!(request.effort, Some(ReasoningEffort::None));
            assert!(request.model.is_none());
            assert!(futures::poll!(&mut call).is_pending());
            match admission {
                Some(result) => {
                    let accepted = result.is_ok();
                    request.respond_to.send(result).unwrap();
                    let result = call.await;
                    if accepted {
                        assert_eq!(
                            serde_json::to_value(result.unwrap()).unwrap(),
                            serde_json::json!({"subagent_id":"exact-child","status":"queued"})
                        );
                    } else {
                        assert!(result.unwrap_err().detail.contains("actor rejected"));
                    }
                }
                None => {
                    drop(request.respond_to);
                    assert!(
                        call.await
                            .unwrap_err()
                            .detail
                            .contains("before confirming admission")
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn invalid_selection_never_reaches_the_backend() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut resources = Resources::new();
        resources.insert(ChannelBackend::for_session(sender, "host-parent").into_resource());
        let resources = resources.into_shared();
        for input in [
            SetSubagentModelInput {
                subagent_id: "child".into(),
                model: None,
                effort: None,
            },
            SetSubagentModelInput {
                subagent_id: " ".into(),
                model: Some("catalog-model".into()),
                effort: None,
            },
            SetSubagentModelInput {
                subagent_id: "child".into(),
                model: None,
                effort: Some("HIGH".into()),
            },
        ] {
            assert!(
                tool_runtime::Tool::run(&SetSubagentModelTool, test_ctx(resources.clone()), input,)
                    .await
                    .is_err()
            );
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn schema_requires_exact_target_and_has_no_source_or_legacy_fields() {
        let schema = serde_json::to_value(schemars::schema_for!(SetSubagentModelInput)).unwrap();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], serde_json::json!(["subagent_id"]));
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 3);
        for name in ["subagent_id", "model", "effort"] {
            assert!(properties.contains_key(name));
        }
    }

    #[test]
    fn input_is_strict_and_null_is_omitted_while_none_is_explicit() {
        for json in [
            serde_json::json!({"subagent_id":"child"}),
            serde_json::json!({"subagent_id":"child","model":null,"effort":null}),
            serde_json::json!({"subagent_id":"","model":"test"}),
            serde_json::json!({"subagent_id":" \n","effort":"none"}),
            serde_json::json!({"subagent_id":"child","effort":"HIGH"}),
            serde_json::json!({"subagent_id":"child","effort":"invalid"}),
        ] {
            assert!(
                serde_json::from_value::<SetSubagentModelInput>(json)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for json in [
            serde_json::json!({"model":"test"}),
            serde_json::json!({"subagent_id":null,"model":"test"}),
            serde_json::json!({"subagent_id":1,"model":"test"}),
            serde_json::json!({"subagent_id":"child","model":"test","source_session_id":"other"}),
            serde_json::json!({"subagent_id":"child","model":"test","session_id":"other"}),
            serde_json::json!({"subagent_id":"child","model":"test","meta":{"intent":"forged"}}),
            serde_json::json!({"subagent_id":"child","model":"test","client_id":"forged"}),
            serde_json::json!({"subagent_id":"child","effort":1}),
        ] {
            assert!(serde_json::from_value::<SetSubagentModelInput>(json).is_err());
        }
        for effort in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
            let input: SetSubagentModelInput = serde_json::from_value(
                serde_json::json!({"subagent_id":"child","model":null,"effort":effort}),
            )
            .unwrap();
            assert!(input.model.is_none());
            assert!(input.validate().unwrap().is_some());
        }
        let input: SetSubagentModelInput = serde_json::from_value(
            serde_json::json!({"subagent_id":"child","model":"test","effort":null}),
        )
        .unwrap();
        assert!(input.validate().unwrap().is_none());
    }
}
