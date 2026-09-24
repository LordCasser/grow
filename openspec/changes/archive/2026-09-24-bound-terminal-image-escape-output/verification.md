## Verification

- `cargo test --locked -p pager-render --lib terminal::image::tests`: passed, 26 tests. Covers exact-fit/overflow for Kitty and iTerm2 builders, frame aggregate append atomicity, existing Kitty chunk framing, and image overlay behavior.
- `CARGO_BUILD_JOBS=2 cargo test --locked -p pager --lib clear_escapes`: passed, 4 tests, including recursive child overflow retaining its ID.
- `CARGO_BUILD_JOBS=2 cargo test --locked -p pager --lib dashboard_stale_clears_share_limit_and_retain_overflow_state`: passed, 1 test. Dashboard agents share the budget and state is retained when the clear does not fit.
- `CARGO_BUILD_JOBS=2 cargo test --locked -p pager --lib inline_media_clear_state_is_removed_only_after_escape_append`: passed, 1 test; no-fit cleanup stays pending.
- `CARGO_BUILD_JOBS=2 cargo test --locked -p pager --lib failed_inline_media_escape_keeps_previously_placed_id_for_cleanup`: passed, 1 test; previously placed IDs survive a rejected upload for cleanup, while never-placed IDs are discarded.
- `CARGO_BUILD_JOBS=2 cargo check --locked -p pager --lib`: passed without warnings after test-only helper gating.
- `openspec validate --all --strict --no-interactive`: passed, 18 items.
- `git diff --check`: passed.

The contract bounds serialized Grow-owned upload buffers and each buffered inline-media draw/clear accumulator, including recursive old-ID Kitty clears and dashboard popup merging. A clear whose bytes do not fit stays pending with its state until a later buffer can append it. Direct stderr clear writes and unrelated notification escapes are separate output paths. This does not measure process RSS, bound transient allocator capacity, or claim any limit on terminal-process decoding or cache memory.

Follow-up: aggregate rejection also removes the speculative iTerm2 emitted-rectangle marker while retaining an already-placed image ID for cleanup. This ensures the next admitted escape contains a new placement instead of treating a rejected placement as delivered.

- `CARGO_BUILD_JOBS=2 cargo test --locked -p pager --lib aggregate_rejection_forces_iterm2_to_retry_placement`: passed, 1 test. The linker emitted a non-fatal `.eh_frame` compact-unwind warning.
- `CARGO_BUILD_JOBS=2 cargo check --locked -p pager --lib`: passed after the iTerm2 retry-state fix.
