# Verification

- `rustfmt --edition 2024 crates/codegen/shell/src/session/image_normalize.rs`: passed.
- `git diff --check -- crates/codegen/shell/src/session/image_normalize.rs docs/development.md openspec/backlog.md openspec/changes/bound-image-normalization-staging`: passed.
- `openspec validate bound-image-normalization-staging --strict --no-interactive`: passed.
- The first test attempt stopped before execution at an unrelated shared worktree compile error in `session/actor/updates.rs:1543` (`E0507`, moving `chunk.content` from a pattern guard). The root agent fixed that error; no change to that file was part of this change.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p shell --lib session::image_normalize::tests:: -- --test-threads=1`: 48 passed, 0 failed/ignored, 3879 filtered out. The linker emitted an `.eh_frame` compact-unwind warning, but the test binary completed successfully. The lowest observed free disk space during builds was 9.5 GiB.

The existing `canceled_waiter_retains_running_worker_admission` test covers permit retention after cancellation; source structure now sends the complete per-image pipeline through that same adapter.
