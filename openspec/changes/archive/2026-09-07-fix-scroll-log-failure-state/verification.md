# Verification

## Source evidence
ScrollLogRecorder already changes its sink to Disabled on open/write failure. MouseScrollState previously queried only recorder.is_some, so that failed object still appeared enabled and the next toggle merely removed it. Both state query and toggle now use is_active: Pending/Open true, Disabled false. No transition or delivered-line logic changed.

## Regression scope
The new test points a recorder at a directory so the first event triggers a real File::create error. Twenty identical synthetic scroll events are fed to that state and a recorder-off twin; delivered lines match and failed state reports off after each event. One toggle then returns a new default path and enabled Pending status without creating a file; another toggle turns it off. The generated path is not written, so no actual GROW_HOME logs are modified.

## Limits
The test injects an open error, not a write/flush failure. Those branches set the same Disabled sink, verified from source. No real gestures, disk-full condition, UI rendering or CLI relink is claimed. Error reporting remains the existing tracing warning; this change corrects the queried status and retry transition.

## Results
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib scroll_log --quiet: 6 passed, 0 failed.
- git diff --check passed. OpenSpec all strict before archive: 16 passed.
- Existing macOS compact-unwind-size warning remains. target 13 GiB, disk available 63 GiB.
