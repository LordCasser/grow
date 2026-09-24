## ADDED Requirements

### Requirement: Scrollback search retains bounded pending work
Scrollback search SHALL retain at most one pending merged request while a worker scans and SHALL submit query updates without waiting for worker capacity. Its result identity SHALL continue to prevent an older scan from replacing the visible query result.

#### Scenario: Query burst during a scan
- **WHEN** a new corpus and several newer queries arrive before the worker can take pending work
- **THEN** the pending request retains the newest corpus and query, submission does not wait for scanning, and only the latest matching result is applied to the current search view.

#### Scenario: Search owner closes with pending work
- **WHEN** search closes while a scan or pending request exists
- **THEN** stop takes precedence over the pending request without waiting for worker notification capacity; the worker does not apply that pending request after closing.
