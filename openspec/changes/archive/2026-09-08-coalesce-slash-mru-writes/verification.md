# Verification

## Evidence
The only production caller remains SlashController::record_command_use. It marks the shared store dirty if persist_async returns false. Snapshot writes retain unique temporary-file ownership and atomic publication. The new bounded channel carries only a wake token; pending holds at most one snapshot, and worker takes it before file IO.

Tests added:
- Submit 1,000 snapshots before a worker runs; submissions perform no file IO, only the latest snapshot remains, and the production worker loop publishes that exact JSON after sender disconnect.
- Take one snapshot as in-flight, submit middle/latest while it is held, publish the old snapshot, then run the production loop; final file equals latest. This deterministically models the interval before an in-flight write finishes; it does not simulate OS-level disk latency.
- Drop the receiver and submit; returns false, clears pending and leaves the existing file unchanged.

Existing MRU tests cover mark_dirty retry, normalization, ranking, isolated stores, bounded loading, concurrent atomic publication and failure cleanup.

## Limits
No process-exit flush guarantee, disk retry timer, cross-process merge, total byte budget for a snapshot, OS thread-spawn failure injection, or installed CLI/UI exercise. Submit uses a short mutex critical section without file IO; it is not lock-free. A background write failure remains best effort until another command sends a complete snapshot.

## Results
Low-disk environment: CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216.

cargo test --locked --offline -p pager --lib slash::mru --quiet:17 passed, 0 failed, 0 ignored,0.24s.

cargo test --locked --offline -p pager --lib slash:: --quiet: test result: ok. 377 passed; 0 failed; 0 ignored; 0 measured; 6759 filtered out; finished in 0.24s

Both processes exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
