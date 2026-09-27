## MODIFIED Requirements

### Requirement: Subagent lifecycle projections are bounded metadata

Subagent lifecycle Grow notifications SHALL carry status and identity metadata without duplicating the child final output. The canonical child result and admitted completion receipt SHALL remain the source of that output. The parent actor SHALL persist and forward one stamped event under the existing independent-update and active-attempt gateway budget boundaries. It SHALL forward a spawned lifecycle projection before emitting a later-stamped Goal status refresh caused by that spawn, so live clients can retain the running child. If an independent live update overtakes a lifecycle projection, the client SHALL reconcile missing spawn/finish facts by child identity without regressing its Grow highwater or reconnect cursor or reviving an already terminal child. A failed preview gateway reservation SHALL reject that exact attempt's canonical admission. Client-origin Grow notifications SHALL remain persist-only.

#### Scenario: Child finishes with a large answer during parent sampling

- **WHEN** a child result contains a large final answer and its parent has an active sampling attempt
- **THEN** the lifecycle Grow projection contains only metadata, its forwarded event uses the same event ID as its durable record, and the completion receipt continues to reference the full answer.

#### Scenario: Lifecycle preview gateway budget is exhausted

- **WHEN** the parent actor cannot reserve preview gateway credits for a child lifecycle projection
- **THEN** no uncredited lifecycle payload enters the gateway queue, that attempt fails preview admission, and canonical child lifecycle facts remain available for reconnect projection.

#### Scenario: Goal-owned child spawn refreshes Goal status

- **WHEN** a Goal-owned child spawn reaches the parent actor and triggers a Goal status refresh
- **THEN** the stamped SubagentSpawned projection is persisted and forwarded before the later-stamped GoalUpdated projection, and the live Tasks pane retains the running child.

#### Scenario: Independent live update overtakes a child lifecycle projection

- **WHEN** a higher-ID Grow update reaches a live client before a child spawn or finish whose durable projection is still pending
- **THEN** the client applies the missing child lifecycle fact once by child identity, retains its higher Grow highwater and reconnect cursor, and does not resurrect a child with an already visible terminal row.
