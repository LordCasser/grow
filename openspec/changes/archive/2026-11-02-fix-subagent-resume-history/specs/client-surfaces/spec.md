## ADDED Requirements

### Requirement: Deferred child history precedes resumed live presentation

After a root session resume, Pager SHALL restore a deferred child's existing transcript before applying its first live transcript update or terminal presentation. A live Thinking, assistant message, tool record or completion footer SHALL NOT prevent later access to the child's task prompt and previous execution records. Replay and control-only restoration SHALL retain deferred loading, and already-restored children SHALL NOT replay their history again.

#### Scenario: Live child output arrives before first open
- **WHEN** a child has deferred history after root resume and its live Thinking, assistant or tool output arrives before the user opens it
- **THEN** its task prompt and previous execution records precede the new output, and reopening shows each record once.

#### Scenario: Child finishes before first open
- **WHEN** a deferred child receives a live child terminal or turn-completed notification before first open
- **THEN** existing history is restored before terminal presentation, with no missing prompt or execution records and no repeated completion footer on open.

#### Scenario: Only restoration facts arrive
- **WHEN** root replay and child control restoration complete without child live transcript updates
- **THEN** child transcript loading remains deferred until first open.
