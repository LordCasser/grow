## ADDED Requirements

### Requirement: MCP server name is the session identity

Within one session, an MCP server's `name` SHALL be the single identity used by its catalog directory entry, client, and tool-name prefix. Configuration SHALL preserve the established source precedence, including configuration over plugin and client precedence where applicable. When same-name entries from different sources refer to different endpoints, the catalog SHALL select one authoritative entry deterministically according to that precedence and SHALL NOT combine identity from one entry with the endpoint or client of another. A single client-provided server array containing duplicate names SHALL be rejected.

#### Scenario: Same name appears across sources with different endpoints
- **WHEN** applicable configuration sources provide the same server name with different endpoints
- **THEN** one authoritative entry is selected by the existing source precedence and its name, client, endpoint, and tool prefix remain associated

#### Scenario: Client array repeats a server name
- **WHEN** one client-provided server array contains the same server name more than once
- **THEN** the array is rejected with a clear validation error and no ambiguous catalog is published

### Requirement: Explicit MCP call identity matches the current catalog

A direct MCP call that supplies both server `name` and URL SHALL be accepted only when both identify the same current authoritative catalog entry. A URL match SHALL NOT redirect a mismatched name to a different server. The identity check SHALL continue to apply across cold recovery and hot replacement, so calls and published tools cannot use a stale or mixed identity.

#### Scenario: Explicit name and URL identify different entries
- **WHEN** a direct call provides a name and URL that do not belong to the same current authoritative catalog entry
- **THEN** the call is rejected without routing to either entry by partial match

#### Scenario: Explicit name and URL match the current entry
- **WHEN** a direct call provides the current authoritative entry's name and URL
- **THEN** the call is routed through that entry's client

#### Scenario: Cold recovery rebuilds server identity
- **WHEN** a session recovers after its previous runtime stopped
- **THEN** each catalog name, client, endpoint, and tool prefix are rebuilt from the same current authoritative entry

#### Scenario: Hot replacement completes while old work is pending
- **WHEN** a same-name server is replaced while initialization or call-discovery work for the previous endpoint is pending
- **THEN** the old work cannot publish old tools as current or route a new call through the previous endpoint

#### Scenario: Configuration mutation shares catalog authority
- **WHEN** setup, toggle, upsert, or delete changes a disk definition whose name also exists in client input
- **THEN** the live session applies the complete merged catalog using the same source precedence as reload, and its displayed configuration and invocation use that same authoritative entry.
