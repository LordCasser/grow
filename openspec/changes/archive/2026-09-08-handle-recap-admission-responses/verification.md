# Verification

## Implementation
The effect decodes the actual ExtMethodResult envelope. Only ok=true with disabled absent/false is accepted; disabled=true, errors, missing results and malformed flag types populate RecapRequested.error. The existing dispatcher clears manual progress by session and leaves automatic errors silent. No global availability change or new protocol type.

## Test scope
The async test executes SendRecap with actual ACP channels, verifies outgoing sessionId/auto, returns responses via shell::extensions::to_ext_response, then dispatches TaskComplete into a local AppView. Six payloads cover accepted, explicitly enabled, disabled, rejected, malformed disabled and missing ok; each runs for manual and automatic requests. Existing manual progress is deliberately present for auto cases to prove auto failure does not clear it. A separate test rejects missing/null/array/error/partial-error envelopes.

## Limits
No live feature reload, provider call, real user session, installed CLI or Windows validation. These tests prove response handling, not a new cancellation/request-ID protocol. Overlapping recap requests and already in-flight notifications keep existing behavior.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib recap --quiet: 40 passed, 0 failed, 0 ignored; 0.10s; process exited 0. Includes existing recap dispatch/feedback regression tests. Existing macOS compact-unwind linker warning only. git diff --check passed.
