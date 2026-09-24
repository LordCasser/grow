## ADDED Requirements

### Requirement: File search results belong to the current query
Pager file search SHALL expose results only when they belong to the currently active query request. Submitting a new query SHALL immediately make prior results unavailable for display and selection.

#### Scenario: Previous query completes after dismiss and reopen
- **WHEN** a user dismisses file search, opens it again with another query, and the daemon publishes a snapshot from the previous query before processing the new request
- **THEN** the previous snapshot is not displayed or selectable, and results from the current request can be applied

#### Scenario: Previous query remains visible during an edit
- **WHEN** the active `@` query changes while an earlier daemon snapshot is still available
- **THEN** the old snapshot is cleared immediately and cannot be restored by a late poll

证据：`crates/codegen/pager/src/views/file_search/state.rs` — `start_query`, `poll`, `apply_results`；`crates/codegen/workspace/src/file_system/fuzzy.rs` — `FuzzyFileMatcherDaemon::set_query` 与结果快照的 `query_id`。
