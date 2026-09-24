# Verification

- `CARGO_INCREMENTAL=0 cargo check --locked --offline -p shell --lib`: passed.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p shell --lib agent_drop_aborts_owned_local_tasks -- --nocapture`: 1 passed.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p shell --lib supervisor -- --quiet`: 2 passed.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib leader_kill_reconnect_reloads_without_duplicating_history -- --ignored --nocapture`: the old SIGSEGV did not recur. The test now fails at `session/load` with `session ... already has an active writer`; the old in-process session actor survives aborting the leader's local task. This is a separate fixture/process-lifetime issue, retained in the backlog. A first run waited for replay and timed out because the test checked the load error only afterward; the failure check is now immediate.
- `openspec validate bind-agent-local-tasks-to-owner --strict --no-interactive` and `git diff --check`: passed before archive.

The isolated reconnect scenario is not counted as passing. The accepted scope here is the owner-local task lifetime and its direct regression, not full leader-process replacement semantics.
