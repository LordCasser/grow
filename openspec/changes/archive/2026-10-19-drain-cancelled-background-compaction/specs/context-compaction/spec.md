## ADDED Requirements

### Requirement: Cancellation preserves and drains the compaction generation owner
When ordinary authority invalidation cancels a background compaction generation, the Session SHALL signal cancellation without joining an unfinished worker from inside the root command handler, and SHALL retain the worker in its existing owner slot. An explicit suspension or teardown barrier SHALL keep the root accounting inbox able to process the worker's Goal settlement receipt while it joins the worker; it SHALL publish the cancellation terminal only after that drain. A runtime freeze SHALL use the existing shared deadline for this drain and SHALL fail without claiming a settled checkpoint when the worker has not joined by that deadline. Cancelled or late generation output SHALL NOT be committed to the visible Surface.

#### Scenario: Goal usage invalidation and Sideband settlement share the root inbox
- **WHEN** a promoted summary is still running and `RecordGoalUsageIncomplete` is processed by the root accounting inbox
- **THEN** invalidation signals cancellation and returns from the current inbox command, allowing the same inbox to process the worker's `SettleGoalUsageAttempt` before a later explicit drain records cancellation.

#### Scenario: Promoted generation is cancelled after incomplete Goal usage
- **WHEN** a promoted summary is still running as the Goal usage window becomes incomplete
- **THEN** the same worker is cancelled and joined, its Sideband attempt reaches a terminal settlement, the Goal settlement is not detached, and the generated summary is not committed.

#### Scenario: Suspension deadline expires during generation drain
- **WHEN** runtime suspension cancels a background summary but its owner has not joined by the shared deadline
- **THEN** suspension returns an unconfirmed result and retains the worker owner for retry; it does not publish a completed frozen checkpoint.

#### Scenario: Teardown drains with the accounting inbox active
- **WHEN** graceful or fatal teardown has an unfinished cancelled compaction worker
- **THEN** its existing drain loop continues dispatching Goal usage and settlement commands until the compaction worker and non-Workflow children finish, or teardown fails without publishing a final persistence frontier.

#### Scenario: Manual compaction waits for the existing generation owner
- **WHEN** a manual compaction starts while an asynchronous summary still owns the compaction lease
- **THEN** manual compaction signals cancellation, leaves the root Goal inbox available while it joins that worker, and acquires the lease only after the original worker has settled.
