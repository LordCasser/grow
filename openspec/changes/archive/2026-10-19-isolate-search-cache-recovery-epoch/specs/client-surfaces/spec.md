## ADDED Requirements

### Requirement: Search-cache recovery epochs are scoped to index identity
Search-cache recovery SHALL associate quarantine and recreation epoch changes with the effective search-index path. Query and bootstrap work SHALL observe changes only for the index they use. Healing one index SHALL NOT trigger rebootstrap or invalidate the completed-bootstrap marker for a distinct root. Replacing an index at the same effective path SHALL invalidate work already in progress for that index.

#### Scenario: One root is healed while another root bootstraps
- **WHEN** the cache for root A is quarantined or recreated while root B has an active query or bootstrap
- **THEN** root A observes its epoch change and root B's epoch and completion decision remain unchanged.

#### Scenario: Same-root replacement during bootstrap
- **WHEN** an index at root A's effective path is replaced while root A is bootstrapping
- **THEN** that bootstrap withholds or clears its stale completion marker and retries through the existing bootstrap path.

#### Scenario: Two roots heal concurrently
- **WHEN** separate roots undergo cache recovery concurrently
- **THEN** each recovery advances only its own index identity, and both roots can complete using their own bootstrap lease and marker.
