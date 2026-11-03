## ADDED Requirements

### Requirement: Runtime-disabled memory does not admit flush work

A memory flush SHALL observe whether memory is enabled for the current session before admitting work. Pre-compaction admission SHALL check the current state before freezing input and after asynchronous snapshot capture. A flush SHALL recheck the state before provider work and before persisting an accepted response. If memory is disabled, the flush SHALL not start a provider request or write memory data.

#### Scenario: Memory is disabled before compaction

- **WHEN** the user turns memory off and a later compaction reaches the pre-compaction flush threshold
- **THEN** pre-compaction flush does not capture or dispatch flush work and no memory-flush provider request is sent

#### Scenario: Memory is disabled while snapshot capture waits

- **WHEN** a memory flush is waiting for primary image projection and the user turns memory off before capture completes
- **THEN** the frozen read-only snapshot is not sent to a provider and no memory file is written

#### Scenario: Memory is disabled while the provider responds

- **WHEN** memory is turned off after a flush provider request begins but before its accepted response is persisted
- **THEN** the response is not written to memory
