## ADDED Requirements

### Requirement: Pre-session command discovery has a bounded execution boundary

Pre-session non-chat `grow/commands/list` SHALL perform folder-trust resolution and plugin, skill, and workflow discovery in the shared single-permit blocking worker under a five-second deadline that includes waiting for capacity. A timeout SHALL NOT claim to interrupt an already-running filesystem operation. Chat catalog requests and live-session command requests SHALL retain their existing paths without this pre-session discovery.

#### Scenario: Pre-session discovery remains blocked after timeout
- **WHEN** pre-session command discovery remains blocked beyond its deadline and another pre-session listing arrives
- **THEN** the first request returns an RPC error, its worker retains the shared discovery permit until it exits, and the later request cannot start an additional scan beyond the concurrency bound

#### Scenario: Pre-session discovery fails or times out
- **WHEN** the discovery worker fails or the deadline expires
- **THEN** `grow/commands/list` returns an RPC error rather than a successful empty command catalog

#### Scenario: Discovery completes with no commands
- **WHEN** all pre-session scans finish successfully and find no commands
- **THEN** the request returns the existing successful empty catalog shape

#### Scenario: Chat and live-session command paths
- **WHEN** `kind="chat"` or `sessionId` selects its existing early-return branch
- **THEN** the request bypasses pre-session plugin, skill, and workflow discovery and preserves its existing response behavior
