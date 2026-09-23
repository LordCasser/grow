# Verification

## Completed checks

- `cargo test --locked -p chat-state admitted_response_view -- --test-threads=1` — passed, 4 tests. Covers healthy/identity-bearing ownership, quarantine integrity repair, compaction preservation, rewind retaining an earlier response, rewind removing later responses, compaction-prefix rewind provenance, and legacy identity-less exclusion.
- `cargo test --locked -p shell --lib response_projection -- --test-threads=1` — passed, 4 tests. Covers deterministic reasoning/text projection without fabricated event IDs, quarantine Discarded with zero raw preview, exact/conflicting record comparison, and the typed fatal `response-projection` marker.
- `cargo check --locked -p shell -p chat-state` — passed.
- Changed-file `rustfmt` — completed. Rustfmt recursively touched sibling modules; those unrelated formatting-only changes were restored before handoff.
- `git diff --check` — passed.
- `openspec validate reconcile-response-replay-projection --strict --no-interactive` — passed.

## Additional coverage added or confirmed

- `shell/src/session/storage/jsonl/durable_tests.rs` covers a committed
  post-append bookkeeping error, exact projection idempotence/conflict, and
  file/directory sync failures that leave a physical row and are reconciled by
  retry without duplication.
- `shell/src/session/persistence_tests.rs` covers projection append failure at
  the projection anchor and after the anchor, preserving independent rows and
  removing candidate rows on retry.
- `shell/src/session/actor/tests/truncation_recovery_tests.rs::response_projection_fatal_boundary_blocks_tool_dispatch_and_recovery`
  drives the real turn loop and asserts one provider request, no Accepted
  boundary, and no completed tool event after a projection fatal.
- `shell/src/session/actor/tests/truncation_recovery_tests.rs::response_projection_physical_ack_loss_blocks_tool_dispatch_and_recovery`
  routes the projection barrier through a real JSONL `StorageAdapter`, drops
  its acknowledgement after the physical commit, and asserts exactly one
  projection record with no recovery, Accepted boundary, or tool completion.
- `shell/src/agent/mvp_agent/tests.rs::resident_response_snapshot_delivers_future_projection_once_before_tools`
  covers cold/resident snapshot states and cursors before/after the projection;
  it also asserts that replay reconciliation leaves the updates ledger bytes
  unchanged.

Final current-tree validation on 2026-09-23 ran the new ledger-integrity and
physical ACK-loss assertions as part of the full Shell library suite:

- `cargo test --locked -p chat-state --lib -- --test-threads=4`: 516 passed, 1 ignored.
- `cargo test --locked -p shell --lib -- --test-threads=4`: 3,882 passed, 3 ignored. The ACK-loss turn test, resident snapshot/cursor test, and named SSE turn test all passed.
- `cargo test --locked -p pager --lib reconnect -- --test-threads=4`: 70 passed, 2 ignored.
- `cargo test --locked -p pager --lib replay -- --test-threads=4`: 87 passed, 2 ignored.
- `rustfmt --check` passed for the newly edited `support.rs` and `truncation_recovery_tests.rs`; the changed assertion in `mvp_agent/tests.rs` was inspected in place. Whole-file rustfmt check of the latter reports existing unrelated formatting differences, so it was not reformatted wholesale.
- `git diff --check`, `openspec validate reconcile-response-replay-projection --strict --no-interactive`, and `openspec validate --all --strict --no-interactive` passed (18/18 before archive).
- All Rust builds for this task used an isolated `/tmp/grow-open-changes-target` (8.8 GiB before cleanup). `cargo clean --target-dir /tmp/grow-open-changes-target` removed 8.7 GiB/12,367 files; filesystem available space rose from 14 GiB to 22 GiB. No shared workspace target was deleted.

## Implemented production paths

- Timeline-owned selected-branch response provenance, independent of display-leaf Surface IDs.
- Event-FIFO live projection barrier and typed fatal boundary.
- Exact eventId-keyed append for independent ACP rows; missing IDs and payload conflicts fail closed.
- Shared projection scan/validation/synthesis core used by typed/direct replay, streaming child replay, raw MvpAgent replay, ordinary typed load, and cold replacement-writer repair.
- Missing `updates.jsonl` is treated as an empty cache and can synthesize a safe trailing response in memory.
- Projection records remain id-less; cursor replay consequently uses the existing full-replay safety fallback.

## Verification limits

The committed-error case is covered at the JSONL storage boundary and the
turn-level physical test covers ACK loss; no single test combines a
post-append committed bookkeeping error with the turn actor's no-recovery
assertion. The turn fault test routes through a real `JsonlStorageAdapter`
but does not start a full `SessionPersistence::new` actor. Export calls
`load_updates_for_replay`, which shares the read-only reconciliation core;
the storage and child replay tests exercise that core. The export process was
not separately driven with provider/tool counters. Source inspection of
`pager/src/export_cmd.rs` and `storage/mod.rs` confirms that the read-only
reader has no provider or tool dispatch capability. These are validation
limits, not additional authorities or accepted history paths.
