## ADDED Requirements

### Requirement: Human dependency changes wake idle Goal arbitration

A resident Session actor SHALL observe changes to its existing pending human-interaction registry and reconsider idle work through the existing idle arbiter. When a whole-work Goal plan wait becomes ready because the matching human interaction has answered, timed out or been cancelled, the actor SHALL NOT require another user prompt or unrelated notification to schedule eligible continuation. Existing execution fences, stopped Goal status, pending controls, compaction, user FIFO and notification precedence SHALL remain authoritative. The in-turn dependency waiter SHALL retain its current behavior.

#### Scenario: A pending interaction resolves while the actor is idle

- **WHEN** an Active Goal is idle under a durable whole-work wait on a valid pending human interaction and that interaction is removed
- **THEN** the actor rechecks readiness and admits eligible continuation without an additional external wakeup.

#### Scenario: A stopped Goal receives the same registry change

- **WHEN** the matching registry entry changes after the Goal has stopped
- **THEN** the same idle arbitration does not restart that Goal.

#### Scenario: User FIFO and control precedence survive the wake

- **WHEN** a registry change occurs while an existing control or user FIFO input has priority
- **THEN** that work retains its existing priority over automatic Goal continuation.
