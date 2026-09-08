# Verification

## Before fix
`cargo test --locked --offline -p shell --lib last_owner --quiet`: 0 passed / 2 failed, 2.04s. The existing reaper joined the released actor but never sent Stop; Joined final-owner Drop also failed to send it. Ordinary clone Drop and the no-early-Stop period preceded these assertions.

## After fix
All commands use --locked --offline -p shell --lib and CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216.

- failed_session_init_join_tests: 9 passed, 0 failed, 0.06s. Includes both new regressions, thread panic and initialization cancellation tests, delayed persistence exit and resource-drop cases.
- cold_resume_preserves_durable_effort_chain: 1 parent passed, 0 failed, 2.35s; nine isolated child processes cover real MvpAgent cold load, resident reconnect, same-process close/load and effort continuity.
- stop_drains_accepted_updates_with_retained_sender: 1 passed, 0 failed, 0.05s.
- persistence_drain_timeout_retains_incarnation: 1 passed, 0 failed, 0.01s.

The new tests exercise the actual process reaper and destructor with retained weak sender routes. They verify protocol ordering, not a claim that Drop itself waits for storage release. The existing stop/drain test separately checks real temporary storage and lease release. No forced OS resource-exhaustion or reaper-thread startup failure was injected. No user session was modified and no installed binary replaced. The existing macOS linker compact-unwind warning remained.

Scoped git diff --check and pre-archive strict all validation (18 items) passed. After confirming no cargo/rustc process remained, cargo clean removed 7372 files / 2.7 GiB.
