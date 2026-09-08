# Verification

## Implementation and callers
The public helper retains pid/counter names and delegates its file operation to a private explicit-temp helper. Creation failure returns before cleanup. A successfully created file is written and closed before rename; subsequent errors remove that owned temp. Diagnostics ID-cache and workspace permission-state callers keep unchanged signatures, parent setup and mode inputs.

## Test evidence
Three config fs_atomic tests passed: a preexisting temporary file causes AlreadyExists without changing it or the destination; failed publication against a nonempty directory removes only the created temp and preserves destination contents; the public writer replaces an older longer file with complete new content and Unix0600. Tests use explicit tempfile paths, no GROW_HOME or environment mutation.

Command: CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p config --lib fs_atomic --quiet. 3 passed, 0 failed, 0 ignored; process exited0. git diff --check passed.

## Limits
No fresh diagnostics/workspace package tests or Windows runtime test. Caller signatures unchanged and source inspected. No new fsync/crash durability, collision retry or hostile parent-directory race guarantee.
