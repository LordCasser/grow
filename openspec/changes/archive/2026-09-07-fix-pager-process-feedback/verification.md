# Verification

## Source evidence
The old $PAGER branch discarded Command::status with let _. It now captures the result as the existing editor branch does. suspend_for_child restores terminal state before returning; after restore_after_child the event loop classifies pager_result and invokes the existing mode-dependent notice sink. Timeout and other suspend error branches return before process-result reporting. TempPath cleanup remains in place.

## Results
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib app::root::event_loop::tests --quiet: 77 passed, 0 failed.
- New tests execute a missing program inside a private test directory and assert NotFound plus visible error classification; fixed /bin/sh exit 0 and exit 7 commands validate success silence and failed-status feedback. A constructed Unix signal ExitStatus verifies signal status preservation without signaling any user process.
- Existing tests retain minimal system-block vs inline/fullscreen toast routing, handoff retry/dedup, and owned temporary transcript retry cleanup.
- git diff --check and OpenSpec pre-archive all strict validation passed (16 items).
- Existing macOS compact-unwind-size linker warning remains. target 13 GiB; available disk 64 GiB.

## Limits
No PTY test, real interactive pager, or post-change CLI relink was performed. Error classification and existing routing are tested; the after-restore ordering is verified directly from control flow. PAGER argument splitting remains whitespace-based. Request origin across pending view changes remains a separate audit item.
