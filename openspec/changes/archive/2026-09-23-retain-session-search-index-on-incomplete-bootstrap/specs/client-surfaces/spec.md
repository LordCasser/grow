## ADDED Requirements

### Requirement: Incomplete search bootstrap preserves index state
Session search bootstrap SHALL treat a missing or invalid summary inside an opened session directory, or a failure in any required Timeline read or index write, as an incomplete attempt. An incomplete attempt SHALL NOT prune existing index rows or publish a completed-bootstrap marker. After the source is repaired, a bootstrap recheck SHALL be able to rebuild the affected index row. Evidence: `crates/codegen/shell/src/session/storage/search.rs` (`reindex_all`, `SearchIndexJob::RecheckBootstrap`) and `crates/codegen/shell/src/session/storage/jsonl/mod.rs` (session summary enumeration).

#### Scenario: Missing or invalid summary
- **WHEN** bootstrap opens a session directory whose summary is missing, cannot be decoded, or cannot be validated
- **THEN** it fails the attempt before orphan pruning, preserves existing indexed rows, and leaves no completed-bootstrap marker

#### Scenario: Required Timeline or index operation fails
- **WHEN** a session's required Timeline read or index write fails during bootstrap
- **THEN** the attempt remains incomplete, existing index rows are not pruned, and no completed-bootstrap marker is published

#### Scenario: Recheck after source repair
- **WHEN** a failed bootstrap is followed by repair of the invalid summary or Timeline and a bootstrap recheck
- **THEN** the recheck indexes the repaired session and publishes the completed-bootstrap marker
