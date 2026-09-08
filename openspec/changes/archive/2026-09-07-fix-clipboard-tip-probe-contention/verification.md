# Verification

## Source evidence
The macOS metadata entry points took PASTEBOARD_LOCK.lock(), the same mutex native_image_read holds while accessing image data on a blocking worker. UI polling could therefore wait for that reader. Both metadata entry points now use try_lock and return unknown when occupied; native_image_read retains its blocking lock and serialization. No extra worker or lock was introduced.

The pager poll previously committed the cheap version when classification said has_image=false, including an unavailable classification. It now commits only a known classification version and records that version, preserving retries after unavailable results.

## Red / green
Before implementation: `cargo test --locked --offline -p pager --lib tips::clipboard_focus --quiet` ran 12 tests: 10 passed, 2 failed. The failures showed that unknown classification suppressed a same-version retry and version 42 was classified again after a cheap-41/classify-42 result.

After implementation (all commands also use CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216):

- `cargo test --locked --offline -p client-support --lib metadata_probes_skip_an_occupied_native_lock --quiet`: 1 passed, 101 filtered out. This macOS test holds the actual native mutex while calling both metadata entry points; both return unknown. It does not invoke AppKit or access the real pasteboard.
- `cargo test --locked --offline -p pager --lib clipboard_ --quiet`: 68 passed, 7029 filtered out, 0.06 seconds. Includes the two new deterministic state regressions and existing clipboard hint gates/throttle/cooldown behavior.

Existing macOS compact-unwind-size linker warning remains. No installed CLI was rebuilt or replaced. This verifies lock contention avoidance, not a total deadline for AppKit loading or native messaging. Cross-process snapshot consistency remains separately recorded in backlog.
