# Verification

- `concurrent_permission_like_attempts_settle_past_goal_budget_once` admits foreground and permission-like background work before the threshold, then confirms actual charges of 7 and 9 tokens, the 16-token total and exact breakdown, idempotent settlement, and refusal of new admission.
- `cargo test --locked -p workspace -p shell --lib concurrent_permission_like_attempts_settle_past_goal_budget_once -- --nocapture`: passed.
- The test expects settled usage, including cache-hit input only once, rather than treating a cache-hit counter as an extra charge.
- The final Shell `goal_support::tests` suite passed 21/21.
