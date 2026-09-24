# Verification

- `cargo test --locked --offline -p shell --lib session::storage::search -j 1 -- --test-threads=1` — passed, 76 tests; 0 failed. This includes the real-root missing/malformed summary preservation and Recheck recovery tests, Timeline failure recovery, bootstrap marker race coverage, and the index-artifact scanner regression. The build emitted a linker compact-unwind warning but completed successfully.
- `rustfmt --edition 2024 --check crates/codegen/shell/src/session/storage/search.rs crates/codegen/shell/src/session/storage/search_fts.rs crates/codegen/shell/src/session/storage/jsonl/mod.rs crates/codegen/shell/src/session/storage/mod.rs` — passed.
- `git diff --check` — passed.
- `openspec validate retain-session-search-index-on-incomplete-bootstrap --strict --no-interactive` — passed before archive.
- `openspec validate --all --strict --no-interactive` — passed, 21/21 items.
- `openspec validate --archived --no-interactive` — passed after archive, 377/377 items.

The scanner test first confirms that a real session is discoverable while the SQLite index is present beside its cwd entry, then verifies that a missing or malformed summary in that opened session directory still makes strict search discovery fail. Ordinary session listing continues to skip those invalid candidates.
