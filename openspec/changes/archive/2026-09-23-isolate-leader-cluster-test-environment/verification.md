# Verification

- `CARGO_INCREMENTAL=0 cargo test -p pager --lib -- app::leader_cluster --ignored`: three scenarios passed in isolated exact-test child processes. `two_clients_share_session_and_stream_both_ways`, `n_client_fan_out_without_replay_duplication`, and `reattach_completion_roundtrips_durable_log` passed.
- The same command reported `leader_kill_reconnect_reloads_without_duplicating_history` as failed because its exact-test child exited before libtest could report a result. A direct run with `GROW_PAGER_LEADER_CLUSTER_CHILD=1`, a temporary startup `GROW_HOME`, and `RUST_BACKTRACE=1` confirmed process exit by SIGSEGV (signal 11), with no panic/backtrace. This is tracked separately in the backlog; the isolation change preserves the failure instead of treating a non-running child as success.
- `rustfmt --edition 2024` on both touched Rust files and `git diff --check` passed.
- `openspec validate isolate-leader-cluster-test-environment --strict --no-interactive` passed. Full strict validation also passed before the reconnect crash was added to the backlog; rerun after the backlog edit and archive.
