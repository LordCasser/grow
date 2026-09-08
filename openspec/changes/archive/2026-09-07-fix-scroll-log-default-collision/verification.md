# Verification

## Evidence
The old default_log_path formatted time to seconds, and open_writer used File::create, so distinct recorders in the same second could truncate the same generated path. Both from_env_at default values and MouseScrollState::toggle_scroll_log call default_log_path. It now delegates to a directory/time helper adding UUID v4, using the already-present dependency. Path generation remains side-effect-free and Recorder::new remains pending until its first write.

## Coverage
The new regression supplies one fixed timestamp twice under a private directory, verifies distinct paths and no directory/file before writing, then uses real ScrollLogRecorder::write_line to write and flush two different JSONL lines. Both files retain their exact respective content and expected timestamp prefix/extension.

## Limits
No user environment was changed and no real gestures were recorded. UUID collision prevention is probabilistic, not an adversarial path-reservation protocol. Explicit target paths retain File::create behavior, and stream schema/timing is unchanged. Recorder growth, synchronous I/O and disabled-state visibility are separate backlog items. No CLI relink is claimed.

## Results
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib scroll_log --quiet: 5 passed, 0 failed, including prior wire format and toggle checks.
- git diff --check passed; OpenSpec all strict before archive: 16 passed. Existing macOS compact-unwind-size warning remains.
