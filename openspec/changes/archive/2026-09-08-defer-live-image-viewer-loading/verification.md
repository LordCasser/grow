## Final results

- `cargo test --locked --offline -p pager --lib image_viewer --quiet`:4passed (0.10s). The new root test invokes the actual Enter handler for both source forms, asserts loading/empty buffers, takes the emitted effect, loads on a separate thread, verifies captured Kitty protocol converts JPEG to PNG, and applies completion. Memory precedence over a nonexistent file is checked. Reopening the same attachment assigns a new owner; an old Failed result cannot remove it.
- `cargo test --locked --offline -p pager --lib subagent_image_loading --quiet`:1passed (0.03s), retaining child-target loading/visibility/completion behavior.
- `cargo test --locked --offline -p pager-render --lib prompt_images --quiet`:162passed (0.07s). New tests verify deferred invalid-memory/missing-file sources, one-time request consumption, source-less rejection and unique owner assignment. Existing synchronous candidate R20 and deferred helper coverage remains.

The first test compile used nonexistent PromptWidget::clear; corrected to set_text. The initial JPEG fixture was3x2 and failed the existing8x8 image admission minimum before viewer code; corrected to16x12. The input minimum was not changed. Existing macOS compact-unwind linker warning appeared; final commands exited0.

Builds used disabled incremental/debug info, two jobs and RUST_MIN_STACK=16777216. No installed CLI replacement or live terminal interaction was performed. The test drives the real key-handler entry and worker loader with synthetic task completion; it is not a complete live event-loop test. File-byte budgets remain separate. No candidate feature was deleted.
