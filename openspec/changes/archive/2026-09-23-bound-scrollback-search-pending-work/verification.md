# Verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p pager --lib scrollback::search:: -- --quiet`: 30 passed. This includes corpus/query coalescing, current-request result gating, and owner-drop worker exit.
- `rustfmt --check --edition 2024 --config skip_children=true crates/codegen/pager/src/scrollback/search.rs`: passed.
- `openspec validate bound-scrollback-search-pending-work --strict --no-interactive`: passed before archive.

The one-slot notification channel and one merged pending state bound queued requests by count. A scan in progress can still retain its current corpus and result buffer until it observes cancellation; this change does not establish a wall-clock worker-exit deadline. The remaining history/file-search worker lifecycle and UI response questions stay in backlog.
