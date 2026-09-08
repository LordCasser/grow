# Verification (main, 2026-09-08)

All Cargo commands used --locked --offline -p shell --lib, CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2 and RUST_MIN_STACK=16777216.

- session::storage::search_recovery::tests: 7 passed, 0 failed (0.01s). Covers successful isolation, failure before any move, partial isolation invalidation, recreation failure, absent files, reprobe decisions and corruption classification.
- session::storage::search_fts::tests: 32 passed, 0 failed (0.05s), including existing corruption recovery and version handling.
- busy_eviction_retries_without_another_notification: 1 passed, 0 failed (6.44s). Existing real SQLite busy/retry regression retained.
- Existing macOS linker compact-unwind size warning remains; no test failures.

The two caller gates were inspected in source; not every OS fault was injected through both public entry points. This change does not make multifile quarantine atomic or provide coordination with arbitrary external SQLite writers. Tests used temporary databases; no user session/cache was repaired, migrated or deleted, and no installed binary was replaced.

Scoped rustfmt and git diff --check passed. Formatting also normalized the immediately preceding version-read regression tests in search_fts.rs; no behavior changed after tests. Strict all validation passed 17/17 before archive. cargo clean removed 7,372 files (2.7 GiB) after confirming no cargo/rustc process remained.

Post-archive strict validation: all 16/16, archived 249/249. Available disk after clean: 58 GiB.
