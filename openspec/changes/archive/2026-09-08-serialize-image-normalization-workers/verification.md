# Verification
- main, low-disk shell lib tests (incremental off, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216, locked/offline).
- session::normalize_cache::tests: 13 passed, 0 failed/ignored, 0.15s. Cancellation regression starts and blocks actual worker, aborts async waiter, verifies second request stays pending for100ms, releases first and receives second result; existing panic-to-error test and cache dedup pass.
- session::image_normalize::tests: 41 passed, 0 failed/ignored, 36.54s. Includes camera-sized photo compression, aspect ratios, size/area budgets and existing image validation.
- No actual clipboard/TUI/native terminal or multi-process total-memory measurement. Gate is per process and only this adapter; waiting encoded inputs and OS/terminal allocations are not capped here. No user GROW_HOME mutation; shell unit tests have pre-main log redirection.
