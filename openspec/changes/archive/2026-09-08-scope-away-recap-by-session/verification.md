# Verification

## Implementation
FocusTracker's shown set and attempt map are keyed by SessionId and cleared on each focus loss. Existing terminal away timing and 90-second retry duration remain. The active-session helper is shared by pre-generation and focus-return logic, with focus-return querying before timer reset. Dispatch and notification paths pass their own actual session IDs.

## Tests
Existing focus/recap tests now supply explicit session identity. New tests verify independent attempts/results and fresh-away reset, an actual background SessionRecap notification does not consume foreground eligibility, and active-session helper behavior when switching between two sessions and Welcome. Existing replay isolation test still runs through the real handler with the corresponding session ID.

## Limits
No live terminal focus automation, provider calls, user session access or Windows run. Per-away state remains until next focus loss, including closed sessions. Late notifications spanning away periods retain existing behavior. Full FocusGained event-loop execution is not newly simulated; both entrypoints use the tested shared eligibility helper.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib recap --quiet: 44 passed, 0 failed, 0 ignored; 0.10s; process exited 0. First compile required an AgentId import in the new test module. The next run had 43 pass/1 fail because a background handler return was incorrectly asserted true; source returns changed && is_active. Corrected that assertion to false while retaining history and eligibility assertions; final run passed all 44. Existing macOS compact-unwind linker warning only. git diff --check passed.
