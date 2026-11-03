## ADDED Requirements

### Requirement: Explicit restore retry reloads an unprepared temporary owner
A failed load SHALL physically retire and drain only the actor newly created by that load when its original runtime remains Suspended and has not acquired RestorePrepared. The next explicit load SHALL rebuild that actor from authoritative history and available dependencies. Retirement SHALL preserve logical work and its original checkpoint/operation, and an unconfirmed retirement SHALL retain the physical owner.

#### Scenario: A failed cold load is retried
- **WHEN** a cold load fails before preparation and its checkpoint source becomes available before an explicit retry
- **THEN** the temporary actor is retired without finalizing the logical task, and the retry rechecks current authoritative facts rather than reusing the failed snapshot.

#### Scenario: Restore ownership was already prepared or released
- **WHEN** an existing viewer or a Restoring/Running owner is involved in a failed load
- **THEN** that owner is retained and the same operation/instance is reconciled; no replacement execution is created.
