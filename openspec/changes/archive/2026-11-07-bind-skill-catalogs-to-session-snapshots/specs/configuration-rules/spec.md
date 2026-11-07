## ADDED Requirements

### Requirement: Session skill catalog discovery uses the named owner

Skills catalog and configuration requests SHALL resolve a supplied sessionId to that exact resident primary owner, use its actual execution cwd and acknowledged plugin snapshot, and fail when that owner cannot answer. A successful empty plugin snapshot SHALL remain distinct from a closed command or reply channel. Requests without sessionId SHALL retain each method's existing cwd parameter rules and discover plugins for that cwd without borrowing another session's plugin roots or the shared latest registry.

#### Scenario: Sessions have different plugin roots in the same directory

- **WHEN** two sessions share cwd but have different session pluginDirs and each requests its skill catalog by sessionId
- **THEN** each result uses the named session's snapshot, even if the shared latest registry contains another session's plugins.

#### Scenario: Owner is absent or retired

- **WHEN** a supplied sessionId has no resident primary or its owner cannot confirm the plugin snapshot
- **THEN** the request fails before any associated configuration mutation and does not fall back to another session or a successful native-only catalog.

#### Scenario: Owner acknowledges no plugin snapshot

- **WHEN** the exact owner successfully returns an empty plugin snapshot
- **THEN** discovery succeeds using that session's cwd and native skill sources.

#### Scenario: Conflicting request contexts

- **WHEN** a request supplies both a nonempty sessionId and a nonnull cwd string
- **THEN** it returns invalid_params before discovery or configuration mutation; cwd:null is treated as omitted.

#### Scenario: Discovery without a session

- **WHEN** no sessionId is supplied and the request satisfies that method's existing cwd rule
- **THEN** discovery builds a read-only registry for that cwd, retains explicit process plugin roots, excludes session pluginDirs, and does not publish the shared latest snapshot.

#### Scenario: Fresh management catalog includes inactive candidates

- **WHEN** a skill is disabled by global configuration or awaits conditional runtime activation
- **THEN** the management catalog retains its existing display semantics while runtime invocation remains governed by the named session's SkillManager.
