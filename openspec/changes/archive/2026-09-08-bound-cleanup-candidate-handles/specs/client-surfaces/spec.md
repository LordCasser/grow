## ADDED Requirements

### Requirement: TTL cleanup bounds candidate handle ownership
TTL cleanup SHALL discover summaries without retaining a directory handle per result and SHALL process candidates with operation-scoped handle and maintenance lease ownership. Discovery SHALL complete before any deletion. A discovery snapshot SHALL NOT authorize deletion without current same-entity eligibility checks.

#### Scenario: History exceeds descriptor capacity
- **WHEN** the number of sessions exceeds the process descriptor limit but sequential cleanup has sufficient capacity
- **THEN** cleanup can process all eligible candidates without retaining one handle per discovered session.

#### Scenario: Candidate is preserved or rejected
- **WHEN** current candidate state is fresh, hidden, writer-owned, invalid or cannot be opened
- **THEN** cleanup preserves it, emits no deletion callback and releases operation-owned resources.

#### Scenario: Discovery rejects duplicate identity
- **WHEN** the complete discovery scan detects duplicate canonical session identities
- **THEN** cleanup fails before deleting any candidate.
