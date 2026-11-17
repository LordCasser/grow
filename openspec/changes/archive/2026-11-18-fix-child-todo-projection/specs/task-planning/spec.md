## ADDED Requirements

### Requirement: Child TODO projections follow live and replayed Plans

Pager SHALL project each child session's committed Plan updates into that child's Todo pane both during live ACP routing and deferred child transcript replay. The projection SHALL use the existing Plan-entry conversion and SHALL NOT update the root session's Todo pane. Once a live child Plan is applied, deferred transcript replay SHALL NOT replace it with an older persisted Plan, including when the live Plan is empty.

#### Scenario: Live child Plan update
- **WHEN** a registered child receives an ACP Plan update
- **THEN** that child's Todo pane reflects the Plan entries and statuses while the root Todo pane remains unchanged.

#### Scenario: Deferred child Plan replay
- **WHEN** a child transcript containing committed Plan updates is replayed later
- **THEN** the child Todo pane reflects the replayed Plan entries and statuses.

#### Scenario: Live Plan precedes deferred replay
- **WHEN** a live child Plan arrives before deferred transcript replay, including an empty Plan that clears TODOs
- **THEN** older persisted Plan updates do not replace the live child Todo projection.

#### Scenario: Same-session reconnect replays persisted child Plan
- **WHEN** a child view reconnects to the same session identity after its local Plan changed while detached, then receives a new live Plan during the reconnect window
- **THEN** the prior connection's live precedence is cleared so deferred replay can restore the persisted Plan, while the new connection's live Plan still takes precedence over older history.

#### Scenario: An already opened child changes its Plan while the parent is detached
- **WHEN** the parent reconnects with a previously opened child and the validated child Timeline includes a newer committed Plan
- **THEN** the child Todo pane refreshes without replaying its transcript, unless the new connection already delivered a live Plan for that child.
