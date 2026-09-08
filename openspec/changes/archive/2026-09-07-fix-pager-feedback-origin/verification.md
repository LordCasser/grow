# Verification

## Regression and implementation
The new `pager_feedback_stays_with_origin_after_view_switch` regression failed on the old current-view feedback helper: another session received old pager feedback. After binding PendingPager to the root AgentId, selected AgentId and session id, it passed.

All old pending path/ANSI consumers were located and migrated together. Normal dispatch constructs the request using its existing with_active_agent target; minimal completion explicitly supplies its build owner. Suspend retry moves the complete request. TempPath still owns cleanup on replacement, completion and errors. report checks the original session before mutating its feedback state.

## Tests
Commands use `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`, locked/offline dependencies.

- `cargo test --locked --offline -p pager --lib pager_ --quiet`: 16 passed. This covers child transcript capture, return to root, rebound/removed child, minimal build owner after a tab switch, removed root, and request retention across suspend retry. Earlier red/green test counts: 1 failed before, 1 passed after.
- `cargo test --locked --offline -p pager --lib app::root:: --quiet`: 1269 passed, 1 failed. The failure is `word_select_tip_retires_on_prompt_divergence_and_accepts_before` at the post-tick tip-retirement assertion. The same test failed when run alone. It is outside pager feedback code and has been recorded separately in backlog; this is not a fully green root suite.
- `cargo test --locked --offline -p pager-minimal --lib --quiet`: 86 passed, 0 failed (0.33 seconds).

## Limits
No real interactive pager/PTY launch was added to this change. The source-identity tests validate real request creation and state delivery; the minimal hidden/stale-source fallback uses the existing terminal insert_before API for one independent, width-limited notification line. It does not add that line to any session ScrollbackState. This path has been source reviewed, not visually verified in PTY. Existing macOS compact-unwind-size linker warning remains. No installed binary was changed.

## Disk
Started after cargo clean with about 76 GiB free. Build cache uses debug information disabled and incremental compilation disabled. Final measurement is recorded below.

Final disk check: target 5.2 GiB, filesystem 71 GiB available. Cache retained for the next isolated tip-retirement investigation rather than immediately recompiling these dependencies after another clean.
