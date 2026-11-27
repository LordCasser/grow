## ADDED Requirements

### Requirement: Ordinary persistence acknowledgements preserve the resident writer

An ordinary Timeline append without a caller deadline SHALL remain owned by the original writer while its persistence acknowledgement is outstanding. The writer SHALL serialize later facts behind it, accept a durable success exactly once, and admit subsequent facts on the same writer. A confirmed permanent error SHALL still close the affected writer. Retry dispatch following reported failures or a lost acknowledgement SHALL remain bounded.

#### Scenario: The Shell persistence queue stalls during a delegated resume
- **WHEN** an ordinary foreground write waits more than 30 seconds for a live acknowledgement while the persistence worker is delayed
- **THEN** the original writer retains the pending identity, sends no duplicate or subsequent record during the delay, then accepts a successful acknowledgement and continues at the next sequence without reloading the session.

#### Scenario: A bounded operation expires while waiting in the mailbox
- **WHEN** the operation deadline has already elapsed before its first append is submitted
- **THEN** the caller receives a deadline error, no storage request is submitted, and the same writer can use the next sequence for another operation.

#### Scenario: A bounded barrier has an unresolved write at its deadline
- **WHEN** an explicit barrier has submitted its write and the acknowledgement is still outstanding when its caller deadline expires
- **THEN** the caller receives a deadline error and the original pending identity remains available for reconciliation without admitting a competing write.
