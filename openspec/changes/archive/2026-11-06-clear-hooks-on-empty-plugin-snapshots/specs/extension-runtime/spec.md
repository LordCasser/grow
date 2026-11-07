## ADDED Requirements

### Requirement: Empty plugin snapshots revoke prior Hook contributions

Adopting a session plugin snapshot SHALL replace that snapshot's complete Hook contribution, including when the new registry is None or has no active plugins. Previously registered `plugin/` handlers SHALL no longer execute after their contribution disappears. Other configuration, file and Agent Hook sources SHALL remain intact. The existing Hook generation and listing SHALL reflect the adopted snapshot, while MCP and skill adoption retain their existing ownership and refresh rules.

#### Scenario: Last plugin disappears

- **WHEN** a session has an executable plugin Hook and adopts an empty None plugin snapshot
- **THEN** the plugin list and Hook list no longer expose that contribution, the next matching event does not execute its handler, and non-plugin handlers still execute.

#### Scenario: Plugins remain but none are active

- **WHEN** a session adopts Some registry with no active plugins
- **THEN** prior plugin handlers are removed with the same effect as None and no unrelated Hook contribution is removed.

#### Scenario: Empty snapshot without a Hook registry

- **WHEN** no Hook registry exists and the adopted plugin snapshot has no Hook specifications
- **THEN** adoption does not discover unrelated Hook sources or create an unnecessary registry, and existing MCP/skills adoption still proceeds.
