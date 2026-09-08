## ADDED Requirements

### Requirement: Drain the complete session writer incarnation
Session lifecycle drain SHALL include termination of the persistence task and release of its storage writer ownership as well as termination of the dedicated actor thread. A flush acknowledgement alone SHALL NOT establish writer termination.

#### Scenario: Actor exits before persistence owner
- **WHEN** the actor thread has terminated but its persistence task still owns the writer lease
- **THEN** close/replacement drain remains pending and does not admit a replacement writer.

#### Scenario: Complete writer exit
- **WHEN** both the actor thread and persistence owner have terminated
- **THEN** lifecycle drain may finish and a subsequent cold load can acquire the writer lease.

#### Scenario: Drain timeout
- **WHEN** the persistence owner does not terminate before the existing drain deadline
- **THEN** drain reports the outstanding shutdown and preserves the old incarnation's ownership record for a later retry.

#### Scenario: Retained handle after actor exit
- **WHEN** an external resource retains a persistence sender after the actor thread exits
- **THEN** lifecycle drain explicitly closes persistence admission, drains already accepted messages, and waits for owner exit without requiring that handle to be dropped.
