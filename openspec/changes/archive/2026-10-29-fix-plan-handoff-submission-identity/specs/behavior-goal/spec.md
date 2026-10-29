## ADDED Requirements

### Requirement: Plan handoff receipt versions identify the submitted plan

Plan handoff notification source versions SHALL use the positive session-wide Plan submission identity independently of the artifact revision. Artifact hash, positive revision and handoff kind SHALL remain part of source identity and owner validation. Different submissions of the same body SHALL remain distinct receipts; receipt admission SHALL NOT reject a valid submission merely because its ID differs from the document revision. This SHALL preserve existing event shapes, immutable historical receipts and exact current-handoff matching before execution.

#### Scenario: Another Plan cycle submits the same artifact body

- **WHEN** Plan exits and is entered again, and the same body is submitted with a new submission ID while its current-cycle artifact revision is one
- **THEN** the new handoff is admitted and consumed exactly once under the new submission identity without deduplicating against the previous cycle or reporting acknowledgement loss.

#### Scenario: Revision and submission identity differ

- **WHEN** a valid Plan handoff has positive artifact revision and submission ID with different values
- **THEN** Timeline accepts and replays that exact receipt, while zero submission IDs, zero revisions and malformed artifact identities remain invalid.
