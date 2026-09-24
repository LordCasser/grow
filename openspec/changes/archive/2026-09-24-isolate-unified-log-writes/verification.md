# Verification

- `cargo test --locked --offline -p diagnostics --lib unified_log::tests`: 27 passed. The blocked-writer regression fills the queue while the writer mutex is held, verifies producer return, then verifies a coalesced loss record when the worker resumes. The flush regression verifies its deadline when the queue is full.
- `cargo test --locked --offline -p diagnostics --lib`: 65 passed.
- `cargo test --locked --offline -p shell --test test_leader_stdio_integration unified_log_redirect_precedes_test_start`: passed.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate isolate-unified-log-writes --strict --no-interactive`: passed.
- The broader Shell test `test_sever_mid_rpc_orphans_response_and_replay_recovers` fails before any unified-log assertion: the first forwarded ACP frame is `grow/internal/queue_client_disconnected` (a notification with no `id`), while the test unconditionally unwraps `new_json["id"]` at `test_leader_stdio_integration.rs:3183`. This is a separate test fixture/protocol expectation, not evidence of a logging failure; no test was weakened here.

The remaining disk-capacity issue is tracked separately in `openspec/backlog.md`: the existing 5 MiB maintenance threshold is not a hard quota.
