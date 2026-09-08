## ADDED Requirements

### Requirement: Retry contended session search projections
A live search worker SHALL retain and retry a session projection update or eviction that fails specifically because SQLite remains Busy or Locked after its own wait. Retry SHALL use a nonzero delay and the existing per-root coalescing queue. Non-contention failures SHALL NOT be treated as lock contention.

#### Scenario: Lock clears after failed eviction
- **WHEN** an indexed session has been deleted and its queued eviction encounters SQLite lock contention
- **THEN** the worker retains the key and retries after a delay, allowing eviction after lock release without another notification.

#### Scenario: Projection succeeds
- **WHEN** a pending update or eviction succeeds
- **THEN** that pending work is removed rather than repeated indefinitely.

#### Scenario: Permanent error
- **WHEN** an update fails with invalid session data or another non-contention error
- **THEN** the worker reports the failure without entering the contention retry loop.
