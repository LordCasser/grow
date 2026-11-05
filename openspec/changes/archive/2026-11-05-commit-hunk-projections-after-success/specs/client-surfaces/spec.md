## ADDED Requirements

### Requirement: Hunk action projections commit only after the action succeeds

Hunk action statistics, turn membership removal and affected identifiers SHALL reflect only successfully applied actions. A failing single action or file batch SHALL retain that file's pending hunks and action projections. Multi-file batches SHALL retain already committed successful files and SHALL NOT project a failing file as successful or simulate an all-project rollback.

#### Scenario: Single reject write fails
- **WHEN** the public hunk action encounters a write or delete failure
- **THEN** it returns the error with the hunk, its turn membership and action statistics still pending, without a successful removal projection.

#### Scenario: A file batch fails
- **WHEN** a file-scoped, turn-scoped or all-files batch cannot apply one file
- **THEN** that file's hunks are not counted or removed from their turn, while earlier successfully committed files remain represented by their actual result.

#### Scenario: Normal accept or reject succeeds
- **WHEN** a single hunk or file batch is successfully accepted or rejected
- **THEN** its content, statistics, turn membership and affected identifiers agree and the action is counted once.
