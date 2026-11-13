## ADDED Requirements

### Requirement: Suspended tree execution waits for load finalization

A cold executable resume SHALL prepare and reconcile the suspended tree while execution admission is closed. Shell SHALL complete load reconciliation and commit the resume usage boundary before releasing the original tree for execution. The load response SHALL retain any preserved foreground identity. Pager SHALL enable ordinary input at its existing successful load boundary.

#### Scenario: An active Goal is restored from a suspended session

- **WHEN** history replay has reached its end but load reconciliation or resume accounting is still pending
- **THEN** the suspended tree stays gated and cannot start a new Goal turn or continue a preserved turn. Successful finalization releases the same owners through the existing tree release path.

#### Scenario: A suspended turn is preserved by the checkpoint

- **WHEN** a successful cold load restores a checkpointed foreground turn
- **THEN** the load response identifies the preserved foreground and execution continues under that owner after the load's recovery and accounting work.
