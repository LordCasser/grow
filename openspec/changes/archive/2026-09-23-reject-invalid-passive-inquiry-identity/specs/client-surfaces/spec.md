## ADDED Requirements

### Requirement: Invalid passive inquiry identities remain finite notices
Pager SHALL coalesce an incoming coordination notice into a passive inquiry row only when its structured audit and non-empty correlation ID provide a valid identity. A notice without valid identity SHALL remain visible as a finite raw notice and SHALL NOT enter passive running lifecycle. The scrollback upsert SHALL reject absent or empty identity without panicking or mutating state.

#### Scenario: Incoming notice has no structured audit or usable inquiry ID
- **WHEN** an incoming inquiry has missing or malformed structured audit data, or an empty correlation ID
- **THEN** Pager retains the original notice as a finite row and creates no passive coordination entry.

#### Scenario: Internal upsert receives absent or empty identity
- **WHEN** `upsert_coordination_row` receives a block without a coordination identity, or with empty source peer or inquiry ID
- **THEN** it returns without mutation and does not panic or create a running row.

#### Scenario: Valid inquiry identity
- **WHEN** a non-empty source peer ID and correlation ID accompany a valid audit
- **THEN** the existing correlated passive row behavior and running/terminal projection remain unchanged.
