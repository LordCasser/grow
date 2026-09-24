## Why

Corrupt local draft records are moved out of the active namespace so they cannot be restored, but the quarantine directory currently grows without a bound. Repeated malformed records can therefore consume unbounded disk space.

## What Changes

- Bound retained quarantine files to 64 entries and 16 MiB total.
- Reclaim entries in a stable oldest-first order, using filename as a tie-breaker so directory enumeration order cannot change the result.
- Keep the existing quarantine trigger and local-draft recovery behavior; this change does not address path replacement races or slow filesystem deadlines.

## Capabilities

### Modified Capabilities

- `client-surfaces`: define bounded local-draft quarantine retention and deterministic reclamation.

## Impact

- Implementation and focused regression tests in `crates/codegen/pager/src/local_drafts.rs`.
- Update the client-surfaces contract and the input-routing developer architecture note.
- Resolve only the quarantine retention part of the existing local-draft backlog item; concurrent replacement and slow filesystem boundaries remain open.
