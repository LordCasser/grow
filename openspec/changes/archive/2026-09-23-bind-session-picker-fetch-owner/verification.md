# Verification

- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib session::load:: -- --quiet`: 62 passed. The new cases cover Agent request cwd and rejecting a pending result after Agent or child focus changes.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib session_list_ -- --quiet`: 5 passed, including relaxed notices after Agent cwd changes.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib fetch_session_list_pushes_query_and_echoes_seq -- --quiet`: 1 passed; the wire request uses the effect's frozen cwd rather than the executor's app cwd.
- `CARGO_INCREMENTAL=0 cargo check --locked --offline -p pager --lib`: passed after the final guard/formatting edit.
- `openspec validate bind-session-picker-fetch-owner --strict --no-interactive`, scoped rustfmt check, and `git diff --check`: passed before archive.

One current list binding still serves all picker surfaces. Concurrent pending modals can therefore lose an older result and retain a loading state after focus switches; the narrowed backlog item tracks that separate lifecycle issue.
