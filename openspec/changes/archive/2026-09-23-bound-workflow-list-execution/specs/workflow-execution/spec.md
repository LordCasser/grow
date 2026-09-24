## ADDED Requirements

### Requirement: Workflow listing has a bounded execution boundary
Asynchronous workflow listing SHALL isolate filesystem scans from runtime worker threads, bound concurrent scans to one, and apply a five-second deadline that includes waiting for scan capacity and scan completion. A timeout SHALL NOT claim to have interrupted an already-running filesystem operation.

#### Scenario: Scan stays blocked after request timeout
- **WHEN** a workflow scan remains blocked past the listing deadline and another listing request arrives
- **THEN** the first request returns an error, its worker retains the scan slot until it exits, and the later request cannot start an additional scan beyond the concurrency bound

#### Scenario: Listing scan fails or times out
- **WHEN** the scan worker fails or the listing deadline expires
- **THEN** `grow/workflows/list` returns an error rather than a successful empty workflow list

#### Scenario: Listing completes without results
- **WHEN** the scan completes successfully and discovers no workflows
- **THEN** the request returns a successful empty workflow list
