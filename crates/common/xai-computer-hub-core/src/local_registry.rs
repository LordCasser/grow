//! In-process registry used by Grow's local tool dispatcher.
//!
//! The registry owns type-erased tool handles and never opens a socket or
//! performs remote discovery. Entries preserve registration order so the
//! model-facing tool list remains deterministic.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::RwLock;
use serde_json::Value;
use xai_tool_protocol::ToolId;
use xai_tool_runtime::{ListToolsContext, Tool, ToolCallContext, ToolStream, TypedToolOutput};
use xai_tool_types::ToolDescription;

use crate::{ErasedTool, ToolHandle};

/// Extracts model-facing content from a typed tool result.
pub type ModelOutputExtractor =
    Arc<dyn Fn(&Value) -> Option<Vec<xai_tool_runtime::ContentBlock>> + Send + Sync>;

/// Build a type-safe model-output extractor for a tool output type.
pub fn extractor_for<T>() -> ModelOutputExtractor
where
    T: xai_tool_runtime::ToolOutput + serde::de::DeserializeOwned + 'static,
{
    Arc::new(|value: &Value| {
        serde_json::from_value::<T>(value.clone())
            .ok()
            .map(|output| output.model_output().to_vec())
    })
}

#[derive(Default)]
struct LocalRegistryState {
    entries: IndexMap<ToolId, Arc<dyn ToolHandle>>,
    extractors: IndexMap<ToolId, ModelOutputExtractor>,
}

/// Concurrency-safe registry for tools dispatched inside the current process.
#[derive(Clone, Default)]
pub struct LocalRegistry {
    state: Arc<RwLock<LocalRegistryState>>,
}

impl std::fmt::Debug for LocalRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.read();
        f.debug_struct("LocalRegistry")
            .field("entries", &state.entries.len())
            .field("extractors", &state.extractors.len())
            .finish()
    }
}

impl LocalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&self, tool: T) -> Option<Arc<dyn ToolHandle>>
    where
        T: Tool + std::fmt::Debug + 'static,
    {
        self.register_arc(Arc::new(tool))
    }

    pub fn register_arc<T>(&self, tool: Arc<T>) -> Option<Arc<dyn ToolHandle>>
    where
        T: Tool + std::fmt::Debug + 'static,
    {
        let id = tool.id();
        let handle: Arc<dyn ToolHandle> = Arc::new(ErasedTool::from_arc(tool));
        self.state.write().entries.insert(id, handle)
    }

    pub fn register_dyn(
        &self,
        tool: Arc<dyn xai_tool_runtime::ToolDyn>,
    ) -> Option<Arc<dyn ToolHandle>> {
        let id = tool.id();
        let handle: Arc<dyn ToolHandle> = Arc::new(DynToolAdapter(tool));
        self.state.write().entries.insert(id, handle)
    }

    pub fn register_with_model_output<T>(&self, tool: T) -> Option<Arc<dyn ToolHandle>>
    where
        T: Tool + std::fmt::Debug + 'static,
        T::Output: xai_tool_runtime::ToolOutput + serde::de::DeserializeOwned + 'static,
    {
        let id = tool.id();
        self.register_extractor(id, extractor_for::<T::Output>());
        self.register(tool)
    }

    pub fn register_extractor(&self, tool_id: ToolId, extractor: ModelOutputExtractor) {
        self.state.write().extractors.insert(tool_id, extractor);
    }

    pub fn register_alias(&self, alias_id: ToolId, target_id: &ToolId) -> bool {
        let mut state = self.state.write();
        let Some(handle) = state.entries.get(target_id).cloned() else {
            return false;
        };
        if let Some(extractor) = state.extractors.get(target_id).cloned() {
            state.extractors.insert(alias_id.clone(), extractor);
        }
        state.entries.insert(alias_id, handle);
        true
    }

    pub fn find(&self, tool_id: &ToolId) -> Option<Arc<dyn ToolHandle>> {
        self.state.read().entries.get(tool_id).cloned()
    }

    pub fn unregister(&self, tool_id: &ToolId) -> bool {
        let mut state = self.state.write();
        state.extractors.shift_remove(tool_id);
        state.entries.shift_remove(tool_id).is_some()
    }

    pub fn len(&self) -> usize {
        self.state.read().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.read().entries.is_empty()
    }

    pub fn contains(&self, tool_id: &ToolId) -> bool {
        self.state.read().entries.contains_key(tool_id)
    }

    pub fn model_output(
        &self,
        tool_id: &ToolId,
        output: &Value,
    ) -> Option<Vec<xai_tool_runtime::ContentBlock>> {
        let extractor = self.state.read().extractors.get(tool_id).cloned()?;
        extractor(output)
    }

    pub fn list_tools(&self, ctx: &ListToolsContext) -> Vec<ToolDescription> {
        self.state
            .read()
            .entries
            .values()
            .filter(|handle| handle.should_list(ctx))
            .map(|handle| handle.description(ctx))
            .collect()
    }
}

struct DynToolAdapter(Arc<dyn xai_tool_runtime::ToolDyn>);

impl std::fmt::Debug for DynToolAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynToolAdapter")
            .field("id", &self.0.id())
            .finish()
    }
}

#[async_trait::async_trait]
impl ToolHandle for DynToolAdapter {
    fn id(&self) -> ToolId {
        self.0.id()
    }

    fn description(&self, ctx: &ListToolsContext) -> ToolDescription {
        self.0.description(ctx)
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        self.0.capabilities()
    }

    fn should_list(&self, ctx: &ListToolsContext) -> bool {
        self.0.should_list(ctx)
    }

    async fn execute(&self, ctx: ToolCallContext, args: Value) -> ToolStream<TypedToolOutput> {
        self.0.execute(ctx, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use xai_tool_runtime::{ToolError, ToolOutput};

    #[derive(Debug, Deserialize, JsonSchema)]
    struct Args {
        value: String,
    }

    #[derive(Debug, Serialize)]
    struct Output {
        value: String,
    }

    impl ToolOutput for Output {}

    #[derive(Debug)]
    struct Echo;

    impl Tool for Echo {
        type Args = Args;
        type Output = Output;

        fn id(&self) -> ToolId {
            ToolId::new("echo").unwrap()
        }

        fn description(&self, _ctx: &ListToolsContext) -> ToolDescription {
            ToolDescription::new("echo", "echo a value")
        }

        async fn run(&self, _ctx: ToolCallContext, args: Args) -> Result<Output, ToolError> {
            Ok(Output { value: args.value })
        }
    }

    #[test]
    fn register_find_alias_and_unregister_are_local() {
        let registry = LocalRegistry::new();
        registry.register(Echo);
        let echo = ToolId::new("echo").unwrap();
        let alias = ToolId::new("echo_alias").unwrap();
        assert!(registry.find(&echo).is_some());
        assert!(registry.register_alias(alias.clone(), &echo));
        assert!(registry.find(&alias).is_some());
        assert!(registry.unregister(&alias));
        assert!(registry.find(&alias).is_none());
        assert!(registry.find(&echo).is_some());
    }
}
