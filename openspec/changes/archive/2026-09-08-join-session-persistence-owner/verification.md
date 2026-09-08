# Verification

## Red and intermediate evidence
- `finished_actor_waits_for_persistence_owner`: with task tracking attached but the old thread-only exit predicate retained, 0 passed / 1 failed (0.05s): actor exit exposed a replacement boundary while the simulated persistence task remained blocked.
- Passive completion alone passed local owner tests but made real MvpAgent close hit the existing 5s shutdown deadline. CoordinationBackendResource retains SessionHandle; workspace resource unbinding follows drain. Completion therefore also needs explicit receiver closure after actor-thread exit. No sleep/retry workaround was added.
- One test compile failed on an unqualified acp type; corrected to acp_transport::protocol. A broad `persistence` test command was interrupted during compilation before tests ran, then replaced with reviewed selectors.
- The stop-drain test initially expected two records. Existing adjacent-text merging produces one record; the final assertion checks the complete concatenated text, not a relaxed data-integrity requirement.

## Final checks
All cargo commands use --locked --offline -p shell --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216.

- failed_session_init_join_tests: 7 passed / 0 failed, 0.07s. Covers delayed persistence completion with a real temporary writer lease and later acquisition by a new adapter; failed-init waiting; task cancellation/panic resource drop; prior thread panic/join/cancel-waiter cases.
- persistence_drain_timeout_retains_incarnation: 1 passed / 0 failed, 0.03s. Actual MvpAgent drain, paused clock, blocked persistence task; deadline returns an error and leaves the incarnation registered; releasing the task permits a successful second drain.
- stop_drains_accepted_updates_with_retained_sender: 1 passed / 0 failed, 0.05s. Real SessionPersistence closes admission, flushes both accepted chunks (including one queued behind Stop), exits despite retained handle, rejects subsequent writes, and releases the file lease. Complete merged text is verified.
- cold_resume_preserves_durable_effort_chain: 1 parent passed / 0 failed, 2.19s (nine isolated child processes). Now additionally includes same-process close immediately followed by cold load. Prior effort/None/unsupported/reconnect cases remain green.
- session_lifecycle_gate_serializes_close_behind_load_incarnation: 1 passed, 0.03s.
- session_delete_tears_down_resident_actor_before_releasing_lifecycle: 1 passed, 0.05s.

Tests use explicit temporary storage roots or the existing isolated process fixture. No user session was modified, no installed binary replaced. macOS emitted the existing compact-unwind linker warning. Source review covers thread-creation failure's stop/wait path; actual OS thread exhaustion was not injected. Completion proves storage ownership release, not successful provider execution or a new guarantee that every prior persistence error was absent. Last-owner reaper/drop without an explicit lifecycle drain remains a separately scoped audit item.

Scoped git diff --check passed; pre-archive strict all validation: 18 passed. No cargo/rustc build remained before cargo clean, which removed 7372 files / 2.7 GiB.
