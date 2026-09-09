# Verification

2026-09-09, main, macOS arm64.

- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test -p shell -p pager --lib`: **10,927 passed, 0 failed, 13 ignored** (pager 7,158/0/10; shell 3,769/0/3). Build emitted the macOS large unwind-table linker warning; execution passed without stack overflow.
- `openspec validate --all --strict --no-interactive`: 18 passed before archive.
- `git diff --check`: passed.

Scenario evidence:
- Full input plus output: tracker arithmetic preserves cache hits and avoids adding reasoning twice; main-response accounting continues to increase when context pressure falls. Descendant mailbox test verifies all three categories reach the root acknowledgment boundary. Root sideband test verifies 100 input including 40 hits plus 20 output charges 120.
- Cache-only budget: `cache_only_usage_exhausts_budget_at_the_step_fence` verifies cached-only usage closes provider admission, preserves the current step boundary and transitions to BudgetLimited after StepEnded.
- Restore: classified counters survive JSON round-trip and accumulate; historical aggregate-only snapshots, including zero scalars, are marked incomplete. Unbudgeted usage remains admissible, while an exact budget is gated.
- Failed settlement and duplicate retry: `failed_goal_settlement_retains_attempt_and_retries_exactly_once` uses mixed cached/uncached/output usage, verifies counters roll back to zero, then confirms one durable total and matching categories after retry. Existing cancellation, incomplete-usage, ownership and sideband evidence regressions passed.
- Display: Goal detail tests assert total, cache-hit input, cache-miss input, output, budget basis and incomplete historical markers.

No live session, timeline or provider configuration was modified. Historical missing cached usage is not heuristically backfilled. Existing archived token audit describes the preceding accounting behavior and remains unchanged.

Post-archive verification: all active specs/changes 17 passed, archived changes 299 passed, zero failures. After confirming no Cargo/rustc process remained, `cargo clean` removed 8,252 files (3.9 GiB).
