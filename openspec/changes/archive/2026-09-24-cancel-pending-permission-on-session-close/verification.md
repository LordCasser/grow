The focused Pager regression passed:

- `cargo test -p pager --lib closing_requester_cancels_pending_permission_without_authorizing -- --nocapture` — 1 passed.
- `openspec validate 2026-09-24-cancel-pending-permission-on-session-close --strict --no-interactive` — valid.
- `rustfmt --check crates/codegen/pager/src/app/root/dispatch/tests/permissions.rs` — the new test is formatted; the command still reports two pre-existing import-order differences elsewhere in this file, which were left untouched.

The test verifies session close sends `Cancelled`, then a stale enable-always-approve selection produces no effect and leaves the session mode unchanged. The linker emitted an existing compact-unwind size warning; the build and test completed successfully.
