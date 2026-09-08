## ADDED Requirements

### Requirement: Announcement preference writes follow local change order
A running pager AppView SHALL have at most one announcement preference write in flight. Changes during that write SHALL be coalesced into the latest hidden-ID state, submitted after the earlier write completes. Completion failure SHALL not prevent an already pending newer state from being submitted.

#### Scenario: Hide and show while a write is pending
- **WHEN** hidden IDs change repeatedly before the in-flight write completes
- **THEN** no overlapping write starts and completion submits only the latest state

#### Scenario: Earlier write fails
- **WHEN** an in-flight write fails with newer changes pending
- **THEN** its error remains reported and the newer state is submitted next

#### Scenario: Update prunes hidden IDs
- **WHEN** an announcement update prunes hidden IDs during another write
- **THEN** the pruned state participates in the same serialized scheduling
