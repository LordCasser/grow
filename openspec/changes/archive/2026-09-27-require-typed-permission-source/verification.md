# Verification

- Production Shell `tool/preparation.rs` constructs `PermissionRequestSource::Child` or `Primary` explicitly. Test-only shorthands reject a partial child identity; `PermissionRequestContext` and `PermissionRequestSource` no longer have implicit Primary defaults.
- `cargo test --locked -p workspace -p shell --lib permission::manager::tests::concurrency_tests -- --test-threads=1`: 13 passed, including partial-identity and child-without-display-type tests.
- `cargo test --locked -p workspace --lib permission:: -- --test-threads=4`: 407 passed.
- Shell's inherited child permission-handle test passed; the external MCP permission persistence integration target compiled with the required explicit context and passed 17/17.
