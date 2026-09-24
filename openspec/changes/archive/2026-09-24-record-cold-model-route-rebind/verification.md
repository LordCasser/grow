# Verification

- Reviewed the existing secret-free route key, exact model-change fold, summary projection, fresh ChatState initialization, cold writer lease, resident reconnect, and model switch commit.
- `cargo test --locked --offline -p shell --lib cold_resume_records_catalog_route_rebind`: passed. The subprocess test creates a fresh baseline, changes the endpoint across a restart, confirms one exact bridge, performs a later model control, restarts again without another bridge, and covers a valid route-free historical Timeline.
- `cargo test --locked --offline -p shell --lib cold_resume_preserves_durable_effort_chain`: passed.
- `cargo test --locked --offline -p shell --lib model_change_fold_rejects_transport_discontinuity`: passed; existing malformed chains remain rejected.
- `cargo test --locked --offline -p shell --lib same_model_catalog_reload_discards_signed_native_history`: passed.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed.

A broad Shell library run reached 1,479 of 3,921 tests before aborting on the default test-thread stack. Two unrelated leader-server tests also failed because they receive the new `grow/internal/queue_client_disconnected` housekeeping notification before their expected eviction result. The compaction case passed alone with `RUST_MIN_STACK=16777216`; the leader test expectations are being repaired in a separate test-maintenance change.
