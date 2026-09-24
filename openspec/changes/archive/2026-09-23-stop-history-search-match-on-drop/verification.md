# Verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p pager --lib 'views::history_search::tests' -- --quiet`: 16 passed. This includes the deterministic blocked-operation Drop test and existing one-slot/Stop precedence coverage.
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p pager --lib views::file_search::state::tests::reopening_after_clear_starts_with_fresh_interaction_state -- --quiet`: 1 passed against the workspace fuzzy worker changes.
- `git diff --check -- crates/codegen/pager/src/views/history_search.rs docs/architecture/pager-motion.md`: passed.
- `openspec validate stop-history-search-match-on-drop --strict --no-interactive`: passed.

The linker emitted a warning that the Pager test binary's `.eh_frame` section exceeded compact-unwind encoding capacity; linking and both test commands completed successfully. Cancellation is cooperative: a currently executing nucleo score, highlight-index calculation, result sort, or prior item-building step has no wall-clock interruption deadline. The stop flag prevents later scoring/highlight operations and prevents partial snapshot publication.
