## MODIFIED Requirements

### Requirement: Live MCP auxiliary policy follows effective configuration

An existing session's effective MCP policy SHALL follow applicable configuration changes to `disabled_mcp_tools`, `startup_timeout_sec`, `tool_timeout_sec`, and `tool_timeouts`. Removing an override SHALL expose the remaining lower-priority setting or that field's default. Policy assembly SHALL retain existing metadata precedence and configuration trust/cwd applicability. Asynchronous work started under older policy SHALL NOT commit that policy after a newer effective policy is accepted. A call already admitted SHALL retain the deadline selected for that call; later policy changes apply to subsequently admitted calls.

Subagent sessions SHALL refresh their local disabled-tool gate and bridge registrations when effective global or trusted project policy changes, while ignoring client MCP catalog replacements. A subagent SHALL NOT mutate timeout policy on inherited shared clients; only the owning session refreshes those clients. Internal configuration reload SHALL enqueue this policy refresh for every active child whose cwd matches the reload scope, including workflow children, without replacing the child's frozen catalog or shared-client ownership.

#### Scenario: Existing session receives policy changes
- **WHEN** applicable configuration changes one or more of `disabled_mcp_tools`, `startup_timeout_sec`, `tool_timeout_sec`, or `tool_timeouts`
- **THEN** the existing session applies the new effective policy without requiring session recreation

#### Scenario: Policy override is removed
- **WHEN** an applicable configuration source removes an override for one of the policy fields
- **THEN** the session uses that field's default instead of retaining the previously effective override

#### Scenario: Metadata precedence is preserved
- **WHEN** a policy value from metadata and another applicable policy source provide competing values
- **THEN** the established metadata precedence determines the effective value

#### Scenario: Project policy is untrusted or outside session scope
- **WHEN** a project policy is not trusted under existing rules or does not apply to a session's cwd
- **THEN** that policy does not affect the session

#### Scenario: Older asynchronous work finishes after policy refresh
- **WHEN** initialization or reconnect started under an older policy completes after a newer effective policy is accepted
- **THEN** the older work cannot restore the old policy

#### Scenario: In-flight call crosses a policy update
- **WHEN** a call has been admitted with a timeout and the effective policy changes before that call completes
- **THEN** the in-flight call retains its admitted deadline and a subsequently admitted call uses the updated policy

#### Scenario: Policy changes while an admitted call waits for initialization
- **WHEN** a call begins initialization under one effective timeout and policy changes before initialization completes
- **THEN** initialization and the call remain bounded by the deadline captured at admission, and the new policy applies to later calls

#### Scenario: Subagent refreshes its local disabled gate
- **WHEN** an applicable global or trusted project disabled-tool policy changes while a subagent is active
- **THEN** the subagent reconciles its local bridge and runtime gate without accepting a client MCP catalog replacement or changing timeout policy on inherited shared clients

#### Scenario: Cold recovery reads current auxiliary policy
- **WHEN** a session runtime is recreated after its applicable disabled-tool or timeout configuration changed while stopped
- **THEN** it installs the current effective policy and does not recover an older cached mask or timeout from session history.

#### Scenario: Subagent policy refresh preserves inherited ownership
- **WHEN** an existing subagent receives a policy-only refresh while using inherited parent MCP clients
- **THEN** it reconciles its own applicable disabled-tool mask without accepting replacement client catalogs or mutating policy on shared parent clients.

#### Scenario: Internal reload reaches active child policy
- **WHEN** an internal configuration reload applies to an active child session's cwd
- **THEN** the child recomputes its local effective auxiliary policy through its actor command path
- **AND** its inherited MCP catalog and parent-owned shared client policy remain unchanged

#### Scenario: Scoped reload skips unrelated child
- **WHEN** an internal configuration reload names a project root that does not contain an active child's cwd
- **THEN** that child receives no policy refresh command
