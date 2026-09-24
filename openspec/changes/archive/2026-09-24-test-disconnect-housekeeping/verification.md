# Verification

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib driver_disconnect_transfers_not_evicts -- --nocapture` — passed (1 test).
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib evict_sessions_notification_on_disconnect -- --nocapture` — passed (1 test).
- The initial invocations used `--exact` without the full module path and ran zero tests; they were rerun without `--exact` as recorded above.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `openspec validate --all --strict --no-interactive` — passed (16 items).
- Archived as `2026-09-24-test-disconnect-housekeeping`; no specification delta was merged.
- Post-archive `openspec validate --all --strict --no-interactive` — passed (15 items).
- `openspec validate --archived --no-interactive` — passed (458 archived changes).
- No production behavior, protocol, or persistence code was changed.
