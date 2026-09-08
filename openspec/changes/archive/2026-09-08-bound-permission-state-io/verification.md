# Verification
- main; explicit temporary paths only, no real GROW_HOME writes or installed binary replacement.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p workspace --lib permission::state::tests`: 30 passed, 0 failed/ignored, 0.03s.
- Exact 1 MiB valid TOML loads; 1 MiB+1 source produces default despite permissive shared cache and stays unchanged. Oversized serialized write preserves existing bytes. Cursor proves actual limit+1 consumption independently of metadata. Unix FIFO without writer rejects within 2-second watchdog and remains FIFO; regular symlink reader succeeds. Existing directory/UTF-8/schema/NotFound/client mapping regressions pass.
- No Windows FIFO test, actual concurrent file growth scheduler, slow ordinary filesystem deadline, upstream serialization allocation or overall directory quota claim.
