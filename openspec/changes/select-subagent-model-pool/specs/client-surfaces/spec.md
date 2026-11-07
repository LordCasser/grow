## ADDED Requirements

### Requirement: Slash panel selects subagent model pools

Pager SHALL provide `/subagent-models` for the active primary session, using the existing modal and searchable model catalog conventions. The panel SHALL select models without effort controls, show the inherited root model, and submit the entire edited pool through the session-owned control API. It SHALL distinguish a local draft, pending application and an authoritative applied revision. Escape SHALL discard an unsubmitted draft. The selection SHALL survive restoration of that session without changing unrelated sessions.

#### Scenario: Apply and pending state

- **WHEN** the user selects models with Space and confirms with Enter
- **THEN** Pager submits one pool revision and keeps a pending projection until Shell acknowledges application; a persistence error preserves the prior authoritative pool.

#### Scenario: Inherited-only default

- **WHEN** the pool is empty
- **THEN** the panel clearly shows that new delegation is restricted to the current root model and effort is chosen by the Agent.

#### Scenario: Catalog unavailable selection

- **WHEN** a previously selected model is no longer selectable in the catalog
- **THEN** the panel marks it unavailable and the effective model list excludes it without substituting a different model under that selection.
