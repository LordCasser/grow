# Verification

- `CARGO_BUILD_JOBS=4 cargo test --locked -p shell --lib planning_completion_tests -- --nocapture`: 3 passed. The new regression admits a real steer after a planning reminder, verifies the model receives the scoped control call and the same Turn samples again with the visible reply and its durable acknowledgement, then checks that the next actual final still returns `planning_completion_blocked`. Existing plain and structured final coverage still stops on unchanged blockers and permits completion after a legitimate release.
- On the same compiled shell libtest executable, filters `interjection_tests provider_completion`: 17 passed, including same-turn input delivery and the existing cancel, schema, provider stop, and Goal budget boundaries.
- Changed Rust files pass `rustfmt --check`, and `git diff --check` passes. The workspace-wide formatting check reports differences in untouched files.
- Strict OpenSpec validation passed before archive. No live provider call was required; the turn regression uses the real actor and a scripted Messages provider.
