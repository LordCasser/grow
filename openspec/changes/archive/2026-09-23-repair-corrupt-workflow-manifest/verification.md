# Verification

- `openspec validate repair-corrupt-workflow-manifest --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed before archive (21 items) and on the final post-archive run (21 active items). An intermediate rerun briefly found a separate concurrent active change, `retain-session-search-index-on-incomplete-bootstrap`, with one incomplete task; that change completed before the final pass.
- `openspec validate --archived --no-interactive` — passed (376 archived items).
- `cargo test --locked --offline -p shell --lib corrupt -j 1 -- --test-threads=1` — passed (18 tests).
- `cargo test --locked --offline -p shell --lib workflow_restore_rebuilds_missing_and_invalid_manifests_from_timeline -j 1 -- --test-threads=1` — passed (1 test).
- `cargo test --locked --offline -p shell --lib corrupt_manifest_repair_replaces_only_the_observed_snapshot -j 1 -- --test-threads=1` — passed after the stale-snapshot assertion was strengthened (1 test).
- `cargo test --locked --offline -p shell --lib concurrent_workflow_manifest_writes_converge_on_highest_revision -j 1 -- --test-threads=1` — passed (1 test), covering the unchanged valid-manifest revision CAS path.
- `rustfmt --check --edition 2024 --config-path rustfmt.toml` on changed Rust files and `git diff --check` — passed.
