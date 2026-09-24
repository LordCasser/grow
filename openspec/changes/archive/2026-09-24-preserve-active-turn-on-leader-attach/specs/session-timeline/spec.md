## ADDED Requirements

### Requirement: Resident reconnect preserves live turn ownership

A viewer reconnecting to a resident session SHALL NOT run interrupted-scope recovery against that session's live turn. Such recovery SHALL occur only after a replacement writer claims a new incarnation. Subagent fact reconciliation MAY continue during resident reconnect without closing unrelated active work.

#### Scenario: Viewer attaches during a live turn

- **WHEN** a leader viewer loads a resident session while its original writer still has an active turn
- **THEN** the viewer receives replay/live updates without an interrupted Recovery terminal for that turn, and the original actor records its matching real terminal before later prompts run.
