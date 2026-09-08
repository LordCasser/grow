# Verification

## Source evidence
The old dispatcher formatted only %Y%m%d-%H%M%S and called fs::write on input-debug-{timestamp}.json, so repeated exports in one second targeted the same file. It also discarded create_dir_all errors. The new helper exclusively creates a tempfile with timestamp plus random component, writes and syncs through its owned handle, then keeps it and returns that actual path. Errors before keep release the temporary owner; the dispatcher uses the returned path and existing success/error feedback.

## Coverage
The filesystem regression uses a private temporary directory and fixed timestamp twice, checks different paths and distinct exact JSON content, verifies .json/name prefix and Unix 0600. It injects failure after writing partial data, checks only the two successful files remain and their content is preserved, and passes an existing file as a parent to verify directory failure without corrupting it.

## Limits
No real GROW_HOME, clipboard, or user input was accessed. No injected fsync/keep OS failure or process-crash test is claimed; owner/error control flow covers those pre-commit returns. No directory fsync or historical retention policy is added. Serialization and I/O remain synchronous. The existing input field sanitization is unchanged; this does not claim arbitrary diagnostic metadata is secret-free. CLI was not relinked.

## Results
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib input_log --quiet: 8 passed, 0 failed, including existing recorder/sanitization checks.
- git diff --check passed. OpenSpec all strict before archive: 16 passed.
- Existing macOS compact-unwind-size warning remains. target 13 GiB, disk available 63 GiB.
