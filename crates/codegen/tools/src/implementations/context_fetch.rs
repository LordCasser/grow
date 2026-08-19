//! Read-only reprojection of context shadowed by compaction.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

pub const CONTEXT_FETCH_TOOL_NAME: &str = "context_fetch";
pub const DEFAULT_CONTEXT_FETCH_LIMIT: usize = 4;
pub const MAX_CONTEXT_FETCH_LIMIT: usize = 20;

fn default_limit() -> usize {
    DEFAULT_CONTEXT_FETCH_LIMIT
}

/// An immutable reference emitted by a compaction summary, plus page bounds.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContextFetchInput {
    /// Timeline that owns the compaction summary.
    pub timeline_id: String,
    /// First event in the reference. Currently equal to `last_seq` because the
    /// referenced compaction summary carries the exact shadowed Surface range.
    pub first_seq: u64,
    /// Last event in the reference.
    pub last_seq: u64,
    /// Zero-based item offset within the shadowed Surface range.
    #[serde(default)]
    pub offset: usize,
    /// Maximum number of shadowed Surface items to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl ContextFetchInput {
    fn validate(&self) -> Result<(), String> {
        if self.timeline_id.trim().is_empty() {
            return Err("timeline_id must not be empty".into());
        }
        if self.first_seq != self.last_seq {
            return Err(
                "context_fetch currently accepts a single-event compaction reference"
                    .into(),
            );
        }
        if !(1..=MAX_CONTEXT_FETCH_LIMIT).contains(&self.limit) {
            return Err(format!(
                "limit must be between 1 and {MAX_CONTEXT_FETCH_LIMIT}"
            ));
        }
        Ok(())
    }
}

/// Session-owned resolver. The tools crate owns only the read-only contract;
/// Timeline access stays in the shell/chat-state layers.
#[async_trait::async_trait]
pub trait ContextFetchBackend: Send + Sync {
    async fn fetch(
        &self,
        input: &ContextFetchInput,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Debug, Default)]
pub struct ContextFetchImpl;

impl crate::types::tool_metadata::ToolMetadata for ContextFetchImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::ContextFetch
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Recover a small page of original conversation context that was unloaded by compaction. \
         Call this only with the immutable timeline reference included in a compaction summary. \
         The operation is read-only: it reprojects the exact shadowed Surface items without \
         changing the current conversation. Start with a small page and fetch more only when the \
         missing detail is relevant."
    }
}

impl tool_runtime::Tool for ContextFetchImpl {
    type Args = ContextFetchInput;
    type Output = ToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(CONTEXT_FETCH_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            CONTEXT_FETCH_TOOL_NAME,
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
        input: ContextFetchInput,
    ) -> Result<ToolOutput, tool_runtime::ToolError> {
        input.validate().map_err(|message| {
            tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new(CONTEXT_FETCH_TOOL_NAME).expect("valid tool id"),
                message,
            )
        })?;

        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        let Some(backend) = resources
            .lock()
            .await
            .get::<Arc<dyn ContextFetchBackend>>()
            .cloned()
        else {
            return Err(tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new(CONTEXT_FETCH_TOOL_NAME).expect("valid tool id"),
                "context reprojection is unavailable for this session",
            ));
        };

        let output = backend.fetch(&input).await.map_err(|error| {
            tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new(CONTEXT_FETCH_TOOL_NAME).expect("valid tool id"),
                format!("context fetch failed: {error}"),
            )
        })?;
        Ok(ToolOutput::Text(output.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ContextFetchInput {
        ContextFetchInput {
            timeline_id: "session-1".into(),
            first_seq: 42,
            last_seq: 42,
            offset: 0,
            limit: DEFAULT_CONTEXT_FETCH_LIMIT,
        }
    }

    #[test]
    fn accepts_single_event_compaction_reference() {
        assert_eq!(input().validate(), Ok(()));
    }

    #[test]
    fn rejects_arbitrary_ranges_and_unbounded_pages() {
        let mut value = input();
        value.last_seq += 1;
        assert!(value.validate().is_err());

        let mut value = input();
        value.limit = MAX_CONTEXT_FETCH_LIMIT + 1;
        assert!(value.validate().is_err());
    }
}
