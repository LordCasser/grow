# Verification

## Scope
Daemon submit assigns a checked monotonically increasing request generation. Coalesced pending stores the newest generation with merged Msg; worker publishes that generation unchanged. UI replaces the old completion-count tracker with requested_generation, clears results and hover on submission, and accepts only the requested generation once. A retained selection index continues to support same-query refresh navigation intent.

## Tests
Deterministic test stops the real worker and injects shared snapshots while using actual activate/deactivate/update_query/refresh_items/poll methods. It checks initial acceptance, reopening invalidation, late earlier-query rejection, exact current acceptance once, and refresh clearing selected text and mouse hover. This injection test is not an OS scheduling test; existing actual-worker history tests validate matching and item/query coalescing.

## Limits
No installed CLI exercise. In-flight matching still runs to completion; results are rejected at the UI boundary. D1 local draft source loading and pending deletion candidates remain untouched.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib history --quiet:49 passed,0 failed,1 ignored;0.09s; process exited0. Existing leader-cluster isolation test remains ignored; no new ignored tests. macOS compact-unwind linker warning only. git diff --check passed.
