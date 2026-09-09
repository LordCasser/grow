## ADDED Requirements

### Requirement: Delegated turns preserve complete Goal ownership
A Goal-owned descendant turn SHALL record both goal_id and the matching definition_revision from its inherited immutable Goal context when its local tracker has no matching owner revision. Timeline SHALL continue rejecting partial or mismatched ownership evidence.

#### Scenario: Child starts with an empty local Goal tracker
- **WHEN** the shared admission window names a Goal and inherited context identifies that same Goal
- **THEN** the descendant TurnStarted contains the Goal id and inherited revision and can cross the durable admission boundary without creating a local Goal runtime.

#### Scenario: Inherited context belongs to another Goal
- **WHEN** inherited context does not match the shared window Goal id and no matching local tracker supplies the revision
- **THEN** turn admission fails without inventing a revision or committing an invalid TurnStarted.
