## Result

- `upsert_coordination_row` now accepts only a coordination identity with non-empty source peer and inquiry ID. Missing or empty identity returns `false` before mutation, preventing panic, key collisions and passive running-state creation.
- Incoming notices with malformed structured audit already used the raw-notice fallback. Empty `correlation_id` now uses the same fallback, preserving the original durable fact without correlating an inquiry row.
- Existing valid lifecycle behavior remains unchanged: `finish_all_running` excludes passive coordination rows, and the regression `coordination_foreground_cleanup_preserves_passive_timing` continues to pass. Typed lifecycle and generic finish-state design remain open.
- Backlog now retains only generic passive lifecycle mapping and notification ordering/replay/parent-child convergence risks.

## Validation

- `cargo test --locked -p pager --lib coordination_ -- --test-threads=1` — passed, 32 tests (including new missing/empty identity and empty correlation ID regressions, plus existing lifecycle/replay coverage).
- `openspec validate reject-invalid-passive-inquiry-identity --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed, 22/22 active items.
- `git diff --check` — passed.
- `openspec validate --archived --no-interactive` — passed, 416/416 archived changes.
