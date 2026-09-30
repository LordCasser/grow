# Verification

- `CARGO_BUILD_JOBS=4 cargo test --locked -p shell --lib session::workflow::registry::tests -- --test-threads=2` — passed, 12 Registry tests. This includes the new overlapping-root case, distinct-scope same-name behavior, folder trust, and symlink boundaries.
- The initial regression run with path-string comparison failed (`2` listings instead of `1`) because macOS resolved the temporary Git root through `/private/var` while the supplied user root used `/var`. The final implementation compares opened directory identities and the same test passes.
- `rustfmt --check --edition 2024 crates/codegen/shell/src/session/workflow/registry.rs` — passed.
- `git diff --check` — passed.
- `cargo fmt --all -- --check` — fails on an unrelated, pre-existing formatting difference in `crates/codegen/workspace/src/file_system/fuzzy.rs:910`; that file was not changed here.
- `openspec validate avoid-overlapping-workflow-discovery --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed before archive (15 items).
- `openspec archive avoid-overlapping-workflow-discovery --yes` — applied the workflow-execution delta and archived the change.
- `openspec validate --all --strict --no-interactive` — passed after archive (14 active specs).
- `openspec validate --archived --no-interactive` — passed after archive (551 changes).
- `cargo clean` — removed this test build's artifacts (11.2 GiB).

The list RPC shape and Workflow Run execution are unchanged. Registry discovery and resolution now share the same single User entry when the two directories overlap.
