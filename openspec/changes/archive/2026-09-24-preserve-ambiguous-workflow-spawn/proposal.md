# Preserve ambiguous Workflow spawns

## Why

`WorkflowManager::launch` currently clears the tracker and queues a sidecar tombstone whenever the durable `Workflow::Spawned` call returns an error. A lost or cancelled acknowledgment does not prove the event was absent: the writer may already have appended it. If the process exits before the queued tombstone lands, restore sees the Spawned fact and revives a run that the launch path treated as rolled back. A landed tombstone can conversely hide a durable Spawned fact from restore. There is no production operation that forgets an already admitted Workflow run; the tombstone is launch compensation only.

## What Changes

- Roll back the local run and sidecar only when the Timeline rejected Spawned before persistence. Preserve the run source on any failure whose commit status is uncertain, and report that uncertainty to the caller.
- Keep the existing no-Spawned rollback for preflight and journal failures.
- State the authoritative recovery behavior in the Workflow contract and correct the developer explanation. The proposed `Forgotten` fact is unnecessary without a committed-run forget operation.
