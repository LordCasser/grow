# Verification

## Before fix
`cargo test --locked --offline -p shell --lib test_version_read_error_preserves_existing_index --quiet`: 0 passed / 1 failed, 0.05s. A real temporary index contained a BLOB schema-version value; opening did not return the required InvalidColumnType error because the query error was swallowed.

## After fix
All commands use --locked --offline -p shell --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216.

- session::storage::search_fts::tests: 32 passed, 0 failed, 0.06s. The new invalid-type test verifies exact BLOB preservation, retained document IDs and bootstrap marker. The missing-version-row test verifies normal stamping without document loss. Existing fresh/idempotent initialization, readable old/newer/malformed-text versions, corruption healing and FTS query tests remain green. GROW_SQLITE_JOURNAL_MODE was unset, so the local WAL test did not take its environment-override early return.
- busy_eviction_retries_without_another_notification: 1 passed, 0 failed, 6.41s. Real SQLite write-lock contention still propagates to the existing delayed retry and succeeds after release.

All database fixtures use temporary directories. No actual user cache/history was opened or modified, and no installed executable was replaced. Only the existing macOS compact-unwind linker warning appeared. Scoped git diff --check passed. The change propagates SQL/query errors generally through optional()?; the new injected error is SQL type decoding, not every possible OS I/O fault. Existing corruption recovery remains separately classified.

Pre-archive strict all validation passed (17 items). No cargo/rustc process remained before cargo clean, which removed 7372 files / 2.7 GiB.
