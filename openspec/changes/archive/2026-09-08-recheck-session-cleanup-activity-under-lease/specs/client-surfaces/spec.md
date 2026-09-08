## ADDED Requirements

### Requirement: Session cleanup rechecks activity under writer ownership
Session cleanup SHALL establish safe current deletion eligibility before removing history.

#### Scenario: Activity refreshed after initial scan
- **WHEN** a candidate looked expired during scanning but its same-entity activity was refreshed before cleanup acquired the writer lease
- **THEN** cleanup preserves it using the fresh activity timestamp

#### Scenario: Fresh eligibility cannot be established
- **WHEN** cleanup cannot read and validate current activity under its lease
- **THEN** it does not delete that candidate based on the earlier snapshot
