# Verification

## Implementation
Only the SessionRecap notification arm changes: replay appends its historical block directly; live handling retains mark_recap_shown and apply_recap_block. Existing replay admission, cursor and late-auto gates are unchanged.

## Cases
Actual ACP handler routing runs replay/live × manual/auto combinations with established session, loading_replay for admitted history, eligible zero-threshold away tracker, and an existing manual status. It checks block append, manual status retention/clearing and automatic eligibility. No sleeps, provider calls or real session data.

## Limits
The app-global focus tracker still lets a live background-session recap affect the active session's away flag; this is separate debt. No request-ID/cancellation semantics or history dedup changes. No installed CLI, live terminal or Windows validation.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib recap --quiet: 41 passed, 0 failed, 0 ignored; 0.24s; process exited 0. Existing macOS compact-unwind linker warning only. git diff --check passed.
