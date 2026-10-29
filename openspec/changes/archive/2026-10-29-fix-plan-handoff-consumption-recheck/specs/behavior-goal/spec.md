## ADDED Requirements

### Requirement: Live Plan handoff rechecks preserve exact consumption ownership
Before the next live Plan Step the Session SHALL consume the exact approved handoff or prove that its immutable receipt was already consumed by the current Turn. Missing receipts, dismissed receipts, actor unavailability and consumption by another Turn SHALL NOT authorize resampling. Rechecking SHALL NOT append duplicate consumption or re-inject model input.

#### Scenario: Active drain consumed the current handoff
- **WHEN** the exact artifact hash, revision, kind and submission receipt was consumed by the current Turn before the next-Step check
- **THEN** the check succeeds without a new Timeline fact or another model message.

#### Scenario: Receipt belongs to another lifecycle
- **WHEN** an exact receipt is dismissed, absent, consumed by another Turn, or its owner cannot answer
- **THEN** the check fails and no successor provider request is admitted.
