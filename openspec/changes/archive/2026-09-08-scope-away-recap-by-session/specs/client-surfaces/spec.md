## ADDED Requirements

### Requirement: Automatic recap bookkeeping is session scoped
Within an away period, showing a recap or attempting automatic recap for one session SHALL not suppress another session's eligibility. Polling and focus-return eligibility SHALL use the active root session identity. A new away period SHALL reset all session recap bookkeeping while retaining the existing timing thresholds.

#### Scenario: Background result arrives
- **WHEN** a live recap is displayed for a background session
- **THEN** only that session's shown state is consumed

#### Scenario: Another session is in backoff
- **WHEN** one session has recently attempted automatic recap
- **THEN** the retry delay applies only to that session

#### Scenario: New away period
- **WHEN** focus is lost for a new away period
- **THEN** prior per-session shown and retry state is cleared
