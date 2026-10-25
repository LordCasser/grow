## ADDED Requirements

### Requirement: Busy Sampling control terminals release matching model projections

Pager SHALL update the owning session's visible model and reasoning effort from an authoritative `ModelChanged` that was parked behind a local Sampling intent once that intent's matching terminal control state resolves. The update SHALL remain independent of other pending control domains and SHALL preserve the Shell's step boundary and terminal notice semantics.

#### Scenario: Effort changes while the session is busy

- **WHEN** a running session receives `ModelChanged` for a new effort before the matching effort-only Sampling control reaches its applied terminal state
- **THEN** Pager displays the new effort after that terminal state without requiring another user action or turn completion

#### Scenario: Another control is still pending

- **WHEN** the matching Sampling terminal resolves while an Agent or Behavior control remains pending
- **THEN** Pager applies the parked Sampling projection without waiting for that other control

#### Scenario: Unmatched terminal cannot release the pending intent

- **WHEN** a terminal receipt does not match the currently pending Sampling intent
- **THEN** Pager keeps that intent and its parked authoritative model projection pending
