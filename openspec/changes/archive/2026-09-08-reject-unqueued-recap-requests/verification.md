# Verification

## Evidence
Actual extension handler checks the feature gate and session lookup, then sends SessionCommand::Recap. The fix uses the same internal-error mapping as grow/steer when the command receiver is closed. No extra acknowledgement protocol or actor lifecycle changes.

## Test scope
An MvpAgent fixture calls the actual recap handler for manual/automatic requests with live/closed receivers. Live requests must enqueue exactly one Recap with the matching flag; closed requests must return the internal channel error. Temp GROW_HOME and serial EnvGuard isolate configuration. The actor does not execute and no provider is contacted.

Existing pager recap_request_transport_failure tests assert manual feedback clears on RecapRequested.error; they were inspected, not newly executed in this shell-only run. The production effect already converts ACP errors into that result. No live terminal, Windows or installed CLI test.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib recap --quiet: 61 passed, 0 failed, 0 ignored; 0.21s; process exited 0. Existing macOS compact-unwind linker warning only. The later pager change is a comment-only correction (default OFF to ON), verified against resolve_session_recap.default(true); no pager binary rebuilt. git diff --check passed.
