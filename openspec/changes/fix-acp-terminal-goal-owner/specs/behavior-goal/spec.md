## ADDED Requirements

### Requirement: ACP background tasks retain complete Goal ownership
The ACP terminal backend SHALL retain goal_id and goal_definition_revision from background task admission in all task snapshots and completion notifications. Missing ownership SHALL remain absent; the adapter SHALL NOT invent or discard a revision.

#### Scenario: Goal-owned background command completes
- **WHEN** an ACP background task is admitted with a Goal id and definition revision
- **THEN** task lookup and completion notification preserve both fields unchanged for durable notification admission.

#### Scenario: Unowned background command
- **WHEN** an ACP background task has no Goal owner
- **THEN** its snapshots retain no Goal id or revision.
