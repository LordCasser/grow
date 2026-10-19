## ADDED Requirements

### Requirement: Linked session history is reclaimed as one durable unit

Automatic TTL cleanup SHALL treat sessions connected by a parent/child result reference as one reclaim component. It SHALL reclaim the component only when every member is independently stale and all parent spawn, child seed, child result, parent terminal, delivery receipt, and local recovery obligations are proven complete while exclusive writer ownership is held for every member. If any member is fresh, active, missing, unreadable, changed, unresolved, or over budget, cleanup SHALL retain the whole component.

Before removing any member, cleanup SHALL durably record the exact validated component and SHALL resume that committed reclaim after restart. An unresolved or unprovable reclaim intent SHALL block admission or deletion of its listed sessions and SHALL retain the evidence for repair. Per-session quarantine alone SHALL NOT be treated as group-atomic.

#### Scenario: Both members of a closed chain are stale
- **WHEN** a parent and all linked child sessions are stale, no writer is active, and their exact result and delivery chain is complete
- **THEN** automatic cleanup durably decides the whole component and removes every member, resuming the same decision after a crash if needed.

#### Scenario: One member of a closed chain is still fresh
- **WHEN** one session in a closed parent/child component is fresh while another member exceeds TTL
- **THEN** cleanup retains every member and does not remove only the stale endpoint.

#### Scenario: A writer or source cannot be proven
- **WHEN** a member has a live writer, changed identity, missing or malformed source, unresolved receipt, or exhausted scan budget
- **THEN** cleanup retains the entire component without writing a reclaim decision.

#### Scenario: Cleanup restarts after committing a component decision
- **WHEN** process interruption occurs after a durable component reclaim intent but before every listed directory is removed
- **THEN** cleanup resumes only that exact intent and does not expose a remaining half-chain as an ordinary session.

### Requirement: Explicit deletion preserves cross-session result sources

An explicit single-session delete SHALL reject a target whose canonical Timeline participates in a cross-session parent/child result chain. The refusal SHALL identify the linked session and explain that single-session deletion would remove recovery evidence. It SHALL NOT recursively delete related sessions.

#### Scenario: Explicit delete targets a linked parent or child
- **WHEN** a user explicitly deletes one session that has a validated cross-session result-chain edge
- **THEN** deletion is refused with the related session identity and no session directory is removed.

#### Scenario: Explicit delete targets an unlinked session
- **WHEN** a user explicitly deletes a session with no cross-session result-chain edge and no unfinished runtime handoff
- **THEN** the existing single-session quarantine and deletion behavior applies.
