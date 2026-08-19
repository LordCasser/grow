//! Model-assisted recall over context unloaded by compaction.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

pub const CONTEXT_RECALL_TOOL_NAME: &str = "context_recall";
pub const MAX_CONTEXT_RECALL_QUERY_CHARS: usize = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContextRecallInput {
    /// What missing fact, decision, constraint, or prior work to recall.
    pub query: String,
}

impl ContextRecallInput {
    fn validate(&self) -> Result<(), String> {
        let query = self.query.trim();
        if query.is_empty() {
            return Err("query must not be empty".into());
        }
        if query.chars().count() > MAX_CONTEXT_RECALL_QUERY_CHARS {
            return Err(format!(
                "query must not exceed {MAX_CONTEXT_RECALL_QUERY_CHARS} characters"
            ));
        }
        Ok(())
    }
}

/// Session-owned recall service. A tool call crosses this boundary into the
/// calling agent's LocalSet, where shell opens and samples a durable Sideband.
#[async_trait::async_trait]
pub trait ContextRecallBackend: Send + Sync {
    async fn recall(&self, query: &str)
    -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Debug, Default)]
pub struct ContextRecallImpl;

impl crate::types::tool_metadata::ToolMetadata for ContextRecallImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::ContextRecall
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Recall a relevant fact, decision, constraint, or piece of prior work from conversation \
         context unloaded by compaction. This is a model-assisted retrieval operation over the \
         calling agent's own immutable session history: describe what you need, and a read-only \
         Sideband searches that history and returns a concise recollection. Use it only when the \
         compacted summary does not contain a detail needed for the current task. It does not \
         restore or expand old messages into the active conversation."
    }
}

impl tool_runtime::Tool for ContextRecallImpl {
    type Args = ContextRecallInput;
    type Output = ToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(CONTEXT_RECALL_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            CONTEXT_RECALL_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
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
        input: ContextRecallInput,
    ) -> Result<ToolOutput, tool_runtime::ToolError> {
        input.validate().map_err(|message| {
            tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new(CONTEXT_RECALL_TOOL_NAME).expect("valid tool id"),
                message,
            )
        })?;

        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        let Some(backend) = resources
            .lock()
            .await
            .get::<Arc<dyn ContextRecallBackend>>()
            .cloned()
        else {
            return Err(tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new(CONTEXT_RECALL_TOOL_NAME).expect("valid tool id"),
                "context recall is unavailable for this session",
            ));
        };

        let output = backend.recall(input.query.trim()).await.map_err(|error| {
            tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new(CONTEXT_RECALL_TOOL_NAME).expect("valid tool id"),
                format!("context recall failed: {error}"),
            )
        })?;
        Ok(ToolOutput::Text(output.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_query_must_be_specific_and_bounded() {
        assert!(
            ContextRecallInput { query: "  ".into() }
                .validate()
                .is_err()
        );
        assert!(
            ContextRecallInput {
                query: "x".repeat(MAX_CONTEXT_RECALL_QUERY_CHARS + 1),
            }
            .validate()
            .is_err()
        );
        assert!(
            ContextRecallInput {
                query: "the database migration decision".into(),
            }
            .validate()
            .is_ok()
        );
    }
}
