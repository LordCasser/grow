## ADDED Requirements

### Requirement: Unavailable child output cannot establish a new successful result
Recovery SHALL NOT synthesize a successful canonical child result from CompletedOutputUnavailable alone. An existing valid canonical result SHALL preserve its outcome independently of output availability. Without such a result the parent spawn SHALL remain open and no successful terminal or completion delivery SHALL be forged.

#### Scenario: Child exists without a published result
- **WHEN** inspection says CompletedOutputUnavailable and valid child storage contains Seed but no SubagentResult
- **THEN** recovery leaves child and parent terminal facts unchanged and reports result unavailable.

#### Scenario: Existing completed result loses its output
- **WHEN** a valid canonical Completed result exists but its referenced output is missing or corrupt
- **THEN** recovery reuses the exact result reference and outcome without rewriting history or guessing output.
