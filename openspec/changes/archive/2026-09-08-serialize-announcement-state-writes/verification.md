# Verification

## Implementation
AppView retains only in-flight and pending flags alongside its existing canonical hidden-ID set. All hide/show/update-prune producers call the scheduler; only an idle scheduler emits a snapshot. Local completion clears in-flight, reports errors as before, and starts the latest snapshot once if pending. No separate queue or duplicated pending set. Event-loop inspection found one retained JoinSet and no abort_all/shutdown reset while the AppView continues.

## Cases
Action tests hold the first completion while executing100 hide/show pairs and final show, assert no overlapping effects and immediate UI state, then verify exactly one latest empty snapshot after success or failure. Completion then returns idle and allows another write. Actual announcement-update handling verifies prune is deferred into the same scheduling path. Existing per-ID hide test now observes the serialized second snapshot after completion. Existing exact-child-process persistence effect test still covers real IO without cached-home contamination.

## Limits
These controlled completion tests establish effect submission order rather than filesystem scheduler timing. The previously verified writer returns only after publication/IO failure. No new shutdown drain, automatic retry without newer state, cross-process merge or task-panic recovery. No real user data writes or live UI validation.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib announcements --quiet: 47 passed, 0 failed, 0 ignored; 0.05s; process exited0. First compile needed Effect import. A prior run had46 pass/1 fail because a second-hide test expected immediate persistence; updated it to require deferred full snapshot after completion. An initial ambiguous replacement did not apply and caused one unchanged rerun before the scoped edit. Final47 passed. Existing macOS compact-unwind linker warning only. git diff --check passed.
