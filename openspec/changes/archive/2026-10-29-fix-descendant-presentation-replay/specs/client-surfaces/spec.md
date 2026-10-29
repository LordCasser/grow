## MODIFIED Requirements

### Requirement: Descendant restoration reuses only boundary-bound validated facts
Session load MAY reuse root child-ownership facts already validated for the same session and history frontier, and MAY derive descendant ownership and ordered lifecycle/control presentation from the same validated read. Reuse SHALL NOT infer authority from UI rows, cross session/load boundaries or bypass storage identity, complete record validation, immediate parent/child canonical Seed/Result link checks, and child ownership checks. Lifecycle presentation SHALL be applied only when its parent session, subagent identity and child session match the canonical owned spawn for that source. Missing reuse data SHALL NOT establish an empty verified tree. Restoration SHALL preserve source-local replay identities, child identity/CWD/worktree/control semantics, history order and the existing full-history readiness barrier without pagination or background history restoration. Repeating forced restoration SHALL NOT add duplicate task rows for valid lifecycle facts that lack eventId. Restoration remains a presentation projection and SHALL NOT grant child execution or resume authority.

#### Scenario: Root history is reused at its captured frontier
- **WHEN** a successful load supplies validated ownership facts for its exact root session and emitted history frontier
- **THEN** descendant restoration uses those facts without rereading the root, and later appends cannot enter that earlier boundary or be confused with an empty result

#### Scenario: Descendant ownership and control share one read
- **WHEN** recovery visits a valid child parent containing nested lifecycle and model/Agent control facts
- **THEN** one complete validated read supplies ownership/enrichment and the ordered source-local projection, preserving identity and preventing duplicate live/replay rows

#### Scenario: Lifecycle presentation disagrees with canonical child ownership
- **WHEN** a descendant replay record names a different parent, subagent identity or child session than the validated canonical spawn
- **THEN** Pager does not project that lifecycle row or traverse the mismatched child, while unrelated valid child presentation remains available

#### Scenario: Forced restoration repeats an ID-less lifecycle record
- **WHEN** Pager restores the same canonical child lifecycle more than once and the replayed Spawned record has no eventId
- **THEN** the existing Started row is reused and no duplicate task row is added

#### Scenario: Reuse is absent, stale or foreign
- **WHEN** ownership data belongs to another session/load boundary, is malformed, or a required child read/link is missing or conflicting
- **THEN** recovery does not accept that data as ownership authority or a verified empty tree; it validates through the existing fallback or rejects the affected recovery without fabricating success or releasing input before required history and checks finish

#### Scenario: Optional ownership exceeds the response budget
- **WHEN** the root ownership projection would exceed the bounded optional response payload
- **THEN** the server omits the whole optimization and restoration uses the independently validated fallback, without truncating ownership into an incomplete or empty verified tree
