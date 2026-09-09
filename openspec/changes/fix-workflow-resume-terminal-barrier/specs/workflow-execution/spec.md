## ADDED Requirements

### Requirement: Resume follows durable execution settlement
Workflow resume SHALL await the previous execution terminal acknowledgment before admitting the next execution epoch. A resumable in-memory tracker projection alone SHALL NOT authorize resume while its terminal watcher is still settling.

#### Scenario: Resume during terminal persistence
- **WHEN** a run is projected paused or failed but its terminal persistence is still pending
- **THEN** resume waits and only appends Resumed after the prior Ended is acknowledged.

#### Scenario: Terminal settlement fails
- **WHEN** the pending terminal watcher reports failure or loses its acknowledgment channel
- **THEN** resume returns that failure and does not open a new execution epoch.
