## ADDED Requirements

### Requirement: Minimal passive inquiry frontier follows typed terminal phase

Minimal Pager SHALL retain a passive inquiry row in its live region while its phase is Received or Approved, regardless of the primary turn's animation state. It SHALL commit the row to native scrollback only after the row reaches Terminal.

#### Scenario: An inquiry progresses while the primary turn is idle
- **WHEN** a passive row is Received or Approved and the primary turn is idle
- **THEN** the row remains live and can accept a later terminal update before print-once commit.

#### Scenario: Inquiry reaches terminal
- **WHEN** the row's typed phase becomes Terminal
- **THEN** Minimal may commit it to native scrollback regardless of the primary turn state.
