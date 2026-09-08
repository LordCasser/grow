# Verification

## Before fix
`cargo test --locked --offline -p shell --lib busy_eviction_retries_without_another_notification --quiet`: 0 passed / 1 failed, 5.52s. A real temporary SQLite index had a row for a missing session; another connection held BEGIN IMMEDIATE. After the driver's busy timeout, flush_ready had removed the failed eviction key. The lock was released before the assertion; no user data was involved.

## After fix
All tests use --locked --offline -p shell --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216.

- busy_eviction_retries_without_another_notification: 1 passed, 0 failed, 6.35s. The same real lock failure leaves a delayed key and the row intact. An early flush does not retry; after lock release and the pending deadline, flush_ready succeeds and removes both the row and pending work without enqueueing another notification.
- search_contention_keeps_sqlite_error_identity: 1 passed, 0 failed, 0.00s. Busy/Locked retry; corrupt/I/O SQLite errors, unrelated filesystem WouldBlock and a string saying database is locked do not.
- search_pending_terminal_errors_and_missing_index_do_not_retry: 1 passed, 0 failed, 0.05s. A malformed temporary summary fails as InvalidData; it and a successfully missing/no-index key leave no retry pending. No index is created.
- auto_cleanup_reports_keys_for_search_eviction: 1 passed, 0 failed, 0.65s. Actual queue/worker deletion regression remains green.

The contention regression drives real flush_ready at the stored deadlines; the existing queue regression checks worker dispatch separately. Source inspection confirms run_worker wakes on the minimum pending deadline. No persistent retry/outbox guarantee is introduced: process/runtime shutdown can still discard in-memory work. Corruption healing, bootstrap claims and SQLite's own busy timeout remain unchanged. No user history, installed binary or external service was modified. Only the existing macOS compact-unwind linker warning appeared.

Scoped git diff --check and strict all validation (17 items) passed. No cargo/rustc process remained before cargo clean, which removed 7372 files / 2.7 GiB.
