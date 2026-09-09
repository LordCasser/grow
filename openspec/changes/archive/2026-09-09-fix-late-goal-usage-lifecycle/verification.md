# Verification

- Baseline: `late_unknown_usage_preserves_stopped_goal_status` failed: Complete became Paused (`/tmp/grow-goal-late-baseline.log`).
- Fixed: `cargo test --locked --offline -p shell --lib goal -- --test-threads=2`: 148 passed, zero failed. Includes all four stopped statuses, durable Control reread, settlement retirement, active budget enforcement, retiring-turn retry fencing, unbudgeted missing usage and admission restoration.
- No-default-budget trace: create_goal Option<i64> has no finite default; slash parser uses None without explicit --budget; implicit Goal admission passes None; GoalTracker preserves None; continuation renders unlimited; edit preserves a previous explicit budget. Existing unbudgeted regression records missing and known usage, restores state and verifies admission remains open.
- Environment: macOS; CARGO_INCREMENTAL=0, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216.
