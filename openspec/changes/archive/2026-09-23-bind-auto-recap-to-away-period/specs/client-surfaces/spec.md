## MODIFIED Requirements

### Requirement: Automatic recap bookkeeping is session scoped
Within an away period, showing a recap or attempting automatic recap for one session SHALL not suppress another session's eligibility. Polling and focus-return eligibility SHALL use the active root session identity. A new away period SHALL reset all session recap bookkeeping while retaining the existing timing thresholds. Each automatic request SHALL carry the owning away-period ID, and its successful live notification SHALL echo that ID. Pager SHALL display and count a live automatic recap only when the ID matches its current away period. A replayed recap SHALL restore display without changing current eligibility; manual recap behavior SHALL remain independent of this automatic request identity.

#### Scenario: Background result arrives
- **WHEN** a live recap is displayed for a background session in the current away period
- **THEN** only that session's shown state is consumed

#### Scenario: Another session is in backoff
- **WHEN** one session has recently attempted automatic recap
- **THEN** the retry delay applies only to that session

#### Scenario: New away period
- **WHEN** focus is lost for a new away period
- **THEN** prior per-session shown and retry state is cleared, and a new ID is minted

#### Scenario: Old automatic result arrives in a new away period
- **WHEN** an automatic recap for away period A arrives live after period B has begun
- **THEN** Pager does not display the A result or mark B shown, and B remains eligible under its own backoff

#### Scenario: Current automatic result arrives after focus return
- **WHEN** a recap for the current away period arrives after focus returns but before another focus loss
- **THEN** Pager may display it if the session remains eligible and counts it against that period

#### Scenario: Historical or manual recap
- **WHEN** a historical recap is replayed or a manual recap is produced
- **THEN** replay affects only display, and manual feedback follows the manual path without requiring an away-period ID
