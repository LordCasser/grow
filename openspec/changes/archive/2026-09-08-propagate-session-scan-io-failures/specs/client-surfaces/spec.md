## ADDED Requirements

### Requirement: Session scans propagate operational failures
Session discovery SHALL distinguish confirmed missing or invalid entries from operational failures. NotFound and InvalidData candidate errors MAY be skipped; other directory-open, summary-read or physical-identity-read errors SHALL fail the scan rather than return a successful partial result.

#### Scenario: Candidate cannot be inspected
- **WHEN** a candidate directory, summary or required CWD marker cannot be read due to permission or another operational failure
- **THEN** discovery returns the error and cleanup does not start deleting from a partial candidate set.

#### Scenario: Invalid or vanished candidate
- **WHEN** a candidate is confirmed missing or invalid
- **THEN** discovery retains the existing skip behavior while processing other valid entries.

#### Scenario: Search discovery fails
- **WHEN** search bootstrap receives a discovery error
- **THEN** that attempt does not proceed to orphan pruning or completed-bootstrap publication using partial results.
