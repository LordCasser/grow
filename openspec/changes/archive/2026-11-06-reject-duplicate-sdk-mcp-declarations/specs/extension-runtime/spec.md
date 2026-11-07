## ADDED Requirements

### Requirement: SDK MCP duplicate declarations fail before session effects

A recognized client SDK MCP declaration array SHALL be rejected when its successfully parsed entries repeat the canonical MCP server name, rather than silently retaining the first entry. New/load ingress SHALL return invalid parameters before session creation, storage preparation or retirement/replacement of an existing session. Valid declarations, absent metadata and established handling of malformed individual entries SHALL retain their current behavior. Cross-source precedence SHALL remain unchanged, and this requirement SHALL NOT add SDK declarations to an update endpoint that does not currently support them.

#### Scenario: New session repeats an SDK name

- **WHEN** session/new supplies two parsed grow/mcp/servers entries with the same name and different serverId values
- **THEN** the whole declaration array is rejected with a duplicate-name validation error before publishing or preparing a session.

#### Scenario: Load repeats a name while a session is resident

- **WHEN** session/load supplies repeated SDK names for an existing resident session
- **THEN** the request fails before lifecycle effects, and the existing handle, catalog and command stream remain unchanged.

#### Scenario: Valid SDK registration remains accepted

- **WHEN** SDK metadata is absent or contains distinct valid names together with entries handled by the established malformed-entry policy
- **THEN** parsing preserves the existing valid registration set without changing precedence or reverse routing.
