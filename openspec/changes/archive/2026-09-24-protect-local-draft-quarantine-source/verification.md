## Verification

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib local_drafts::tests -- --test-threads=1 --quiet` — passed: 23 tests, 0 failures. The linker emitted the existing compact-unwind size warning.
- `cargo fmt --all --check` — passed.
- `openspec validate --all --strict --no-interactive` — passed: 18 items, 0 failures.

The replacement regression holds the original corrupt file open, replaces its active path with a valid record, then invokes the production bounded-reader/quarantine path. It confirms the valid replacement remains loadable and is not quarantined. A separate collision test confirms the no-replace rename preserves both source and existing quarantine entry.

The final file-identity check and rename are separate OS operations; the narrow replacement window between them remains recorded in `openspec/backlog.md`. Windows was not tested.
