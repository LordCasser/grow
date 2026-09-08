## ADDED Requirements

### Requirement: Automatic session cleanup updates search projection
Successful automatic session cleanup SHALL schedule eviction of deleted session identities from the search projection for the same storage root. Preserved or failed candidates SHALL NOT be reported as successfully deleted. Eviction SHALL use the existing asynchronous missing-summary handling without creating an absent index.

#### Scenario: Expired indexed session is removed
- **WHEN** automatic cleanup successfully removes an expired session
- **THEN** its identity is queued for search update and the missing-summary path removes its index document

#### Scenario: Candidate is retained
- **WHEN** a candidate is fresh, explicitly skipped, writer-owned elsewhere or fails cleanup
- **THEN** cleanup does not report it as successfully deleted for eviction

#### Scenario: No search index exists
- **WHEN** a cleanup deletion is processed without an existing index
- **THEN** eviction does not create an index solely to remove that document
