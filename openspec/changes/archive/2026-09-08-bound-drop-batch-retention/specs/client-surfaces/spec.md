## ADDED Requirements

### Requirement: Drop batches bound retained image bytes
The shared drop classifier SHALL retain at most 50,000,000 encoded image bytes per paste. The existing per-file read limit SHALL continue to bound the candidate being examined before aggregate admission.

#### Scenario: Batch exceeds the retained budget
- **WHEN** another valid image would exceed the per-paste retained byte allowance
- **THEN** return no classified entries for the whole paste, allowing existing text fallback rather than delivering a partial batch.

#### Scenario: Exact budget with ordinary paths
- **WHEN** image bytes exactly fill the allowance and other entries are ordinary paths
- **THEN** preserve all entries in source order, without charging path text as image bytes.
