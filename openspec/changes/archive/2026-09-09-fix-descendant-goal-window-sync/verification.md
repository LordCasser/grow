## Session evidence
Read-only inspection of session `01a081be-6168-7772-9e0c-dcc62a76b552` found:

- Timeline seq 3123: Control revision 37, Behavior Goal, Goal `01a081e1-153d-77a1-b6c5-c15839761b90` Active, no token budget, 41840 known tokens, incomplete usage recorded.
- Seq 3134: child `01a081e6-478f-78b2-b854-665a972e9820` spawned.
- Seq 3141: the next root provider attempt failed immediately with closed Goal / active Goal none.
- Seq 3148: the failure handler durably paused the Goal and selected Normal.

Incomplete usage alone does not close an unbudgeted Goal. `handle_request.rs` passes `is_subagent: true` and the cloned root GoalUsageWindow to startup; startup calls sync_goal_usage_window despite the child having an empty local tracker. The mismatch is in shared process state, preceding the durable failure pause.

No user session files were modified. Validation:

- Baseline regression failed before the production fix: descendant synchronization changed the expected root Goal ID to None (0 passed, 1 failed).
- After the fix, `cargo test --locked --offline -p shell --lib goal -- --test-threads=2`: 147 passed, 0 failed. Includes the new descendant test covering active admission for root and child, exhausted and incomplete-budget rejection, and inactive/stale Goal rejection.
- `cargo test --locked --offline -p shell --lib subagent -- --test-threads=2`: 317 passed, 0 failed.
- `cargo test --locked --offline -p shell --lib session::actor::spawn -- --test-threads=2`: 27 passed, 0 failed.
- Test filters can overlap; these counts are not a unique-test total.
- Build environment: macOS, incremental disabled, dev/test debug info disabled, two Cargo jobs, RUST_MIN_STACK=16777216. No live provider requests were made.
- Strict validation of the change and git diff whitespace checks passed before archive.

The published v2.1.5 release is immutable and predates this fix. The fix requires a new build. The existing session remains Paused in its original durable records; users can resume and explicitly restart its Goal after updating. No Timeline migration or automatic Goal restart is introduced.
