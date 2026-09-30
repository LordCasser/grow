# Verification

- `PAGER_BINARY="$PWD/target/debug/grow" CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_SPLIT_DEBUGINFO=off cargo test --locked -p pager --test pty_e2e_minimal minimal::minimal_new_session_keeps_history_and_resets::minimal_new_session_keeps_history_and_resets -- --ignored --exact --test-threads=1` — passed, 1 PTY test. The fixture asserts the first welcome card survives a completed 80-line turn and `/new` adds a second card while preserving prior output.
- `cargo fmt --all -- --check`, `openspec validate --all --strict --no-interactive`, and `git diff --check` — passed before this verification update.

The change only gates a pending card until the session ID is bound. The prior full core regression had 12,962 passing tests before this additional gate. All 20 selected release PTY cases passed after the change; the individual case logs are in `/tmp/grow-pty-*.log`.
