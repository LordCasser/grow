## MODIFIED Requirements

### Requirement: Inquiry presentation identifies the actual coordination participant

Receiving-side inquiry audits SHALL retain whether the inquiry came through peer coordination or direct parent-child delegation and SHALL identify the actual direction and counterpart. A parent receiving a child's inquiry SHALL identify the coordinator-owned subagent task name; a child receiving its parent's inquiry SHALL identify the parent and retain the participating child task in details. Peer coordination SHALL continue to identify the source session. Presentation SHALL derive identity from structured audit facts, never UUID shape, visible rows, or title prose.

#### Scenario: Subagent asks its parent
- **WHEN** a running subagent with task name `TS registry workload presentation` asks its immediate parent and the inquiry is answered
- **THEN** the receiving row identifies that subagent task through receipt and completion, rather than displaying only the child session id.

#### Scenario: Parent asks its subagent
- **WHEN** a primary agent asks a directly-owned subagent
- **THEN** the child-side receiving row identifies the parent as the source and keeps its own participating task identity in details, while the source tool remains the primary-side interaction surface.

#### Scenario: Another primary session asks
- **WHEN** an inquiry arrives through authenticated peer coordination
- **THEN** the receiving row uses `session <source session id>` as the counterpart and does not label the peer as a parent or subagent; a shortened display retains the full id in details.

#### Scenario: Reconnect or replay
- **WHEN** a delegation inquiry is replayed or its terminal update arrives after reconnect
- **THEN** persisted direction and participant identity update the same correlated row without reversing source and target, reverting to ambiguous session identity, or duplicating the inquiry.

#### Scenario: Foreground completes during sideband work
- **WHEN** the receiving foreground turn ends while an inquiry is active
- **THEN** the inquiry row continues until its own terminal outcome and is not completed by foreground tool cleanup.

