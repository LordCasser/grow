## Verification

- `cargo test --locked --offline -p pager --lib coordination -- --test-threads=1`: 41 passed, 0 failed.
- `cargo fmt --check -p pager`: passed.
- `git diff --check`: passed.
- `openspec validate converge-passive-coordination-phases --strict --no-interactive`: passed.
- Archived as `2026-09-23-converge-passive-coordination-phases`; the accepted client-surfaces spec now includes the monotonic phase requirement.
- `openspec validate --all --strict --no-interactive`: 15 passed after archive.

The state tests inject approval before start, terminal before older live/replayed notices, and same inquiry ID across two peers. The handler test verifies that an out-of-order start cannot erase structured approval data. Existing coordination tests cover cursor/full reload, manual fold, and foreground tool independence.
