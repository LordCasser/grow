## ADDED Requirements

### Requirement: Durable control transitions update Goal request scope atomically
A Session control transition SHALL publish its authorized Goal request scope through the same ChatState durable commit boundary as its Control fact. Scope SHALL follow active Goal identity/revision and selected Behavior before another request or context advertisement. Failed Control writes SHALL NOT publish a changed scope or pressure; route changes SHALL retain the current committed scope while clearing provider calibration. Lifetime usage SHALL remain unchanged.

#### Scenario: Stop or leave the active Goal
- **WHEN** pause, completion, budget stop, clear or a Behavior transition durably removes active Goal authorization
- **THEN** pressure before the next request uses no active Goal scope, without a transient old-scope update from the new Control content.

#### Scenario: Restart or change definition
- **WHEN** a durable Control activates a Goal or changes its active definition revision
- **THEN** the new tag governs context projection before the next build and survives model-route replacement.

#### Scenario: Persistence does not confirm control
- **WHEN** a proposed Control fails validation or durable persistence
- **THEN** no changed request scope/pressure is published; permanent storage failure retains existing owner shutdown and cold recovery semantics.

#### Scenario: Request projection overlaps a Goal control
- **WHEN** Goal scope capture and ChatState request preparation overlap a pause, clear or definition transition
- **THEN** the same existing Step control gate serializes scope capture with preparation so an older captured tag cannot overwrite the committed Control scope after an asynchronous image-mode query.
