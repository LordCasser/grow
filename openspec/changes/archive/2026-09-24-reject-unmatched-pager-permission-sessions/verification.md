## Verification

- `cargo test --locked --lib -p pager unmatched_permission_during_startup_is_cancelled_without_root_fallback -- --nocapture` — passed (1 test).
- `cargo test --locked --lib -p pager session_id_none_race_window_routes_to_active_agent` — passed (1 test), preserving ordinary startup update fallback.
- `cargo test --locked --lib -p pager child_permission_queues_on_primary_and_approval_resolves` — passed (1 test), preserving exact child permission routing.
- `cargo fmt -p pager -- --check` — passed.
- `openspec validate --all --strict --no-interactive` — passed (17 items).

Cargo emitted the existing linker warning that the `__eh_frame` section exceeded the compact-unwind encoding size; all focused test commands completed successfully.
