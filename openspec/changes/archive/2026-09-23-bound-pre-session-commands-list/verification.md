## Verification

- `rustfmt --edition 2024 crates/codegen/shell/src/extensions/skills.rs crates/codegen/shell/src/extensions/session_admin.rs crates/codegen/shell/src/agent/mvp_agent/tests.rs` completed.
- `cargo test --locked --offline -p shell --lib commands_list_keeps_chat_and_session_requests_off_pre_session_discovery -- --test-threads=1`: 1 passed. The extension handler test verifies that `kind="chat"` keeps precedence over `sessionId`, and that a non-chat `sessionId` request returns its missing-session error before cwd discovery.
- `cargo test --locked --offline -p shell --lib extensions::skills::tests:: -- --test-threads=1`: 24 passed. The shared worker tests exercise injected blocking, timeout including permit wait, retained permit after timeout, worker panic vs empty success, and cancellation before worker admission.
- `openspec validate bound-pre-session-commands-list --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 18 passed, 0 failed.
- `git diff --check` on the changed Rust, developer documentation, backlog, and change files: passed.

The injected worker test covers uninterruptible discovery behavior; this request-level change deliberately reuses that helper and does not add a second worker pool. Runtime shutdown was not separately exercised; as with the existing worker contract, shutdown follows Tokio's blocking-task policy.
