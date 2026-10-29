## Design

Gate only the test whose expected panic comes directly from a Rust `debug_assert!` with `#[cfg(debug_assertions)]`. Other release failures remain visible; no module-level release exclusion or test filter is added.

The test continues to assert the debug-build invariant. No release-only test is needed because the invalid-anchor case violates the documented precondition of `insert_block_before`.

No archived behavior contract changes. `.openspec.yaml` sets `skip_specs: true`.
