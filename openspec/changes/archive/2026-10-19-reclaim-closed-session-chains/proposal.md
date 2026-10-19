## Why

Session TTL currently deletes stale sessions one at a time. A parent Timeline may still reference a child result after the child directory is removed, or a child seed may lose its parent source after the parent is removed. The remaining session then fails closed forever because its recovery source is missing. Explicit single-session deletion can create the same broken link.

## What Changes

- Make automatic TTL reclaim a linked parent/child component only when every member is independently stale, inactive, and its complete result and delivery chain validates under all member writer leases.
- Persist the already-decided component reclaim before removing any member, then finish that exact reclaim after restart if deletion was interrupted.
- Reject explicit deletion of one session while its Timeline participates in a cross-session parent/child result chain, with a reason that identifies the linked session and explains that automatic cleanup waits until all linked sessions are eligible.
- Preserve fail-closed behavior for missing or invalid sources, live writers, fresh members, unresolved receipts, and incomplete scans.

## Capabilities

### Modified Capabilities

- `session-timeline`: linked recovery evidence is reclaimed as one owned component, and an individual linked session cannot be explicitly deleted.

## Impact

Only JSONL session retention/deletion and its focused tests change. The change reuses existing bounded discovery, Timeline validators, writer leases, and per-session quarantine. It adds one small durable reclaim-intent record because sequential per-session quarantine cannot survive a crash without splitting the component. No UI or cross-store transaction protocol is added.
