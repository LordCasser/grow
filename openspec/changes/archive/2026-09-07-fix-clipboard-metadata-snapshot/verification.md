# Verification

## Evidence and implementation
Native metadata previously read changeCount once, then classified types, treating missing types as false. PASTEBOARD_LOCK only serializes Grow threads; another process may replace clipboard content between messages. The original public comment overclaimed an atomic same-state read.

The private metadata_snapshot function now reads version, classifies, then reads version again. Known equal versions return classification; changed versions or unknown types return (None,false). The actual macOS entry point calls this function under the existing try_lock using the same retained pasteboard object. There is no retry loop, image-data read or additional thread. Documentation now describes an observed-version check, not a frozen external clipboard.

Callers were inspected: attachment_probe_gate treats unavailable snapshots as unable to rule out an image, while ClipboardFocusTipState retains retry opportunities for unknown classifications following the prior fix.

## Red / green
With the extracted function retaining old behavior, `cargo test --locked --offline -p client-support --lib metadata_snapshot_ --quiet` failed both new tests: version 41 was returned despite a 41→42 race, and call order lacked the final version read.

After implementation, using CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216:
- `cargo test --locked --offline -p client-support --lib metadata_ --quiet`: 3 passed, 101 filtered out. Covers changed/unknown/stable samples, read order, and occupied native lock. Injected callbacks and the occupied-lock early return do not access the real pasteboard.
- `cargo test --locked --offline -p pager-render --lib attachment_probe --quiet`: 4 passed, 1036 filtered out. Existing attachment routing/unknown-result gate coverage remains green.

No real pasteboard smoke test was run. No installed binary changed. This change does not guarantee the clipboard remains unchanged after return or bound native API/AppKit initialization duration.

## Resources
During validation target was approximately 6.7 GiB and the filesystem had 69 GiB available. No cargo clean was run during compilation.
