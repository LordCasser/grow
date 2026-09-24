# Verification

- `cargo check -p pager-minimal --lib`: passed after the owner resolver and handoff changes.
- `RUST_MIN_STACK=33554432 cargo test -p pager --lib app::root::dispatch::tests::transcript:: -- --test-threads=4`: 29 passed, including distinct child/parent content and cwd, a focus switch between slices, removed/rebound child identity, root reload restart, and existing root transcript behavior.
- `RUST_MIN_STACK=33554432 cargo test -p pager-minimal --lib full_view::tests -- --test-threads=4`: 8 passed, covering full-view serialization and owner cwd rendering.
- `rustfmt --check --edition 2024` on the touched Minimal production files and transcript tests: passed.
- `openspec validate --all --strict --no-interactive`: 19 passed before archive.
- `git diff --check`: passed before archive.

The focused Pager tests exercise captured ownership and handoff; `full_view::pump_transcript` is in the sibling renderer crate and was also type-checked. Existing ignored PTY transcript lifecycle tests remain a separate coverage item in the backlog.
