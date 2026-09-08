## ADDED Requirements

### Requirement: Slash MRU background writes coalesce pending snapshots
Slash MRU persistence SHALL retain at most one pending complete snapshot in addition to a snapshot being written. New submissions SHALL replace the pending snapshot, and queueing SHALL not wait for disk IO or notification capacity. An unavailable writer SHALL return failure to the controller for its existing dirty-state retry instead of writing synchronously.

#### Scenario: Slow writer receives multiple updates
- **WHEN** multiple complete snapshots arrive before the worker takes pending work or while it writes a previous snapshot
- **THEN** only the latest pending snapshot remains and the worker can publish it after the current write finishes

#### Scenario: Writer unavailable
- **WHEN** the background writer cannot start or its notification receiver is disconnected
- **THEN** persistence reports failure without writing the file on the submitting thread and the controller retains dirty state for a future command
