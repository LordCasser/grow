## Behavior evidence

- `crates/codegen/pager/src/unified_log.rs` now has one worker for the first initialized ACP sender. Producers keep a bounded pending queue; a counting JSON writer rejects a log entry past 64 KiB without materializing its full encoding. The worker takes at most 16 entries / 256 KiB per batch, waits for that batch's ACP result, and publishes a monotonic settlement frontier before taking another.
- `flush_blocking` captures the accepted frontier and waits for all earlier batches, including a batch already out of the pending queue. It has one two-second local wait. A failed or timed-out send is settled locally without retry or a claim of remote persistence.
- Tests cover plain-thread producers, pre-init buffering and duplicate init, entry/count/byte budgets, FIFO across an unacknowledged earlier batch, concurrent flushes, held acknowledgements and a closed peer.

## Validation

- `cargo test --locked --offline -p pager --lib unified_log -- --test-threads=1`: 8 passed, 0 failed. macOS linker emitted its existing large `__eh_frame` warning; tests succeeded.
- `cargo fmt --check -p pager`: passed.
- `git diff --check`: passed.
- `openspec validate bound-pager-unified-log-forwarder --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 16 passed, 0 failed before archive.
