## MODIFIED Requirements

### Requirement: History search submission does not wait for worker capacity
History search SHALL retain a coalesced latest pending request and submit updates without waiting for worker queue capacity. Closing its daemon SHALL not wait for notification capacity or ongoing matching. Drop SHALL signal cooperative cancellation of the active request, and the worker SHALL observe that signal between per-item history conversion, scoring, and highlight-index operations, abandoning the request without publishing partial results. A query request SHALL retain at most 100 scored candidates while scanning the corpus, then order those candidates by descending score; the best match SHALL remain last in the rendered result list. A single conversion, scoring, or index operation is indivisible and may finish after Drop returns.

#### Scenario: Worker is busy while inputs accumulate
- **WHEN** item refreshes and queries arrive before the worker can process pending work
- **THEN** submissions return without waiting for channel capacity and the next request retains the latest item refresh with its latest following query

#### Scenario: Closing with pending work
- **WHEN** the history search daemon is dropped while work is pending
- **THEN** stop takes precedence over pending work without blocking the UI on channel capacity

#### Scenario: Closing during active matching
- **WHEN** the history search daemon is dropped while a score, highlight-index operation, or result sort is in progress
- **THEN** Drop returns without waiting for that operation, and the worker abandons the request after the current operation completes without publishing partial results

#### Scenario: Closing during item preprocessing
- **WHEN** the history search daemon is dropped while history items are being converted for matching
- **THEN** Drop returns without waiting for the current conversion, the worker abandons preprocessing after that operation, and no partial item set is published

#### Scenario: Query matches exceed the visible result limit
- **WHEN** more than 100 history items match a query
- **THEN** matching retains only the 100 highest-scoring candidates, orders them from lower to higher score for rendering, and keeps the best match last
