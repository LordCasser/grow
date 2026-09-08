## ADDED Requirements

### Requirement: Recap admission reflects command enqueue
An enabled recap request SHALL report acceptance only after its session command is queued. A closed command channel SHALL produce an ACP error for manual and automatic requests instead of an accepted response.

#### Scenario: Closed session actor channel
- **WHEN** the recap session exists but its command receiver is closed
- **THEN** recap admission returns an ACP error rather than claiming a recap was queued

#### Scenario: Live session actor channel
- **WHEN** the recap command is successfully enqueued
- **THEN** admission returns success and preserves the requested auto flag
