## ADDED Requirements

### Requirement: Fuzzy file-search submissions and teardown do not wait for worker capacity
Fuzzy file search SHALL submit the latest restart and query without waiting for the worker's notification channel or an ongoing filesystem walk. Closing its daemon SHALL not join the worker on the caller's thread and SHALL prevent pending requests from running after stop is observed.

#### Scenario: Slow walk with rapid input
- **WHEN** a worker is occupied while multiple restart and query updates arrive
- **THEN** submission remains nonblocking, only the newest pending restart and query remain, and the published result retains that query's request identity.

#### Scenario: Coalesced query and workspace status polling
- **WHEN** workspace fuzzy-search status polling starts after several query changes were coalesced by the worker
- **THEN** it waits for the latest query identity, does not publish an older query's result, and can publish the latest result without waiting for skipped worker generations.

#### Scenario: Search closes during worker activity
- **WHEN** the daemon is dropped with pending work or an active walk
- **THEN** stop supersedes pending work, the current walk is asked to cancel, and the caller returns without waiting for channel capacity or worker join.
