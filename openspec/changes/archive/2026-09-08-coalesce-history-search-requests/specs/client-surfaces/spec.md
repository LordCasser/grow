## ADDED Requirements

### Requirement: History search submission does not wait for worker capacity
History search SHALL retain a coalesced latest pending request and submit updates without waiting for worker queue capacity. Closing its daemon SHALL not wait for notification capacity or ongoing matching.

#### Scenario: Worker is busy while inputs accumulate
- **WHEN** item refreshes and queries arrive before the worker can process pending work
- **THEN** submissions return without waiting for channel capacity and the next request retains the latest item refresh with its latest following query

#### Scenario: Closing with pending work
- **WHEN** the history search daemon is dropped while work is pending or being matched
- **THEN** stop takes precedence over pending work without blocking the UI on channel capacity
