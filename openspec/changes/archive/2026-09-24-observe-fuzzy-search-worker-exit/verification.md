# Verification

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p workspace --lib file_system::fuzzy::tests::dropping_daemon_eventually_exits_worker -- --exact --nocapture`: 1 passed, 0 failed. The test observes the actual daemon worker exit after Drop in an idle temporary repository; it is not a bound for an in-flight syscall.
- `openspec validate observe-fuzzy-search-worker-exit --strict --no-interactive` and `git diff --check`: passed.

The matching historical evidence is [history worker exit](../archive/2026-09-24-measure-search-worker-exit/verification.md), [history cooperative stop](../archive/2026-09-23-stop-history-search-match-on-drop/verification.md), and [fuzzy file worker cancellation](../archive/2026-09-23-keep-fuzzy-file-search-off-ui-thread/verification.md). The remaining wall-clock behavior of already entered OS calls is not controllable by these in-process workers and is not an existing product promise; see the decision in `design.md`.
