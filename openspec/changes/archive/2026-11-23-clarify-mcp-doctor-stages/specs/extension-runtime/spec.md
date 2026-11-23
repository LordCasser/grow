## ADDED Requirements

### Requirement: MCP doctor reports the stage reached by each probe
MCP doctor SHALL report local transport setup, MCP handshake, and complete tool discovery as separate checks in text and JSON. A failed check SHALL identify its stage. The command SHALL exit with status 1 if any probed server fails and status 0 if all probed servers pass.

#### Scenario: HTTP initialization and discovery succeed
- **WHEN** an HTTP server completes the handshake and returns a nonempty catalog
- **THEN** both report formats identify local HTTP client creation, the successful MCP handshake with its negotiated protocol, and discovery with the complete tool count.

#### Scenario: Handshake stalls
- **WHEN** the server fails to complete the MCP handshake within its startup timeout
- **THEN** doctor identifies the handshake timeout and its duration, omits tool discovery, and exits 1.

#### Scenario: A later tool page stalls
- **WHEN** the handshake succeeds but the full tools/list traversal has not finished within the session initialization budget
- **THEN** doctor retains the successful handshake check, reports a tool discovery timeout with the initialization budget, and exits 1 without reporting a partial count.

#### Scenario: Stdio process starts but the catalog is invalid
- **WHEN** a stdio process starts and completes the handshake but returns an empty catalog or a pagination error
- **THEN** doctor identifies the process start and handshake as successful, reports failed tool discovery, and exits 1.
