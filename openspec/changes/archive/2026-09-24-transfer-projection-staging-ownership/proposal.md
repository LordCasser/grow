## Why

`SessionPersistence::commit_response_projection` clones the full staged untagged-notification window before its durable append/retry loop. Large ordinary notification payloads therefore remain duplicated throughout projection commits even though the original window is already owned by the persistence actor.

## What Changes

- Temporarily transfer the matching staged notification window into the projection commit operation instead of cloning it.
- Restore the original window when the commit fails so retry and later reconciliation retain the same attempt, anchor, payloads, and FIFO order.
- Preserve exact-append idempotence and the ordering of untagged notifications around the canonical response projection.

## Capabilities

### Modified Capabilities
- `session-timeline`: failed projection commits preserve the staged untagged window for retry without cloning it.

## Impact

`crates/codegen/shell/src/session/persistence.rs`, its focused persistence tests, and the candidate-preview memory-bound backlog entry. No new dependencies or persisted format changes.
