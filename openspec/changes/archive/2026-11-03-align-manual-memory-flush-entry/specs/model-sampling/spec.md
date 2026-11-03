## ADDED Requirements

### Requirement: Context consuming auxiliary extensions wait for session load

Side questions, memory flush and manual compaction extensions SHALL use the existing session/load publication barrier before looking up the target actor. A concurrent load SHALL complete or fail before the extension decides whether the session exists. Closed command channels SHALL return admission failure; no successful acknowledgement SHALL be inferred from an ignored send error.

#### Scenario: Reconnect load overlaps auxiliary input
- **WHEN** a side question, memory flush or manual compaction arrives while its session/load is in flight
- **THEN** it waits for load publication and dispatches to the restored actor, or reports absence only after the load failed.

#### Scenario: Actor command channel closed
- **WHEN** the resolved actor no longer accepts commands
- **THEN** the extension returns an admission error without claiming provider execution or memory persistence.
