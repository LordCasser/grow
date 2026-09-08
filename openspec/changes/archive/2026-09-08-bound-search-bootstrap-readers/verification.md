# Verification (main, 2026-09-08)

Cargo used --locked --offline -p shell --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2 and RUST_MIN_STACK=16777216.

- bootstrap_many_sessions_under_descriptor_limit: red 0/1 (3.06s), OS error 24; green 1/1 (2.94s). Parent verifies exactly one child test passed; child asserts isolated GROW_HOME before setup, builds 96 real sessions, drops writer adapters, lowers only its own descriptor limit to 64 and calls bootstrap_with_lease. The final completed marker and all 96 indexed IDs are asserted.
- scan_opened_sessions: 5 passed.
- list_sessions: 7 passed.
- cleanup_rechecks_activity_after_scan_under_writer_lease: 1 passed (0.26s).
- cleanup_ttl: 3 passed.
- test_launch_: 2 passed (1.02s).
- test_recheck_bootstrap_reruns_reindex_when_marker_missing: 1 passed (0.01s).
- auto_cleanup_reports_keys_for_search_eviction: 1 passed (0.64s).

Total selected tests: 21 passed, 0 failed. Existing compact-unwind linker warning remains. An intermediate compile failed on unupdated private scanner test call sites; those were updated. A subsequent compile was explicitly interrupted (exit 130) to finish the remaining multiline call-site update; no test ran in that interrupted build.

Scenario coverage: low-descriptor end-to-end regression proves bounded enumeration and reader admission together. Existing scanning/cleanup tests cover filtering and identity-sensitive deletion. Retention of the owned permit through a detached blocking read was checked in source: the blocking closure and outer task each own an Arc to the same OwnedSemaphorePermit. No deterministic slow filesystem/timeout injection was added, so that lifetime path is source-verified, not runtime-fault-injected. Failed reader admission continues to return via ? before completion-marker publication; no new reader-open failure fixture was added.

The limit is on admitted work, not a hard wall-clock cancellation guarantee: if all admitted blocking reads stall, new reader admission waits. Session summaries/ID sets and completed task bookkeeping can still scale with session count. Cleanup still deliberately retains identity-bound candidates; changing that cleanup strategy is outside this search bootstrap fix. No user sessions or installed binary were modified.

Scoped search.rs rustfmt and changed-file git diff --check passed. Strict validation before archive: 17/17; after archive: all 16/16, archived 250/250. With no cargo/rustc processes remaining, cargo clean removed 7,372 files / 2.7 GiB. Available disk after clean: 58 GiB.
