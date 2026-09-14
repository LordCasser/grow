## ADDED Requirements

### Requirement: Inquiry presentation identifies the actual coordination participant

Receiving-side inquiry audits SHALL retain whether the inquiry came through peer coordination or direct parent-child delegation. Direct delegation titles SHALL identify the participating subagent by its coordinator-owned task name; peer coordination titles SHALL continue to identify the source session. Presentation SHALL derive this distinction from structured audit facts and SHALL NOT infer it from UUID shape, visible rows, or title prose.

#### Scenario: Subagent asks its parent
- **WHEN** a running subagent with task name `TS registry workload presentation` asks its immediate parent and the inquiry is answered
- **THEN** the receiving row completes as `Answered subagent TS registry workload presentation` rather than displaying the child session id.

#### Scenario: Parent asks its subagent
- **WHEN** a primary agent asks a directly-owned subagent
- **THEN** the child-side receiving audit identifies that same subagent task, and the source tool remains the primary-side interaction surface.

#### Scenario: Another primary session asks
- **WHEN** an inquiry arrives through authenticated peer coordination
- **THEN** the receiving row continues to use `session <source session id>` and does not label the peer as a subagent.

#### Scenario: Reconnect or replay
- **WHEN** a delegation inquiry is replayed or its terminal update arrives after reconnect
- **THEN** the persisted subagent task identity updates the same correlated row without reverting to a session id or duplicating the inquiry.
