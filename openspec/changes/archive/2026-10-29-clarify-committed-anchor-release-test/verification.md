## Verification

- Source review confirms the sole `should_panic` test is guarded by `#[cfg(debug_assertions)]`; no parent/module-wide test exclusion was added.
- The debug-assertion test passed in the root-coordinated `/tmp/pager-final3.log` run. The release build/test is still being run by the root task; it remains pending and this verification does not claim release coverage.
- `rustfmt --edition 2024 --check crates/codegen/pager/src/scrollback/state/mod.rs`: passed.
- `openspec validate clarify-committed-anchor-release-test --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed in the documentation closeout run (33 items).
- `git diff --check` for this change's source and OpenSpec files: passed.
