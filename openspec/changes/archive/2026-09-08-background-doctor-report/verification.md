# Verification

## Implementation
The dispatcher now captures owned workspace, notification settings, fullscreen/Kitty/XTVersion evidence and DoctorFixTarget, then returns PrepareDoctor without collecting the report. The blocking worker collects once and handles report/list/fix planning from that report. Standalone CLI doctor and apply-fix behavior are unchanged.

Each AppView owns one Semaphore permit. Admission uses try_acquire_owned, duplicate requests receive an explicit busy notice, and the actual spawn_blocking closure owns the permit. Cancelling the async waiter cannot open another worker slot while the existing collector is still running. No unbounded pending queue was added.

DoctorFixPlanned is reused with a report_only marker, including on errors. Report results resolve current_doctor_target before delivery; removed/rebound origins cannot route old reports or errors into another agent. Existing initial-session binding promotion is preserved. Existing fix preview/question/confirmation and apply paths remain.

## Tests
Commands use CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 and locked/offline dependencies.

- `cargo test --locked --offline -p pager --lib doctor_ --quiet`: 48 passed, 7053 filtered out. Includes current-thread async tests that block a synthetic collector, observe runtime progress, cancel its waiter, prove the worker still owns the permit, release it, and verify recovery after panic. Existing doctor parsing/formatting/confirmation/target tests remain green.
- After adding removed-origin coverage and final formatting: `cargo test --locked --offline -p pager --lib app::root:: --quiet`: 1273 passed, 5829 filtered out, 2.04 seconds. Dispatcher regressions cover report/list/fix returning PrepareDoctor and rejecting duplicates; source tests cover hidden original session delivery, replacement, removed origins, successful reports and errors.

No real doctor repair or configuration mutation was performed. The blocking collector in new concurrency tests is synthetic; there was no PTY input responsiveness benchmark. The production call chain was checked to ensure collect occurs inside the same tested blocking executor. Existing macOS compact-unwind-size linker warning remains. No installed CLI was rebuilt or replaced.

## Limits
A native/filesystem probe that never returns still holds the single collector slot; cancellation is not claimed to stop blocking OS work. Tmux subprocesses retain their existing deadlines. Output byte budgets and startup diagnostics are separate scope. Existing fix cancellation notices retain their prior routing policy.
