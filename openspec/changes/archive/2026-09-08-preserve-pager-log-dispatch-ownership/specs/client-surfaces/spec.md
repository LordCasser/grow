## ADDED Requirements

### Requirement: Pager log dispatch preserves initialized ownership
Pager log forwarding SHALL use its successfully initialized transport and runtime independently of the calling thread. Flush before initialization SHALL preserve buffered entries, and repeated initialization SHALL not create additional periodic flush tasks.

#### Scenario: Plain thread dispatch
- **WHEN** a thread without an entered Tokio runtime emits a flushable log batch after initialization
- **THEN** forwarding uses the initialization runtime instead of discarding the batch because the producer lacks a runtime

#### Scenario: Flush before initialization
- **WHEN** buffered entries exist and a flush is requested before sender initialization
- **THEN** the entries remain available to the eventual initialized forwarder

#### Scenario: Repeated initialization
- **WHEN** initialization is called after a forwarder is already installed
- **THEN** the existing owner is retained without starting another periodic consumer
