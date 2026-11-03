## ADDED Requirements

### Requirement: Fork display ancestry does not establish a result-chain dependency

Single-session deletion SHALL distinguish fork display ancestry from canonical cross-session dependencies. Summary parent_session_id alone SHALL NOT block deletion of an independent fork or its source. Canonical Spawn and SubagentSeed references SHALL protect both endpoints, including an incoming Spawn whose child has not yet committed its Seed. Writer ownership, unfinished runtime handoffs and identity-bound quarantine SHALL retain their existing checks; deletion SHALL NOT recursively remove related history.

#### Scenario: Delete an independent fork or source
- **WHEN** a persisted fork has only summary ancestry and no canonical result-chain edge or unfinished handoff
- **THEN** either copy can be explicitly deleted while the other copy remains readable, in the same or another cwd.

#### Scenario: Parent committed Spawn before child Seed
- **WHEN** a canonical parent Spawn references the target but the target has no Seed or summary parent yet
- **THEN** deleting that target alone is refused with the parent identity and both histories remain.
