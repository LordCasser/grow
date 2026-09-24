# Verification

- `cargo check -p pager-render -p pager`: passed.
- `cargo test -p pager-render --lib viewer_ -- --test-threads=1 --quiet`: 9 passed, including aggregate rejection, retained/stale result release, and a 2048×2048 JPEG conversion whose peak reservation shrank to the exact retained Vec capacities.
- `cargo test -p pager-render --lib -- --test-threads=1 --quiet`: 1,017 passed, 1 ignored.
- `cargo test -p pager-render --lib concurrent_converted_viewers_release_memory_on_close_and_reopen -- --nocapture`: passed; two 2048×2048 JPEG conversions started together, both loaded under the shared allowance, and closing/reopening released and reused the reservation.
- `RUST_MIN_STACK=16777216 cargo test -p pager --lib image_viewer_real_prompt_entry_loads_owned_sources_and_rejects_old_results -- --nocapture`: passed; the actual Enter/background/owner-ID path still discards a stale result.
- Direct macOS test-binary measurement with `/usr/bin/time -l`, excluding Cargo/rustc: 2048×2048 JPEG single-viewer Rust fallback (PATH without `sips`) reached 44,482,560-byte maximum RSS; two concurrent conversions plus reopen reached 75,382,784 bytes. The normal `sips` path reached about 50 MB parent-process RSS; its child process is outside this parent RSS measurement. This is a representative peak, not a claim of a strict OS RSS ceiling.
- `RUST_MIN_STACK=16777216 cargo test -p pager --lib local_drafts::tests -- --test-threads=1 --quiet`: 21 passed, additionally verifying the adjacent quarantine change.
- `cargo fmt --all -- --check`, `git diff --check`, `openspec validate --all --strict --no-interactive`, and archived strict validation: passed after final changes.

The process account bounds viewer-owned encoded buffers and conservatively reserves in-process pixel/conversion workspace. Existing composer image ownership, image-library allocations beyond that estimate, and macOS `sips` child memory belong to separate image-pipeline limits tracked elsewhere in the backlog.
