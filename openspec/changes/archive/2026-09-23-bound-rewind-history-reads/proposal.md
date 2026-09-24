# Bound and diagnose rewind history reads

## Why

The pinned rewind source is read synchronously inside async session requests. A very large record or ledger can monopolize the runtime thread and allocate without a stated limit. The picker metadata reader also ignores nested snapshot structure; a damaged record can therefore appear to have zero file changes or be silently omitted while the UI offers conversation-only rewind.

The current `Pinned rewind parsing rejects malformed records` contract explicitly calls for an in-memory-only picker fallback on metadata parse failure. That fallback makes a failed scan indistinguishable from a complete checkpoint list, so this change revises that scenario instead of treating the existing behavior as an implementation accident.

## What changes

- Apply explicit per-record and total-byte limits to pinned rewind history reads, rejecting over-budget input without merging partial history.
- Run pinned metadata and full-history scans on the blocking pool while preserving the source lock and retry ownership if the caller is cancelled.
- Validate the nested snapshot shape during metadata scans without materializing file content. Return scan errors through the rewind points request so the UI reports a failed read rather than confirmed absence of file changes.

## Capabilities

- `client-surfaces`: rewind picker accuracy, bounded read failure and interaction responsiveness.
