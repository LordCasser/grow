# Bound auxiliary Grow notification queues

## Why

Goal and Workflow notification senders bypass the session actor's preview delivery budget. They can enqueue full-state Grow snapshots into unbounded persistence and gateway channels while a sampling attempt is active. A subagent progress publisher can likewise enqueue a new transient snapshot every tick while an earlier gateway delivery is stalled.

## What Changes

Share the session's existing preview byte credits with Goal and Workflow senders. Reserve both persisted and gateway copies before enqueue, release each credit only after its consumer completes, and make a failed reservation or append invalidate an active sampling attempt. Let each subagent progress publisher have at most one outstanding gateway delivery. Keep canonical Goal and Workflow state persistence authoritative when an optional notification is skipped under backpressure.
