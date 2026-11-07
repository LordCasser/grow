## ADDED Requirements

### Requirement: Durable queued replacements retain their recovery slot and runtime identity

A saved replacement of a pending FIFO row SHALL inherit the replaced row's canonical FIFO position across cold recovery. The queue row and runtime prompt identity SHALL remain the original row identity while the edited submission retains its own immutable admission identity and current payload/version. Recovery SHALL consume the Timeline-owned replacement order, without replaying superseded inputs or their Hooks.

#### Scenario: Earlier queued row is edited after later rows arrive

- **WHEN** pending rows A, B and C are admitted in that order and A is durably replaced one or more times after B and C are admitted
- **THEN** recovery restores the latest A replacement ahead of B and C, with A's original row/runtime identity and the latest content, version and input identity.

#### Scenario: Multiple rows are independently edited

- **WHEN** A is edited twice and B is edited once before any row is consumed
- **THEN** every recovery cut after a successful replacement admission restores A, B and C once in their original order, and each superseded input remains ineligible for execution.

#### Scenario: Replacement admission is unresolved or rejected

- **WHEN** an edit has no successful durable replacement admission
- **THEN** recovery uses the last successfully admitted row content and identity, and does not expose an unconfirmed replacement as accepted work.
