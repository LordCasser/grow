# Verification (main, 2026-09-08)

Cargo commands used --locked --offline, CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0 and CARGO_BUILD_JOBS=2. Shell commands additionally used RUST_MIN_STACK=16777216 and --features test-support.

- Red workspace failed_historical_load_preserves_live_points_for_retry: 0/1, 0.00s; failed historical read still truncated the live point.
- workspace --lib session::file_state::tests: 30 passed, 0.01s. New regression verifies getter/max/truncate/merge all report failure; point 5 survives, and repairing the same pinned file allows retry to merge [0,5].
- shell --lib rewind_synthetic_turn_tests: 8 passed, 0.04s. New actor test injects unreadable history into the actual handle_rewind entry and verifies an error, unchanged conversation/prompt index, and no rewind intent file. Existing synthetic-turn behavior remains covered.
- shell --lib rewind_cross_compaction_tests: 8 passed, 0.23s.
- shell --no-run --test session_load_perf: passed with test-support enabled; compiled only, performance workload not executed.

Total executed selected tests: 46 passed. Shell lib-test compilation covers updated test call sites. Initial no-run invocation was rejected because session_load_perf requires test-support; the first enabled build then found three test Result adaptations and an unhandled cancel truncate Result. These were corrected, including cancellation's full-history preflight before its conversation rewind. Final builds have only the existing compact-unwind linker warning.

Pending rewind recovery and cancellation preflight were inspected in source; neither received its own runtime history-read fault injection. The cancellation terminal may already have committed before preflight; the failure stops its dependent internal rewind, not the completed cancellation terminal. Parser handling of malformed JSON, metadata-only fallback, read budgets and blocking-IO scheduling remain separate issues. Public workspace rewind_files now returns io::Result<FileRewindResponse>; no repository caller was found, and no compatibility wrapper was added.

All executed fixtures use temporary files or the existing actor test session directory guard. No real user session was repaired, no installed binary was replaced, and no performance workload was run. Changed-file git diff --check passed.

Strict validation: pre-archive 17/17, post-archive all 16/16, archived 254/254. After confirming no cargo/rustc processes remained, cargo clean removed 12,946 files / 6.5 GiB. Disk available afterward: 58 GiB.
