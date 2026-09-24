# Verification

- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib session::load:: -- --quiet`: 60 passed.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib session_list_ -- --quiet`: 5 passed.
- The rapid-fetch regression delivers the newer result first, then the older result; the newer result remains. The modal-dismissal regression proves its late response cannot populate hidden welcome state.
- `openspec validate sequence-picker-list-fetches --strict --no-interactive` and `git diff --check`: passed before archive.

This change does not bind list requests to an exact Agent/child identity or freeze the Agent modal's cwd; those remain in the narrowed backlog item.
