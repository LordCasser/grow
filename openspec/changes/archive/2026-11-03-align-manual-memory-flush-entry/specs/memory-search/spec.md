## ADDED Requirements

### Requirement: Manual memory flush admission is independent of foreground turns

Explicit `/flush` and `grow/memory/flush` SHALL admit an independent memory Sideband while a regular foreground turn is active, provided runtime memory is enabled and session activity admission remains open. Both SHALL reuse the same flush execution lock, frozen context and completion notifications. `/flush` SHALL NOT enter the model input queue or block the command loop while waiting for its provider. Disabled memory SHALL reject admission without provider work. Failure or lock contention SHALL NOT be treated as an automatic successful checkpoint.

The synchronous ACP extension SHALL report failure when execution did not produce a successful checkpoint. A successful checkpoint MAY mean written memory or confirmed nothing to store; the existing completion notification's result and path SHALL distinguish them.

#### Scenario: Flush while foreground is busy
- **WHEN** an enabled session receives an explicit manual flush during a regular turn
- **THEN** either entry admits Sideband work independently and the foreground continues; the original command is not added to model conversation.

#### Scenario: Disabled or closing session
- **WHEN** runtime memory is disabled or activity admission has closed
- **THEN** the manual command reports refusal and emits no provider work.

#### Scenario: Synchronous flush fails or loses execution admission
- **WHEN** the ACP flush is skipped for lock contention, rejected or fails
- **THEN** its response is an error rather than a successful empty receipt; automatic history remains eligible and completion feedback retains the outcome.
