//! `lsp` tool - code intelligence via language servers.
//!
//! Implementation is in `implementations::lsp`. This module provides the
//! `LspTool` (Tool trait impl) under the `Grow` namespace.

use std::sync::Arc;

use crate::implementations::lsp::{LspBackend, LspToolInput};
use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LspToolOutput(pub String);

impl tool_runtime::ToolOutput for LspToolOutput {}

impl From<LspToolOutput> for ToolOutput {
    fn from(o: LspToolOutput) -> Self {
        ToolOutput::Text(o.0.into())
    }
}

#[derive(Debug, Default)]
pub struct LspTool;

impl crate::types::tool_metadata::ToolMetadata for LspTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Lsp
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        r#"Code intelligence via language servers.${%- if tools.by_kind.search and tools.by_kind.read %} Prefer over ${{ tools.by_kind.search }}/${{ tools.by_kind.read }} for understanding code.${%- endif %}
Operations: goToDefinition (jump to where a symbol is defined), findReferences (all usages of a symbol), hover (type info/docs at a position), goToImplementation (trait/interface implementations), documentSymbol (list all symbols in a file), workspaceSymbol (search symbols by name across the workspace — requires query parameter, not file_path).
Requires file_path + line + character for position-based operations."#
    }

    fn emitted_notifications(&self) -> &'static [&'static str] {
        &[
            "LspServerCrashed",
            "LspServerFailed",
            "LspServerReady",
            "LspServerRetrying",
            "LspServerStarting",
        ]
    }
}

impl tool_runtime::Tool for LspTool {
    type Args = LspToolInput;
    type Output = LspToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("lsp").expect("valid tool id")
    }

    fn description(&self, _ctx: &::tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "lsp",
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::Read,
            ..Default::default()
        }
    }

    #[tracing::instrument(
        name = "tool.lsp",
        skip_all,
        fields(operation = %input.operation)
    )]
    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: LspToolInput,
    ) -> Result<LspToolOutput, tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let handle;
        {
            let res = resources.lock().await;
            handle = res
                .get::<Arc<dyn LspBackend>>()
                .ok_or_else(|| {
                    tool_runtime::ToolError::custom(
                        "process_manager",
                        "LSP tool is unavailable. Configure ~/.grow/lsp.json or <cwd>/.grow/lsp.json and ensure the language server can start.",
                    )
                })?
                .clone();
        }

        let result = handle.dispatch(&input).await;
        if result.is_error {
            Err(tool_runtime::ToolError::custom(
                "process_manager",
                result.text,
            ))
        } else {
            Ok(LspToolOutput(result.text))
        }
    }
}
