## ADDED Requirements

### Requirement: Session cleanup TTL cannot wrap or overflow
Session cleanup SHALL establish safe current deletion eligibility before removing history.

#### Scenario: Oversized configured retention
- **WHEN** configured TTL cannot be represented safely
- **THEN** cleanup reports the invalid policy and does not delete session history

#### Scenario: Cutoff cannot be represented
- **WHEN** subtracting the selected retention would exceed supported date bounds
- **THEN** cleanup fails without panic or deletion

#### Scenario: Default retention
- **WHEN** no cleanup TTL is configured
- **THEN** the existing 30-day policy applies
