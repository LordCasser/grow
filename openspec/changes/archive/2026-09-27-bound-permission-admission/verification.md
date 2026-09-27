# Verification

- Saturation regression holds 64 child prompts, verifies the next child fails immediately, the primary can still make a local decision, eight more primary prompts remain admissible, the ninth fails immediately, Reset progresses, and capacity becomes available after cancellation.
- A separate regression cancels a caller before the actor polls its command and checks the slot stays counted until the actor discards that command.
- Live classifier regression holds 16 distinct provider responses, checks a 17th child fails before provider dispatch with `classifier_source=overloaded` and `classifier_verdict=unavailable`, then releases the 16 and verifies all complete independently.
- `cargo test --locked -p workspace -p shell --lib permission::manager::tests::concurrency_tests -- --test-threads=1`: 13 passed.
- `cargo test --locked -p workspace -p shell --lib permission_auto_mode_tests -- --test-threads=1`: 22 passed, including the live Sideband saturation test and the 20-second provider delay case.
- Final linked binaries after the 72-request total cap: workspace permission 407 passed; Shell permission Auto 22 passed; Goal usage 21 passed.
- Changed Rust files pass targeted `rustfmt --check`; the repository-wide format check still reports only unrelated `workspace/src/file_system/fuzzy.rs`.
- After archive, strict current OpenSpec validation passed 15/15 and archived validation passed 538/538. `git diff --check` passed.
