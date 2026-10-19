# extension-runtime Specification Delta

## ADDED Requirements

### Requirement: MCP catalog lifecycle follows the effective configuration

An ACP session's advertised MCP tools, callable MCP tools, and owned server connections SHALL converge on the session's current effective merged catalog after configuration changes. Removal or replacement SHALL retire the superseded server and its tools. An asynchronous initialize or reconnect result started for an older configuration SHALL NOT restore a removed or replaced server. Catalog changes SHALL remain scoped to the sessions and cwd for which the existing merge rules make the configuration applicable. Cold recovery SHALL rebuild from current applicable configuration and preserve client-source behavior supported by the existing session recovery semantics; enabled project watching SHALL resume for the recovered session's cwd.

#### Scenario: Removed server is no longer listed or callable
- **WHEN** an applicable configuration update removes an MCP server from a live session's effective catalog
- **THEN** the session no longer advertises that server's tools, calls cannot use its retired connection, and unrelated servers remain available.

#### Scenario: Replaced server converges on the replacement
- **WHEN** an applicable configuration update replaces a server while retaining its catalog identity
- **THEN** the session advertises and calls through the replacement configuration, and the superseded connection and tools are retired.

#### Scenario: Stale connection work completes after removal or replacement
- **WHEN** initialize or reconnect work for an old server configuration completes after a newer catalog update removed or replaced it
- **THEN** the stale result cannot restore the old connection, readiness, configuration, or tools.

#### Scenario: A direct call names a retired endpoint
- **WHEN** a direct MCP call includes an explicit server URL that is no longer in the session's effective catalog
- **THEN** it is rejected without routing to a same-name server at another URL.

#### Scenario: Recovery or list notification changes tool definitions
- **WHEN** the enabled session event dispatcher observes a current connection becoming ready after recovery or publishing a tools-list change
- **THEN** it refreshes the server's complete tool definitions and search snapshot, including an empty tool list, even if automatic transport recovery is disabled.

#### Scenario: Session dispatcher stops during catalog discovery
- **WHEN** the session event dispatcher is stopped while its catalog discovery is pending
- **THEN** its pending discovery tasks are cancelled and cannot later publish a catalog or continue discovery for another session.

#### Scenario: A retired client's call fails late
- **WHEN** an already-dispatched call reports a recoverable failure after its client was removed or replaced
- **THEN** it returns the failure without reconnecting or retrying the retired client and without routing the call to the replacement.

#### Scenario: Catalog changes remain isolated by session cwd
- **WHEN** two sessions use different cwd values and a project MCP configuration changes for one cwd
- **THEN** only sessions applicable to that project configuration update their catalog; the other session's listed and callable tools remain unchanged.

#### Scenario: A nested session consumes an ancestor project configuration
- **WHEN** a session's configuration merge reads an ancestor directory up to its repository root
- **THEN** the same directory chain is watched for configuration creation, replacement, and deletion; removing another cwd's registration does not remove shared ancestor watches still needed by the session.

#### Scenario: Cold recovery rebuilds from current configuration
- **WHEN** an ACP session is recreated after its prior runtime has stopped and applicable MCP configuration changed while it was stopped
- **THEN** recovery builds the effective catalog from current applicable configuration, retains client-source behavior provided by the existing recovery inputs and precedence rules, and restores project watching when enabled for that session cwd.
