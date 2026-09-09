# Verification

Actual handle_prompt regression failed for a non-Goal child with an active shared Goal window: turn_boundary_persistence_failed / InvalidTurnIdentity. Existing matching and mismatched delegated Goal cases run alongside this ordinary Task case; fixture now stamps delegated_goal exactly as production.

Baseline log: `/tmp/grow-task-owner-baseline.log`.

Validation environment: macOS, CARGO_INCREMENTAL=0, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216. User sessions and provider endpoints are untouched.

Full Shell library regression: `cargo test --locked --offline -p shell --lib -- --test-threads=2`: **3,762 passed, zero failed, 3 ignored**. Both strengthened integration cases passed. Log: `/tmp/grow-owner-propagation-fixed.log`.

`git diff --check` and strict all-spec validation passed.
