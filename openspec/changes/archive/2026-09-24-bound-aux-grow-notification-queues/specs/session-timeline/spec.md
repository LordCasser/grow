## ADDED Requirements

### Requirement: Auxiliary Grow delivery shares the session preview budget

Goal and Workflow Grow presentation snapshots SHALL reserve the session's shared byte credits before entering persistence or gateway queues; their credits SHALL remain held through the respective consumer completion. A failed auxiliary reservation or append during an active attempt SHALL prevent canonical admission. A subagent progress publisher SHALL retain no more than one outstanding transient gateway delivery and SHALL remain cancellable while it waits. Canonical Goal control and Workflow run state persistence SHALL remain independent of optional presentation snapshots.

#### Scenario: Auxiliary Grow sender meets a slow consumer

- **WHEN** Goal or Workflow snapshots are produced faster than persistence or gateway delivery completes
- **THEN** retained snapshot payloads stay within the shared session byte credits, and an exhausted sender skips the optional presentation snapshot instead of growing either queue without bound.

#### Scenario: Auxiliary Grow delivery fails during an attempt

- **WHEN** a Goal or Workflow snapshot cannot reserve credits or its durable append fails during an active attempt
- **THEN** the attempt's admission barrier or projection commit fails, and no candidate body becomes durable replay content.

#### Scenario: Transient child progress meets a slow client

- **WHEN** a subagent progress notification is awaiting gateway completion while further progress ticks occur
- **THEN** its publisher does not enqueue another progress notification and remains cancellable.
