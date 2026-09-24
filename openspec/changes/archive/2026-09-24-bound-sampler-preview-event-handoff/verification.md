# Verification

- `cargo check --locked --offline -p sampler --tests -p shell` passed with `CARGO_INCREMENTAL=0`, debug symbols disabled, and two Cargo build jobs.
- `cargo test --locked --offline -p sampler --lib preview_fragment_budget` passed both stalled-drainer/order and cancellation tests. The fragment credit boundary test passed separately, and the complete Sampler library suite passed 246/246 tests.
- `cargo test --locked --offline -p shell --lib sampling_candidate_is_scoped_and_dropped_until_durable_admission` passed, preserving candidate metadata and discard delivery. This test drives the Shell handler directly; the Sampler tests cover credit ownership and queue behavior.
- `rustfmt --edition 2024` and `git diff --check` passed for changed code.
- The 64 MiB budget applies to queued L2 fragments. One fragment currently held by the Shell handler, provider raw evidence, canonical response state, terminal payload, and untagged persistence staging are separate allocations; the remaining end-to-end measurement and staging boundary stay in `openspec/backlog.md`.
