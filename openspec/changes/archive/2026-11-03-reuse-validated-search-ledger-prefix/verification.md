# Verification record

## Executed

- `cargo test --locked -p shell --lib session::storage::search::tests::timeline_projection_matches_full_across_append_rewrite_truncate_and_torn_tail -- --exact --nocapture` passed before the final strengthening that reads a fresh snapshot containing a partial first suffix record and restores the original mtime on the same-size rewrite.
- `cargo test --locked -p shell --lib session::storage::search::tests::timeline_projection_preserves_full_reader_error_order_and_messages -- --exact --nocapture` passed. It compares the legacy full collector and projection errors by `ErrorKind` and exact display text for checksum, version, valid-checksum sequence gap, lifecycle-only invalid suffix, and a later physical syntax error after an earlier invalid lifecycle event.
- `cargo test --locked -p shell --lib session::storage::search::tests::worker_projection_cache_alternates_keys_and_drops_failures_until_retry -- --exact --nocapture` passed. It covers single-entry key alternation, malformed body read preserving the SQLite row and forcing a cold retry, and SQLite trigger rejection preserving the row and withholding the failed candidate until retry.
- `cargo test --locked -p shell --lib session::storage::search::tests::search_projection_timing_probe -- --ignored --exact --nocapture --test-threads=1` passed. Raw samples and conditions are in [`measurement.md`](measurement.md).
- `cargo test --locked -p shell --lib session::storage::search::tests::search_projection_retained_allocation_probe -- --ignored --exact --nocapture --test-threads=1` passed with the temporary allocator in `shell/src/lib.rs`. The parent restored lib.rs to its exact pre-probe SHA256 after the run; details and limitations are in [`measurement.md`](measurement.md).

## Previously pending final test edits (completed below)

The append fixture was strengthened after its green run to create a fresh reader while the first new physical record is torn, assert its `complete_len` remains at the old complete prefix, then finish that record and resume from the torn snapshot. The same-size rewrite also now restores and asserts the original modification time. A mixed physical suffix test was added for presentation-only identity state followed by a legal domain event, and a duplicate presentation identity. These final test-only edits were formatted and diff-checked but not run because the parent requested removal of both temporary probes and immediate allocator cleanup before any more Cargo invocation. The final suites and exact rerun below completed this verification before archive.

The broad `cargo test ... projection` substring command was interrupted after it selected 118 unrelated tests; it is not counted as a completed verification run.

## Final complete suites

The parent restored the temporary allocator to the exact pre-probe lib.rs bytes/SHA256 before running:

```sh
GROW_HOME=<fresh mktemp directory> RUST_MIN_STACK=16777216 cargo test --locked -p chat-state -p shell --lib -- --test-threads=4
```

`/tmp/grow-search-prefix-final-full.txt`: ChatState **604 passed, 0 failed, 3 existing ignored** in18.87s; Shell **4485 passed, 0 failed, 9 existing ignored** in235.62s. This run includes the strengthened fresh-torn-snapshot/mtime and mixed-presentation tests, and all JSONL/bounded-reader/writer/restore/search regressions. The temporary benchmark and allocator tests are absent from the normal build. While the suites ran, the existing error-order test gained a final same-Control dual-invalid assertion at independent review's request; its exact final rerun is recorded separately below, rather than claimed as part of the original full-suite executable.

The independent review found and verified fixes for moving full text extraction back into the original blocking closure and releasing an invalid old candidate before rebuilding its replacement. It verified the common physical scanner, replay-only error priority, complete-prefix hash, failed candidate ownership and successful SQLite publication. The cache holds no file handle, skips no newly appended record validation, and does not claim Summary/body transaction consistency beyond the preexisting upsert boundary. See[review](review.md).

Final exact rerun after the additional dual-invalid Control assertion: same isolated env and `cargo test --locked -p shell --lib session::storage::search::tests::timeline_projection_preserves_full_reader_error_order_and_messages -- --exact --nocapture --test-threads=1` passed1/1 in0.01s; `/tmp/grow-search-prefix-order-final.txt`. Targeted strict validation and git diff --check passed.

Before archive: `openspec validate --all --strict --no-interactive`21/21 passed. After archive: `openspec validate --archived --strict --no-interactive`793/793 passed, all tasks complete.
