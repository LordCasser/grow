# Verification

- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p shell --lib agent_drop_requests_primary_and_child_shutdown -- --quiet`: 1 passed.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib leader_kill_reconnect_reloads_without_duplicating_history -- --ignored --nocapture`: 1 passed after correcting the fixture's lock path and cursor-reload expectation. The load succeeds, the previous history appears once, and the replacement serves a new turn.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib app::leader_cluster::scenarios -- --ignored --nocapture`: 4 passed, including reconnect, fan-out, reattach and two-client streaming.

The fixture waits for the same writer lock that storage uses before starting the next generation. Its exact-test subprocess keeps the global environment isolated from other Pager tests.
