# Verification

- `cargo check --locked --offline --all-targets -p shell -p pager -p sampler` — passed.
- `cargo test --locked --offline -p shell --test test_grow_session_update` — passed (4 tests).
- `openspec validate --all --strict --no-interactive` — passed (17 items).

No behavior contract changed; this change only keeps a test exhaustive over the storage update enum.
