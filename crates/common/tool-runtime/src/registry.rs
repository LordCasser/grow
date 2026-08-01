//! In-process tool registry and type-erased dispatch.

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use indexmap::IndexMap;
use parking_lot::RwLock;
use serde_json::Value;
use tool_protocol::{ToolCapabilities, ToolId};
use tool_types::ToolDescription;

use crate::{
    ListToolsContext, ModelOutputExtractor, Tool, ToolCallContext, ToolError, ToolOutput,
    ToolStream, ToolStreamItem, TypedToolOutput, extractor_for, terminal_only,
};

#[async_trait]
pub trait ToolHandle: Send + Sync + std::fmt::Debug {
    /// Stable identity used by the router to route calls.
    fn id(&self) -> ToolId;

    /// Model-facing description of the tool's argument schema.
    ///
    /// Receives the per-turn [`ListToolsContext`] so handles backed by a
    /// typed [`Tool`] can produce context-aware descriptions at listing
    /// time. Callers outside a listing turn pass
    /// [`ListToolsContext::default`].
    fn description(&self, ctx: &ListToolsContext) -> ToolDescription;

    /// Per-tool capability flags.
    fn capabilities(&self) -> ToolCapabilities;

    /// Per-turn listing predicate.
    fn should_list(&self, _ctx: &ListToolsContext) -> bool {
        true
    }

    /// Streaming execution entry point.
    ///
    /// Implementations encode the tool's typed `Output` to
    /// [`serde_json::Value`] and surface argument-decoding failures as
    /// [`ToolError::InvalidArguments`] within the terminal item.
    async fn execute(&self, ctx: ToolCallContext, args: Value) -> ToolStream<TypedToolOutput>;
}

/// Type-erasing wrapper for any [`Tool`] implementation.
///
/// Decodes `args` into `T::Args`, drives `T::execute`, and re-encodes each
/// `T::Output` (terminal and progress items pass through unchanged
/// otherwise). The wrapper holds the inner tool by `Arc` so the same
/// underlying instance can back multiple registrations cheaply.
pub struct ErasedTool<T> {
    inner: Arc<T>,
}

impl<T> ErasedTool<T> {
    /// Wrap an `Arc<T>` for use as an [`ToolHandle`].
    pub fn from_arc(inner: Arc<T>) -> Self {
        Self { inner }
    }

    /// Wrap an owned tool, taking the `Arc` allocation internally.
    pub fn new(inner: T) -> Self {
        Self::from_arc(Arc::new(inner))
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for ErasedTool<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErasedTool")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<T> Clone for ErasedTool<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[async_trait]
impl<T> ToolHandle for ErasedTool<T>
where
    T: Tool + std::fmt::Debug + 'static,
    T::Output: ToolOutput,
{
    fn id(&self) -> ToolId {
        self.inner.id()
    }

    fn description(&self, ctx: &ListToolsContext) -> ToolDescription {
        self.inner.description(ctx)
    }

    fn capabilities(&self) -> ToolCapabilities {
        self.inner.capabilities()
    }

    fn should_list(&self, ctx: &ListToolsContext) -> bool {
        self.inner.should_list(ctx)
    }

    async fn execute(&self, ctx: ToolCallContext, args: Value) -> ToolStream<TypedToolOutput> {
        let typed_args: T::Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => {
                return terminal_only(Err(ToolError::invalid_arguments(e.to_string())));
            }
        };
        let tool_id = self.inner.id();
        let stream = self.inner.execute(ctx, typed_args).await;
        let mapped = stream.map(move |item| match item {
            ToolStreamItem::Progress(p) => ToolStreamItem::Progress(p),
            ToolStreamItem::Terminal(Ok(out)) => match serde_json::to_value(&out) {
                Ok(value) => {
                    let custom = out.model_output();
                    let model_output = if custom.is_empty() {
                        crate::extract_content_blocks(&value)
                    } else {
                        custom
                    };
                    let chat_completion_output = out.chat_completion_output();
                    ToolStreamItem::Terminal(Ok(TypedToolOutput {
                        tool_id: tool_id.clone(),
                        value,
                        model_output,
                        chat_completion_output,
                    }))
                }
                Err(e) => ToolStreamItem::Terminal(Err(ToolError::custom(
                    "output_encoding",
                    e.to_string(),
                ))),
            },
            ToolStreamItem::Terminal(Err(err)) => ToolStreamItem::Terminal(Err(err)),
        });
        Box::pin(mapped)
    }
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

    pub fn register_dyn(&self, tool: Arc<dyn crate::ToolDyn>) -> Option<Arc<dyn ToolHandle>> {
        let id = tool.id();
        let handle: Arc<dyn ToolHandle> = Arc::new(DynToolAdapter(tool));
        self.state.write().entries.insert(id, handle)
    }

    pub fn register_with_model_output<T>(&self, tool: T) -> Option<Arc<dyn ToolHandle>>
    where
        T: Tool + std::fmt::Debug + 'static,
        T::Output: crate::ToolOutput + serde::de::DeserializeOwned + 'static,
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
    ) -> Option<Vec<crate::ContentBlock>> {
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

struct DynToolAdapter(Arc<dyn crate::ToolDyn>);

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

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
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
    use crate::{ToolError, ToolOutput};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

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
