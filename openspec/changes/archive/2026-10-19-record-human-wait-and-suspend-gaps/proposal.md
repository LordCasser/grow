# Record human waits and suspended time

## Why

Timeline currently records tool and request boundaries but not the moment a human question or permission prompt is actually shown and resolved. Pending interaction notifications are process-local, so historical replay cannot distinguish a real user wait from tool execution. The new session-tree suspension lifecycle also records durable suspend and release boundaries, but activity projection does not subtract that offline interval from an open operation.

## What Changes

- Add validated typed Timeline open/resolve facts for question, permission, and Plan approval interactions. Facts carry the existing tool-call identity and a unique request-attempt identity; they never persist question, command, path, or permission payloads.
- Persist open immediately before the reverse request is sent and resolve it on every answer, rejection, cancellation, timeout, transport failure, or suspension path. A restored request uses a fresh attempt identity.
- Project only validated complete interaction intervals as human waits. An interrupted historical open remains unknown rather than being closed at the later process-recovery time.
- Treat a validated `TreeSuspended` to matching `ExecutionReleased` interval as offline time for activity projection. Keep recovery gaps distinct, and retain conservative protected activity when another operation overlaps the interval.

## Capabilities

### Modified Capabilities

- `session-timeline`: durable identity and time facts for human waits, and offline exclusion for same-operation session-tree suspension.

## Impact

Timeline schema/fold, Shell question and Plan approval dispatch, Workspace permission prompt callback, and transcript activity projection. Existing history remains unchanged and is not backfilled from tool spans.
