# Verification

- `RUST_MIN_STACK=33554432 cargo test --locked -p shell --lib image_input_recovery_tests::disabled_memory -- --test-threads=1 --nocapture` — passed, 6 tests.
- `RUST_MIN_STACK=33554432 cargo test --locked -p shell --lib image_input_recovery_tests::promoted_image::promoted_background_compaction_recovers_1210_and_commits_a_fresh_text_transaction -- --nocapture` — passed, 1 test.
- `openspec validate fix-disabled-memory-flush-admission --strict --no-interactive` — passed.

The disabled-memory suite verifies durable Sideband terminal events at both boundaries: disabling after attempt ACK records `End(Cancelled)` without HTTP, and disabling after provider acceptance records `End(Completed)` while suppressing the memory write and storage resurrection.
