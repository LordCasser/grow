## MODIFIED Requirements

### Requirement: Deferred child history precedes resumed live presentation

After a root session resume, Pager SHALL restore a deferred child's existing transcript before applying its first live transcript update or terminal presentation. A live Thinking, assistant message, tool record or completion footer SHALL NOT prevent later access to the child's task prompt and previous execution records. Replay and control-only restoration SHALL retain deferred loading, and already-restored children SHALL NOT replay their history again while that transcript remains resident.

#### Scenario: Live child output arrives before first open
- **WHEN** a child has deferred history after root resume and its live Thinking, assistant or tool output arrives before the user opens it
- **THEN** its task prompt and previous execution records precede the new output, and reopening shows each record once.

#### Scenario: Child finishes before first open
- **WHEN** a deferred child receives a live child terminal or turn-completed notification before first open
- **THEN** existing history is restored before terminal presentation, with no missing prompt or execution records and no repeated completion footer on open.

#### Scenario: Only restoration facts arrive
- **WHEN** root replay and child control restoration complete without child live transcript updates
- **THEN** child transcript loading remains deferred until first open.

## ADDED Requirements

### Requirement: Completed child transcripts release reconstructible resident state

Pager SHALL release a finished, idle, unselected child's resident transcript, tool tracking and associated render resources when no replay/reload or viewer/interaction owner requires them. It SHALL retain canonical history, child identity, control generation, draft and interaction ownership, and live deduplication highwaters. Normal and Minimal SHALL share this lifecycle. Reopening SHALL reconstruct the transcript through the existing history projector. A failed history read SHALL remain visible and retryable without accepting a partial loaded state or preventing later history restoration.

#### Scenario: Sequential children finish
- **WHEN** many children finish sequentially without being selected
- **THEN** their transcript payloads and tool tracking are released while task metadata and durable history remain available.

#### Scenario: Selected completed child is left
- **WHEN** a selected child finishes and the user subsequently leaves its view
- **THEN** its visible transcript is retained until the selection is left, after which its reconstructible heavy state is released.

#### Scenario: Child is reopened repeatedly
- **WHEN** an unloaded child is opened, left and opened again
- **THEN** each open restores the task and execution history once, without losing current control identity, draft or terminal state.

#### Scenario: History read fails and recovers
- **WHEN** an unloaded child's history read fails and its original history later becomes readable
- **THEN** the failure is shown without marking history loaded, and another open restores the original history without duplicate partial content.

#### Scenario: Late presentation follows reclamation
- **WHEN** a late child notification arrives after reclamation
- **THEN** retained highwaters reject duplicates, and any accepted live presentation restores prior history before appending new content; a failed read does not block a later successful restoration.

#### Scenario: A presentation owner is still active
- **WHEN** a child is running, selected, reloading, or still has a viewer or transport interaction owning its presentation
- **THEN** reclamation does not discard that owner's state or response responsibility.

#### Scenario: Background task history is reconstructed
- **WHEN** a child with background task lifecycle rows is reclaimed and reopened
- **THEN** its original task prompt, start and terminal rows are restored once through the shared presentation reducers, entry links target the reconstructed rows, and existing stdout, cancellation and task state owners are preserved rather than replaced by history.
