## ADDED Requirements

### Requirement: Resident loads restore the current Workflow task state

A resident session/load SHALL give the loading client the current tracked Workflow revisions after history replay and before successful load completion. A Workflow snapshot SHALL follow the existing revision ordering and clear semantics. Applying the snapshot SHALL update task state without adding a historical transcript row or starting work.

#### Scenario: A second client joins while a Workflow child runs
- **WHEN** client A observes a newer live Workflow revision with a running child and client B attaches to the same resident session
- **THEN** client B receives the current revision and running child at load completion, including changes newer than the last durable display row.

#### Scenario: An existing client has a newer revision
- **WHEN** the view already observed a newer update or a clear
- **THEN** an older load snapshot does not replace that state, and applying the snapshot does not alter the transcript.
