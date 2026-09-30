# Verification

- `CARGO_BUILD_JOBS=4 cargo test --locked -p pager --lib ask_answer_tests -- --test-threads=2` — passed (1 test). The test feeds the active tool-result formatter into the Pager parser and expanded Ask block, covering freeform-only, selected option with notes, and selected option without notes.
- `rustfmt --check --edition 2024 crates/codegen/pager/src/scrollback/blocks/tool/other.rs` — passed.
- `git diff --check` — passed.
- `cargo fmt --all -- --check` — fails on an unrelated, pre-existing formatting difference in `crates/codegen/workspace/src/file_system/fuzzy.rs:910`; the file was not changed in this change.
- `openspec validate show-ask-freeform-answer --strict --no-interactive` — passed before implementation.
- `openspec validate --all --strict --no-interactive` — passed before archive (15 items).
- `openspec archive show-ask-freeform-answer --yes` — applied the client-surfaces delta and archived the change.
- `openspec validate --all --strict --no-interactive` — passed after archive (14 active specs).
- `openspec validate --archived --no-interactive` — passed after archive (550 changes).
- `cargo clean` — removed the test build artifacts (10.5 GiB).

The accepted-result formatter and stored tool-result body are unchanged. Live and replayed accepted Ask rows use the same Pager parser.
