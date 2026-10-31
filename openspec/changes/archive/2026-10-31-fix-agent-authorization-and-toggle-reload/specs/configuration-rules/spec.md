## ADDED Requirements

### Requirement: Subagent toggles reload into live admission and catalogs
A valid change to global `[subagents.toggle]` SHALL update the existing runtime without restart in both daemon and in-process hosts. Subsequent ordinary child admission SHALL use the current toggle snapshot, including contexts prepared before the update. Resident primary and ordinary child Agent catalogs SHALL refresh at the next safe Step or idle control boundary, including re-enabling Task after all permitted types were disabled. Agent replacement SHALL NOT reintroduce a catalog built under an obsolete toggle snapshot. Existing running children and frozen Workflow routes SHALL retain their lifecycle semantics. Other subagents settings retain their existing startup behavior.

#### Scenario: Disable and re-enable
- **WHEN** a registered type is disabled then re-enabled during a live session
- **THEN** subsequent admission follows each current snapshot and the next safe catalog publication removes then restores the available type, including Task when its catalog becomes empty then nonempty.

#### Scenario: Delete the toggle table
- **WHEN** a valid global configuration removes its toggle table or one name
- **THEN** omitted entries return to the existing default-enabled behavior without bypassing parent filters or depth ceilings.

#### Scenario: Invalid toggle update
- **WHEN** the file cannot parse or toggle values are not booleans
- **THEN** the last valid toggle snapshot remains effective and no permissive default snapshot is published.

#### Scenario: Agent selection prepared before reload
- **WHEN** an Agent replacement was prepared using older toggles and becomes publishable after a newer toggle update
- **THEN** its preparation is checked against current toggles before claiming the control and rebuilt outside the cancellation gate when stale; updates arriving during a claimed commit remain queued for the following safe boundary.
