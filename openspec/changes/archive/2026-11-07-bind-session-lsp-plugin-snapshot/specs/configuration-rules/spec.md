## ADDED Requirements

### Requirement: Session LSP construction uses its own plugin snapshot

When Shell constructs a new session LSP backend, it SHALL derive active plugin LSP configurations from the same cwd, trust and session-plugin-root snapshot used to construct that session. A process shared latest snapshot SHALL NOT add or replace another session's plugin execution configuration. Existing user/project/plugin precedence, project trust filtering, LSP enablement and inherited backend ownership SHALL remain in force.

#### Scenario: Different cwd values have the same server name

- **WHEN** sessions A and B have different permitted plugin snapshots defining the same LSP server name
- **THEN** each newly constructed backend uses and launches only the plugin command permitted for its own snapshot.

#### Scenario: Session plugin roots differ at the same cwd

- **WHEN** two sessions share cwd but specify different pluginDirs with file or inline LSP definitions
- **THEN** their newly constructed backends include the corresponding session roots without using cwd as a substitute for session identity.

#### Scenario: Current session has no active plugin contribution

- **WHEN** a new session's plugin snapshot is absent or contains no active plugins while the process latest snapshot still contains another session's LSP
- **THEN** the new backend does not use that latest plugin contribution, and its own allowed user and project configuration remains eligible.

#### Scenario: A backend was already inherited

- **WHEN** the ToolContext already carries the parent's LSP backend
- **THEN** bootstrap preserves that backend and its existing lifecycle rather than replacing it because another plugin snapshot exists.
