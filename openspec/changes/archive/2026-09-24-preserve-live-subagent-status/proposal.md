## Why

A live child can be running while the parent's Tasks pane reports no running tasks. The parent persists `SubagentSpawned`, but its actor emits a newer `GoalUpdated` before forwarding the older spawn. Pager's live event highwater then discards the spawn. The reported parent and child session timelines confirm this mismatch; the canonical child lifecycle is intact.

## What Changes

- Stamp and persist the parent-owned subagent lifecycle projection after its observe hook has completed, then forward that projection before emitting its derived Goal refresh.
- Keep direct gateway fallback stamped when no parent actor can accept the lifecycle command.
- Let Pager reconcile a late lifecycle fact by child identity when another producer has already advanced its live Grow highwater; retain the higher cursor and reject duplicate or obsolete lifecycle facts.
- Pin live gateway order, shared durable/live event ID, and late lifecycle reconciliation with regression tests.

The existing Tasks pane consumes the lifecycle projection; no new panel or protocol field is needed.
