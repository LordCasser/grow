# Verification

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p shell --lib session::persistence::durable_update_tests:: -- --test-threads=1`: 19 passed, 0 failed, 3909 filtered out. This includes failure restoration, exact-key retry, and FIFO projection order.
- `rustfmt --edition 2024` on the two changed Rust files: passed.
- `openspec validate transfer-projection-staging-ownership --strict --no-interactive`: passed.
- `git diff --check` on the changed Rust, docs, backlog, and OpenSpec files: passed.

The linker emitted the macOS `.eh_frame` compact-unwind size warning; the test binary linked and all focused tests ran. The persistence channel remains unbounded for other messages and payloads; this change only removes the projection-time full notification-window clone.
