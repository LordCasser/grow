## ADDED Requirements

### Requirement: Search quarantine failure stops recreation
Search-cache recovery SHALL stop before recreation when a required quarantine rename fails. Missing source files SHALL be distinguished from unsuccessful isolation. Recovery diagnostics SHALL distinguish an isolated cache from a successfully recreated cache.

#### Scenario: Isolation fails
- **WHEN** a main database or sidecar cannot be moved into quarantine
- **THEN** that recovery attempt reports the isolation failure and does not call recreate at the live path.

#### Scenario: Recreation fails after isolation
- **WHEN** quarantine changes the cache namespace but recreation fails
- **THEN** recovery preserves quarantine artifacts, exposes the namespace change to existing invalidation, and does not report successful empty-cache recreation.

#### Scenario: Isolation completes
- **WHEN** required files are successfully isolated or confirmed absent
- **THEN** recovery may attempt recreation through the existing path.

#### Scenario: Caller observes failed recovery
- **WHEN** isolation or recreation fails
- **THEN** the index caller returns its triggering error instead of unconditionally reopening the live path after failed healing.
