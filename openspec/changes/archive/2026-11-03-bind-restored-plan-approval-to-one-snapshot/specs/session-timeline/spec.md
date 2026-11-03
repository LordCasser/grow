## ADDED Requirements

### Requirement: Plan approval restoration retains one exact candidate snapshot
A restored Plan approval attempt SHALL bind its artifact lookup and user request to one exact pending Behavior snapshot. It SHALL reuse that validated content rather than rereading a mutable current candidate. Missing or invalid artifacts MAY normalize only that exact snapshot; a replacement Plan SHALL remain unchanged and SHALL NOT be approved or abandoned by the old attempt.

#### Scenario: Plan is replaced while artifact validation is in flight
- **WHEN** recovery reads candidate A while the current Plan is replaced by candidate B
- **THEN** A's completion neither requests approval for B nor normalizes B, including when B's artifact is unavailable.

#### Scenario: The original candidate artifact is invalid
- **WHEN** the exact current pending snapshot has an absent, empty, corrupt or unreadable artifact
- **THEN** existing durable normalization applies only to that snapshot and no replacement body is inferred.

#### Scenario: Malformed external approval response
- **WHEN** an external client sends a malformed approval response
- **THEN** the interaction attempt closes as Failed, the pending candidate remains unchanged, and an explicit later restore/load can repark it on the same connection with a new attempt identity; no automatic retry or implementation turn starts.

#### Scenario: Candidate changes before request construction completes
- **WHEN** an approval request already has body A but the current candidate changes while awaiting internal state or notification hooks
- **THEN** it never combines body A with B's submission/revision/hash; stale or body/hash-mismatched requests fail before opening a waiter and the new candidate remains unchanged.

#### Scenario: Stale transport state is normalized
- **WHEN** a persisted approval-pending bit has no approval-capable phase
- **THEN** normalization acquires the existing control transaction boundary, compares the captured snapshot, and publishes only its cleared bit after durable confirmation; a replacement snapshot or failed Control write remains unchanged.
