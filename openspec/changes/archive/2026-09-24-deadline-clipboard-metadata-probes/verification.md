# Verification

- Reviewed the Pager event-loop snapshot and change-count callers, the deferred attachment version check, the native pasteboard lock, fallback image read, focus-tip retry state, and the archived clipboard contracts.
- `cargo test --locked --offline -p pager-render --lib clipboard::tests`: 46 passed, including a stalled metadata queue, late reply isolation, and the attachment baseline hook.
- `cargo test --locked --offline -p client-support --lib clipboard::platform::native_image_read_skips_an_occupied_native_lock`: 1 passed.
- `cargo test --locked --offline -p pager --lib clipboard_focus`: 12 passed, including unknown-classification retry.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed before archive.

The native AppKit operation itself cannot be interrupted safely. A stalled operation can retain one metadata worker and the pasteboard mutex; the Pager receives unknown metadata by its caller deadline, and later image reads take the bounded AppleScript fallback. The off-thread TOCTOU version check stays under its existing five-second outer task deadline instead of using the foreground 10 ms scheduling deadline.
