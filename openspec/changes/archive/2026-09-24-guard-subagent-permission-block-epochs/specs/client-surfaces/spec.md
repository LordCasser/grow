## ADDED Requirements

### Requirement: Subagent permission groups preserve their source epoch

Pager SHALL append or merge subagent permission events into a permission group only when the source permission epoch matches that group's epoch. Membership mutation SHALL be mediated by the scrollback state path that owns and compares those epochs; an event from an older or newer epoch SHALL NOT alter the group.

#### Scenario: Append within the active epoch
- **WHEN** a permission event is appended while its source epoch matches the active group
- **THEN** the group retains the event and invalidates its rendered projection.

#### Scenario: Append across an epoch boundary
- **WHEN** a permission event from a different epoch is offered to an existing group
- **THEN** the group remains unchanged and the state handles the event in the appropriate current group.

#### Scenario: Reconnect merge crosses a terminal boundary
- **WHEN** reconnect tail groups are merged and a source group's epoch differs from the destination group's epoch
- **THEN** their members remain separate and no event crosses the terminal boundary.
