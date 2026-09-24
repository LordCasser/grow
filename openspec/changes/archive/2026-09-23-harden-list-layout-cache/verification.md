# Verification

- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib views::list_pane::layout::tests -- --nocapture`: 11 passed, 0 failed. Covers empty and populated out-of-range geometry plus FixedHeight and Variable append behavior.
- A broader `views::list_pane::` test run began after adapting all API callers, but compilation stopped in the concurrently edited `pager/src/views/file_search/state.rs` because `FuzzyMatcherStatus` is missing at lines 591 and 604. This failure is outside the list geometry files.
- `openspec validate harden-list-layout-cache --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 21 passed, 0 failed.
