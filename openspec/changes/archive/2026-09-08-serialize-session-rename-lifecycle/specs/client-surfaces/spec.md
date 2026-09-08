## ADDED Requirements

### Requirement: Session rename participates in lifecycle serialization
Within one agent, session rename SHALL share the per-session lifecycle gate with load and delete from authoritative lookup through title commit. It SHALL preserve the distinction between resident actor mutation and dormant storage mutation without waiting on a loader queued behind its own guard.

#### Scenario: Load or delete competes with rename
- **WHEN** rename owns the session lifecycle guard
- **THEN** same-agent load/delete cannot cross its lookup and title commit boundary

#### Scenario: Loader announced behind rename
- **WHEN** a load is announced but awaits the lifecycle guard held by rename
- **THEN** rename resolves current residency without awaiting that blocked load and can complete
