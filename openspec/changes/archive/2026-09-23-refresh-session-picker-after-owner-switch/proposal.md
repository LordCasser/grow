# Change: Refresh a pending picker after switching owners

## Why

Each list request now belongs to an exact view binding. When two Agent or child modals are open, starting a fetch for one supersedes the other's global sequence. Its result is safely discarded, but switching back can leave that modal in a perpetual loading state.

## What Changes

- After dispatch, detect a visible picker still loading under a different current list binding and start a fresh request for it.
- Keep loaded modals intact; refresh only a pending picker whose old request no longer owns the global sequence.
- Verify Agent, child, and directory switches plus late responses.

## Impact

Pager picker lifecycle only. It reuses the existing sequence and binding, without a per-modal request registry.
