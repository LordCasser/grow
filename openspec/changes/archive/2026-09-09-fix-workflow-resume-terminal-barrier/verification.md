# Verification

- Baseline: `resume_waits_for_terminal_manifest_and_timeline_acknowledgment` failed with causal-fold error “already has an active execution” while terminal manifest acknowledgment was withheld (`/tmp/grow-workflow-resume-baseline.log`).
- Fixed: `cargo test --locked --offline -p shell --lib workflow -- --test-threads=2`: 188 passed, zero failed. Includes delayed durable terminal acknowledgment, terminal failure/closed channel without epoch advancement, pause/cancel, launch, restore, workspace and registry regressions.
- The delayed-manifest case runs an actual await_user/complete script; resume stays pending until terminal acknowledgment, then completes the next epoch.
- Environment: macOS; CARGO_INCREMENTAL=0, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216.
