## ADDED Requirements

### Requirement: Descendant restoration reuses only boundary-bound validated facts

Session load MAY reuse root child-ownership facts already validated for the same session and history frontier, and MAY derive descendant ownership and ordered lifecycle/control presentation from the same validated read. Reuse SHALL NOT infer authority from UI rows, cross session/load boundaries or bypass storage identity, complete record validation and child ownership checks. Missing reuse data SHALL NOT establish an empty verified tree. Restoration SHALL preserve source-local replay identities, child identity/CWD/worktree/control semantics, history order and the existing full-history readiness barrier without pagination or background history restoration.

#### Scenario: Root history is reused at its captured frontier
- **WHEN** a successful load supplies validated ownership facts for its exact root session and emitted history frontier
- **THEN** descendant restoration uses those facts without rereading the root, and later appends cannot enter that earlier boundary or be confused with an empty result.

#### Scenario: Descendant ownership and control share one read
- **WHEN** recovery visits a valid child parent containing nested lifecycle and model/Agent control facts
- **THEN** one complete validated read supplies ownership/enrichment and the ordered source-local projection, preserving identity and preventing duplicate live/replay rows.

#### Scenario: Reuse is absent, stale or foreign
- **WHEN** ownership data belongs to another session/load boundary, is malformed, or a required child read/link is missing or conflicting
- **THEN** recovery does not accept that data as ownership authority or a verified empty tree; it validates through the existing fallback or rejects the affected recovery without fabricating success or releasing input before required history and checks finish.

#### Scenario: Optional ownership exceeds the response budget
- **WHEN** the root ownership projection would exceed the bounded optional response payload
- **THEN** the server omits the whole optimization and restoration uses the independently validated fallback, without truncating ownership into an incomplete or empty verified tree.
