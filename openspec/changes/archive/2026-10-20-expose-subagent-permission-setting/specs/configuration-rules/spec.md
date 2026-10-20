## ADDED Requirements

### Requirement: Subagent permission settings update only the authored policy
The Settings writer SHALL persist the existing typed RequestPermissionMode into `[subagents].permission_mode` under the shared config write lock and atomically replace the config. It SHALL preserve unrelated values, unknown subagent fields and literal environment references without materializing other SubagentsConfig defaults. Invalid source text or a non-table subagents section SHALL fail without replacing the original bytes. The saved policy SHALL be read by the existing startup subagent resolver.

#### Scenario: Save with existing sibling settings
- **WHEN** a subagent permission mode is saved alongside a primary permission default, unknown subagent keys and environment references
- **THEN** only the child permission policy changes and startup resolves the saved mode.

#### Scenario: Create or reject source
- **WHEN** the file is absent or its existing source/section is invalid
- **THEN** absence permits creation with the requested policy, while invalid source or section causes a save error and preserves the original file.
