## Why

Workflow restore correctly falls back to the Timeline-owned seed when `state.json` cannot be decoded, but its queued repair then fails on the same corrupt bytes and provides no durable acknowledgement. A session can therefore resume from memory while every restart encounters the corruption again.

## What Changes

- Repair a corrupt Workflow sidecar from its Timeline seed only when the file still matches the corrupt snapshot observed during restore.
- Await the existing persistence acknowledgement and surface repair failures before publishing the restored actor.
- Keep the revision compare-and-swap rules for valid manifests unchanged; a sidecar changed after restore must not be overwritten.

## Capabilities

### New Capabilities

### Modified Capabilities

- `workflow-execution`: require guarded, acknowledged repair when restore falls back to the Timeline seed because its sidecar is invalid.

## Impact

The Workflow JSONL restore path, Workflow persistence message and storage adapter, session actor initialization, and `workflow-execution` scenarios/tests are affected. No new dependency or manifest format is introduced.
