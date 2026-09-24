# Verification

- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib session::load:: -- --quiet`: 63 passed after adding the refresh behavior. The new two-modal case checks that a stale failure for Agent A causes a request for pending Agent B, does not toast B, and that switching back to A refetches its own cwd. Existing owner-switch coverage now checks the child picker also refetches.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib app::root::dispatch::tests -- --quiet`: 930 passed, covering the shared dispatch wrapper.
- `openspec validate refresh-session-picker-after-owner-switch --strict --no-interactive`, scoped rustfmt, and `git diff --check`: passed before archive.

The list handlers intentionally apply entries/loading/toast directly and return no follow-up effects: the list result has no required asynchronous continuation. The dispatch wrapper adds a fetch effect only when the newly visible picker is still loading under a superseded binding.
