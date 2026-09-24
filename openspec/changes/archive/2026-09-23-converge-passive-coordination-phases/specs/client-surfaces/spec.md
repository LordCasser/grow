## ADDED Requirements

### Requirement: Passive inquiry rows converge by participant and phase

Pager SHALL correlate an incoming passive inquiry row by structured source peer and inquiry ID. Received, approved and terminal facts SHALL advance that row monotonically without consuming the primary turn's tool state. A lower phase or equal-phase replay SHALL NOT replace a newer live projection; terminal state SHALL remain final. A distinct peer with the same inquiry ID SHALL own a distinct row.

#### Scenario: Approval arrives before an older start
- **WHEN** a valid approval notice is displayed and a delayed start for the same source peer and inquiry ID arrives
- **THEN** the existing row retains approval detail and identity without a second row.

#### Scenario: Completion precedes older notices
- **WHEN** a terminal inquiry outcome arrives before start or approval and those older notices later arrive live or by replay
- **THEN** the terminal row remains final and is not duplicated or restarted.

#### Scenario: Parent and child sources interleave
- **WHEN** distinct source peers deliver notices carrying the same inquiry ID in alternating order
- **THEN** each peer's row advances independently and neither consumes the other's completion.
