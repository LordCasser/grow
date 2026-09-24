## Verification

- `rustfmt --edition 2024 crates/codegen/shell/src/extensions/skills.rs` completed.
- `cargo test --locked --offline -p shell --lib extensions::skills::tests:: -- --test-threads=1`: 24 passed, 0 failed. Controlled blocking-worker coverage verifies that timeout includes capacity wait, an uninterruptible worker retains its permit until exit, a later scan cannot start beyond the one-worker limit, waiter cancellation does not start a scan, worker panic is an error, and an empty successful result remains distinct.
- `openspec validate bound-workflow-list-execution --strict --no-interactive`: passed.
- Tokio 1.53.1 local runtime documentation confirms `spawn_blocking` work continues until return; ordinary runtime drop waits for spawned work, while `shutdown_timeout` may return and leave unfinished work running. The discovery worker retains the semaphore permit for that entire lifetime. This runtime shutdown policy was documented, not separately exercised by a shutdown integration test.

The registry itself is not forced to block in a filesystem integration test. Its production scan is passed directly to the same injected bounded-worker boundary exercised by the deterministic synchronization test.
