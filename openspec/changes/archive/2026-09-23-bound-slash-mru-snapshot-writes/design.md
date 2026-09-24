## Context

See `proposal.md` for motivation. Loading checks the opened regular file and bounds actual reads to the existing `MAX_STORE_BYTES` allowance. Snapshot serialization occurs before the asynchronous handoff, and the worker writes the owned bytes through a same-directory temporary file before atomic replacement.

`SlashMru` owns the dirty bit. `SlashController::record_command_use` records the command and takes a snapshot, which clears dirty before handing bytes to `persist_async`; when that handoff fails the controller sets dirty again. After acceptance, the background worker owns the snapshot and retains the existing best-effort write behavior.

## Goals / Non-Goals

**Goals:** Keep persisted snapshot size within the same encoded-byte allowance used by the loader, reject before either queue admission or filesystem side effects, and keep the controller's existing dirty-state retry behavior.

**Non-Goals:** Bound the temporary memory used while serializing the snapshot, alter coalescing/worker lifecycle, or change the existing best-effort semantics after a snapshot has been accepted by the worker.

## Decisions

- Reuse `MAX_STORE_BYTES` as the sole write allowance. This ties writer and reader behavior to one constant instead of introducing a second policy value.
- Reject oversized snapshots in `persist_async` before initializing the process-wide writer. This makes the result observable to `SlashController::record_command_use`, which restores dirty state when handoff fails.
- Repeat the same size check at `MruSnapshot::write` before creating directories or temporary files. The publication boundary must reject independently so future/internal direct writer paths cannot publish unreadable files.
- Keep the existing atomic publication path unchanged for snapshots that fit. Add a test-only destination override to exercise the real controller entry without mutating process-global `GROW_HOME` or writing into a user's home.

## Risks / Trade-offs

- Serialization still allocates the complete encoded snapshot before the limit is checked → this change prevents oversized disk publication and queue retention but is not a serialization-memory budget.
- Once a valid snapshot is accepted, worker write failures remain best-effort and do not notify the controller → existing behavior is preserved; this change only covers rejection before handoff and the final publication guard.
- If the in-memory map remains over 1 MiB, each later command-use attempt will be rejected again and dirty remains set → this change does not evict entries by encoded size or guarantee that a future retry will fit.

## Migration Plan

No data migration is required. Existing oversized MRU files remain untouched; the existing loader continues to disable persistence for those stores. A later command use retries the current complete map, but it succeeds only if that map fits the allowance.
