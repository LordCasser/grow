# Verification

- `RUST_MIN_STACK=16777216 cargo test -p shell timeline_failure_preserves_row_and_recheck_indexes_repaired_content -- --test-threads=4` — passed; 1 focused unit test passed, 3,998 filtered out. Cargo emitted an existing linker warning that the `__eh_frame` section exceeded the compact-unwind encoding limit.
- `cargo fmt --all -- --check` — passed.
- `openspec validate retry-search-bootstrap-test --strict --no-interactive` — passed.
