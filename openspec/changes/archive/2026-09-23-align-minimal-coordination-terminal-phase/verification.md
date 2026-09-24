## Verification

- `cargo test --locked --offline -p pager-minimal --lib coordination -- --test-threads=1`: 4 passed, covering Received, Approved, Terminal, idle and running primary turns, and print-once commit.
- `cargo test --locked --offline -p pager-minimal --lib -- --test-threads=1`: 94 passed.
- `cargo build --locked --offline -p cli --bin grow`: passed after replacing the removed boolean field access.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed.

The separate ignored PTY coverage change is still under investigation; its failing mock response does not affect the typed Minimal frontier unit tests or CLI compilation.

Archived as `2026-09-23-align-minimal-coordination-terminal-phase`; the accepted client-surfaces spec now includes the typed Minimal commit frontier.
