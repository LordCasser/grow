# Verification

- `reset_ack_waits_for_saved_state_and_reports_write_failure` confirms a successful reset removes an old grant from disk, an invalid target path returns an I/O error, and both paths keep the old grant revoked in memory.
- `cargo test --locked -p workspace -p shell --lib permission::manager::tests::concurrency_tests -- --test-threads=1`: 13 passed.
- Shell's `ResetPermissionState` error branch sends an existing UI-only `UiNotice` with Error tone; the notice channel is separate from model context. No new model message path was added.
