# Design

The existing `runs` and `reconciliations` collections wrap only their own futures in `AssertUnwindSafe(...).catch_unwind()`. Apply the same local boundary to `output_reads` when a future is queued. The coordinator loop translates that panic outcome to `SubagentObservationError::Unavailable` and passes it through the existing `finish_output_read` path, which removes the keyed request and settles its deduplicated targets. No owner-wide catch or retry is added.

The output future retains its owned semaphore permit while it runs. If it unwinds, Rust drops the future and its captured permit before `CatchUnwind` yields the panic payload. The same permit slot can then admit a later read.

The regression injects a panic from `ChildRunner::load_persisted_output_async`, checks the query result, and uses a subsequent coordinator query/list response to prove the owner remains usable. A companion held-reader test continues to verify the 8-reader bound, ninth-request rejection, and permit reuse after settlement.

Build boundary: Rust test binaries unwind and exercise the isolated reader fault. Cargo release/release-dist retain panic=abort; catch_unwind cannot contain an abort, so production process abort is not relabeled Unavailable or tested as per-worker recovery. This change preserves that existing build policy and claims containment only for unwinds.
