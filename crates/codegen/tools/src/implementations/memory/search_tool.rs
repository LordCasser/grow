//! `memory_search` tool — new architecture (`Tool` trait).

use std::sync::Arc;

use super::types::MemorySearchInput;
use crate::types::memory_backend::{MemoryBackend, format_staleness_note};
use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

fn display_line_range(start_line: usize, end_line: usize) -> (usize, usize) {
    (start_line.saturating_add(1), end_line)
}

#[derive(Debug, Default)]
pub struct MemorySearchImpl;

impl crate::types::tool_metadata::ToolMetadata for MemorySearchImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::MemorySearch
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Search cross-session memory for relevant knowledge chunks. Returns ranked results \
         from global, workspace, and session memory files.\n\n\
         Use this proactively when:\n\
         - A question references prior work, decisions, or context you don't have\n\
         - You need project conventions, coding patterns, or user preferences\n\
         - The user mentions something discussed or decided in a previous session\n\
         - Starting work in an unfamiliar part of the codebase\n\
         - After compaction when prior context may have been lost"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::memory::get_tool::MemoryGetImpl;
    use crate::implementations::memory::types::MemoryGetInput;
    use crate::types::memory_backend::{MemoryBackend, MemorySearchResult};
    use crate::types::output::ToolOutput;
    use crate::types::resources::{Resources, SharedResources};
    use crate::types::tool_metadata::test_ctx;
    use std::sync::Arc;

    struct RangeBackend;

    #[async_trait::async_trait]
    impl MemoryBackend for RangeBackend {
        async fn search(
            &self,
            _query: &str,
            _max_results: usize,
            _min_score: f64,
        ) -> Result<Vec<MemorySearchResult>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![MemorySearchResult {
                chunk_id: "/tmp/memory-range.md:1".into(),
                path: "/tmp/memory-range.md".into(),
                start_line: 4,
                end_line: 5,
                score: 1.0,
                snippet: "search-get-tool-token".into(),
                source: "workspace".into(),
                created_at: None,
            }])
        }

        fn get(
            &self,
            path: &str,
            from: Option<usize>,
            lines: Option<usize>,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            assert_eq!(path, "/tmp/memory-range.md");
            assert_eq!(
                from,
                Some(4),
                "memory_get must convert displayed line 5 to offset 4"
            );
            assert_eq!(lines, Some(1));
            Ok("search-get-tool-token".into())
        }

        fn total_chunks(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
            Ok(1)
        }
    }

    fn shared_range_backend() -> SharedResources {
        let mut resources = Resources::new();
        let backend: Arc<dyn MemoryBackend> = Arc::new(RangeBackend);
        resources.insert(backend);
        resources.into_shared()
    }

    #[tokio::test]
    async fn search_output_range_round_trips_through_both_tools() {
        let shared = shared_range_backend();
        let search_output = tool_runtime::Tool::run(
            &MemorySearchImpl,
            test_ctx(shared.clone()),
            MemorySearchInput {
                query: "search-get-tool-token".into(),
                max_results: Some(1),
                min_score: Some(0.0),
            },
        )
        .await
        .unwrap();
        let ToolOutput::Text(search_text) = search_output else {
            panic!("memory_search must return text");
        };
        let range_line = search_text
            .text
            .lines()
            .find(|line| line.starts_with("**File:** "))
            .expect("memory_search must print a file/range line");
        let range = range_line
            .strip_prefix("**File:** /tmp/memory-range.md (lines ")
            .and_then(|line| line.strip_suffix(')'))
            .expect("memory_search file/range format changed");
        let (display_start, display_end) = range
            .split_once('-')
            .map(|(start, end)| {
                (
                    start.parse::<usize>().unwrap(),
                    end.parse::<usize>().unwrap(),
                )
            })
            .expect("memory_search must print an inclusive range");
        assert_eq!((display_start, display_end), (5, 5));

        let get_output = tool_runtime::Tool::run(
            &MemoryGetImpl,
            test_ctx(shared),
            MemoryGetInput {
                path: "/tmp/memory-range.md".into(),
                from: Some(display_start),
                lines: Some(display_end - display_start + 1),
            },
        )
        .await
        .unwrap();
        let ToolOutput::Text(get_text) = get_output else {
            panic!("memory_get must return text");
        };
        assert!(
            get_text.text.contains("search-get-tool-token"),
            "memory_get output: {}",
            get_text.text
        );
    }
}

impl tool_runtime::Tool for MemorySearchImpl {
    type Args = MemorySearchInput;
    type Output = ToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("memory_search").expect("valid tool id")
    }

    fn description(&self, _ctx: &::tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "memory_search",
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::Read,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: MemorySearchInput,
    ) -> Result<ToolOutput, tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        let Some(memory) = resources
            .lock()
            .await
            .get::<Arc<dyn MemoryBackend>>()
            .cloned()
        else {
            return Ok(ToolOutput::Text(
                "Memory is not enabled. Use --experimental-memory to enable.".into(),
            ));
        };
        let max_results = input
            .max_results
            .unwrap_or_else(|| memory.default_search_max_results());
        let min_score = input
            .min_score
            .unwrap_or_else(|| memory.default_search_min_score());
        tracing::info!(target: crate::types::memory_backend::MEMORY_LOG_TARGET, max_results, "MEMORY_SEARCH: invoked");
        let results = memory
            .search(&input.query, max_results, min_score)
            .await
            .map_err(|e| {
                tool_runtime::ToolError::execution(
                    tool_protocol::ToolId::new("memory_search").expect("valid"),
                    format!("memory search failed: {e}"),
                )
            })?;
        tracing::info!(target: crate::types::memory_backend::MEMORY_LOG_TARGET, results = results.len(), "MEMORY_SEARCH: complete");
        if results.is_empty() {
            return Ok(ToolOutput::Text(
                "No memory results found for query.".into(),
            ));
        }
        let mut output = format!("Found {} memory result(s):\n", results.len());
        for (i, r) in results.iter().enumerate() {
            let staleness = format_staleness_note(&r.source, r.created_at);
            // Backends use 0-based, end-exclusive ranges. Display the
            // equivalent inclusive 1-based range so it can be passed to
            // memory_get without an off-by-one correction.
            let (display_start, display_end) = display_line_range(r.start_line, r.end_line);
            output.push_str(&format!(
                "\n### Result {} (score: {:.2}, source: {})\n**File:** {} (lines {}-{})\n{}```\n{}\n```\n",
                i + 1,
                r.score,
                r.source,
                r.path,
                display_start,
                display_end,
                staleness,
                r.snippet,
            ));
        }
        Ok(ToolOutput::Text(output.into()))
    }
}
