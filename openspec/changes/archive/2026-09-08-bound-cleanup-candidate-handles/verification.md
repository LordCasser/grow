# Verification (main, 2026-09-08)

Cargo flags: --locked --offline -p shell --lib; CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216.

- Red cleanup_many_sessions_under_descriptor_limit: 0 passed / 1 failed (4.21s). In a child process with 96 real expired sessions and RLIMIT_NOFILE=64, old cleanup returned (49, 0), leaving 47 eligible sessions unprocessed without reporting errors.
- Final session::storage::jsonl::tests::cleanup_: 6 passed (5.06s), including the same low-descriptor child regression. The child asserts all 96 deleted callbacks, empty session listing and no caller maintenance cache/lease residue.
- cleanup_preserves_callers_live_writer: part of the six; caller's writer lease is retained, independent adapter cannot acquire it until writer drops.
- cleanup_duplicate_discovery_preserves_all_candidates: part of the six; duplicate IDs across two CWDs reject before deletion or callback, all three directories remain.
- cleanup_refresh_after_discovery_releases_maintenance_leases: part of the six; first deletion callback refreshes the remaining sessions, cleanup returns (1, 0), both are preserved and independent writers immediately acquire their leases.
- Existing cleanup activity recheck test: part of the six, now also checks hiding after scan, alongside refreshed/future/corrupt/identity/expired/held/skip cases.
- auto_cleanup_reports_keys_for_search_eviction: 1 passed (0.66s); actual deletion identity still drives search eviction.

Total distinct selected tests: 7 passed. Existing compact-unwind linker warning remains. Tests use explicit temporary roots; resource-limit changes occur only in the isolated child, whose GROW_HOME is asserted before fixture writes. No real user sessions, installed binary or parent process limits were changed.

Discovery keeps its existing policy of skipping some directory/summary errors; this fix removes the deterministic O(session count) descriptor pressure but does not claim exhaustive reporting for arbitrary scan IO failures. Full discovery still precedes deletion. Candidate identity is pinned at admission, then eligibility is checked on that same entity under its operation lease; discovery summaries alone do not authorize deletion. Native Windows execution was not performed.

Changed-file git diff --check passed. Strict validation: pre-archive 17/17, post-archive all 16/16, archived 251/251. cargo clean removed 7,372 files / 2.7 GiB after owned tests completed and no cargo/rustc processes remained. Disk available after clean: 58 GiB.
