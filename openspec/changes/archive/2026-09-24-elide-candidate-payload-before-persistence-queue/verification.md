# Verification

The actor producer fork sends `SamplingCandidate { request_id, attempt }` to the persistence FIFO without cloning the full ACP notification, while forwarding the original candidate to the live gateway. The persistence actor retains only the first marker's position and untagged notifications. A tagged generic Update is discarded defensively.

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p shell --lib session::persistence::durable_update_tests -- --test-threads=1`: 18 passed, 0 failed. The initial filter `session::persistence_tests` matched 0 tests; the exact module filter above exercised the suite.
- Same Cargo settings with `cargo test --locked --offline -p shell --lib sampling_candidate_payload_bypasses_persistence_queue -- --test-threads=1`: 1 passed, 0 failed, including the live gateway payload and compact persistence marker.
- `rustfmt --edition 2024 --check` on the three changed Rust files: passed.
- `git diff --check` on the changed Rust, documentation, and backlog files: passed.
- `openspec validate --all --strict --no-interactive`: passed before archive.

The linker reported the existing macOS `.eh_frame` compact-unwind size warning; the test binary linked and all focused tests ran. The general persistence channel remains unbounded, so this change makes no aggregate queue-size claim.
