# Verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p workspace --lib file_system::fuzzy::tests -- --quiet`: 5 passed after the final query-ID assignment edit.
- Directly re-ran the built workspace test binary with `file_system::fuzzy:: --quiet`: 7 passed, including the Unix thread-exhaustion child fixture.
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p workspace --lib fuzzy_change_burst_streams_only_the_latest_query -- --quiet`: 1 passed after the final edit. After 100 rapid query changes, the status driver publishes the last query's result without publishing the earlier query.
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p pager --lib views::file_search::state::tests::reopening_after_clear_starts_with_fresh_interaction_state -- --quiet` was run by the concurrent history-search change against the shared Workspace edits: 1 passed.
- `rustfmt --check --edition 2024 --config skip_children=true crates/codegen/workspace/src/file_system/fuzzy.rs`, `git diff --check`, and strict change validation: passed.

The full-wake test parks notification consumption, then submits restarts and queries and drops the daemon within one second. It checks that only the latest restart/query remains pending and that stop/cancel are set. The real worker test verifies the latest query identity reaches its snapshot. A slow or blocked filesystem operation already running in the worker still has no wall-clock exit guarantee; Drop no longer waits for it on the calling thread.
