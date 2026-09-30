## MODIFIED Requirements

### Requirement: Paste collection preserves input-batch boundaries
A detected unbracketed multiline paste SHALL remain one pending insertion across bounded input collection passes. Reaching an event budget SHALL yield without committing the paste prefix. The existing idle boundary SHALL flush the complete insertion even without a subsequent input event. A completed bracketed `Event::Paste` SHALL retain its own boundary and SHALL NOT absorb or discard later events merely because they were collected in the same input batch.

#### Scenario: Large paste exceeds collection budget
- **WHEN** one continuous multiline paste exceeds the input extension event budget and ends with an unterminated line
- **THEN** its complete text is inserted once as one paste, without separately routed tail keystrokes or multiple chips.

#### Scenario: Exactly exhausted batch becomes idle
- **WHEN** collection reaches its event budget and no further input arrives
- **THEN** the idle deadline flushes the pending paste without requiring another key.

#### Scenario: Event-loop fairness
- **WHEN** a large paste is still arriving
- **THEN** each collection pass retains its bounded event budget and returns to the main loop.

#### Scenario: Ordinary input remains ordinary
- **WHEN** input is a normal key, a completed bracketed paste, or a non-paste event storm
- **THEN** existing key routing and bounded handling remain unchanged.

#### Scenario: Control key interrupts pending collection
- **WHEN** a control key arrives during detection or at the next pending-paste batch
- **THEN** collection returns without waiting for the remaining paste tail and retains the control key for normal routing.

#### Scenario: Bracketed paste followed by ordinary input
- **WHEN** one input batch contains a completed bracketed paste followed by Enter, text, navigation or a control key
- **THEN** Pager routes the paste and each subsequent key separately in arrival order without adding the keys to paste text or dropping them.

#### Scenario: Bracketed paste followed by a multiline-shaped key run
- **WHEN** one input batch contains a completed bracketed paste followed by character, Enter and another character keys
- **THEN** Pager retains three ordinary key events after the Paste rather than synthesizing another Paste.

#### Scenario: Two completed bracketed pastes
- **WHEN** one input batch contains two completed `Event::Paste` values
- **THEN** Pager preserves two paste events in their original order rather than combining their content.

### Requirement: File search results belong to the current query
Pager file search SHALL expose results only when they belong to the currently active query request. Submitting a new query SHALL immediately make prior results unavailable for display and selection.

#### Scenario: Previous query completes after dismiss and reopen
- **WHEN** a user dismisses file search, opens it again with another query, and the daemon publishes a snapshot from the previous query before processing the new request
- **THEN** the previous snapshot is not displayed or selectable, and results from the current request can be applied

#### Scenario: Previous query remains visible during an edit
- **WHEN** the active `@` query changes while an earlier daemon snapshot is still available
- **THEN** the old snapshot is cleared immediately and cannot be restored by a late poll

#### Scenario: Escape while a new query has no visible results
- **WHEN** a user presses Escape while an active `@` completion context has empty or pending results after a directory drill
- **THEN** Pager dismisses that context and its drill anchor, and a late result cannot reopen the dismissed query

证据：`crates/codegen/pager/src/views/prompt_widget/mod.rs` — `handle_key_inner`；`crates/codegen/pager/src/views/file_search/state.rs` — `start_query`, `poll`, `apply_results`；`crates/codegen/workspace/src/file_system/fuzzy.rs` — `FuzzyFileMatcherDaemon::set_query` 与结果快照的 `query_id`。
