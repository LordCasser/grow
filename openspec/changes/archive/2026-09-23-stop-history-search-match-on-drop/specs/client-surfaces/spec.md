## MODIFIED Requirements

### Requirement: History search submission does not wait for worker capacity
History search SHALL retain a coalesced latest pending request and submit updates without waiting for worker queue capacity. Closing its daemon SHALL not wait for notification capacity or ongoing matching. Drop SHALL signal cooperative cancellation of the active request, and the worker SHALL observe that signal between per-item scoring and highlight-index operations, abandoning the request without publishing its partial results. A single scoring or index operation, and the current result sort, are indivisible and may finish after Drop returns.

#### Scenario: Worker is busy while inputs accumulate
- **WHEN** item refreshes and queries arrive before the worker can process pending work
- **THEN** submissions return without waiting for channel capacity and the next request retains the latest item refresh with its latest following query

#### Scenario: Closing with pending work
- **WHEN** the history search daemon is dropped while work is pending
- **THEN** stop takes precedence over pending work without blocking the UI on channel capacity

#### Scenario: Closing during active matching
- **WHEN** the history search daemon is dropped while a score, highlight-index operation, or result sort is in progress
- **THEN** Drop returns without waiting for that operation, and the worker abandons the request after the current operation completes without publishing partial results
