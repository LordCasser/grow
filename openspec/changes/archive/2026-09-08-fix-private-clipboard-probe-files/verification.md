# Verification

## Findings
The macOS fallback used process-independent fixed names under temp_dir. Both get_image and get_attachments could therefore overwrite or remove files belonging to another request. The native-read disable switch is active and intentionally selects this fallback, so it is not a dead feature or deletion candidate.

## Red / green
The first regression failed with identical PNG/TIFF/JPEG paths from two calls. After unique directories were introduced, the permissions assertion failed with 0755 rather than 0700. The final implementation supplies Permissions::from_mode(0o700) to tempfile::Builder before tempdir creation. Installed tempfile source confirms this is passed to DirBuilderExt::mode during creation, not applied afterward.

The factory returns the TempDir owner and three paths. Both get_image and get_attachments retain that owner through the whole fallback. run_attachments_osascript now receives these paths instead of making its own set. Existing selected-file cleanup remains; TempDir Drop handles all remaining files and error/empty-output returns.

## Tests
All commands use CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216.

- `cargo test --locked --offline -p client-support --lib clipboard_probe_ --quiet`: 2 passed. Distinct directories/paths, both directory modes, isolated simultaneous payloads, PNG read cleanup, sibling residue cleanup, and read-error RAII cleanup verified with real temporary files.
- `cargo test --locked --offline -p client-support --lib attachments --quiet`: 17 passed. Existing attachment protocol coverage remains green.

No system clipboard read/write or osascript invocation was performed by the new tests. No installed binary changed. The fallback script itself was source checked, not run against user clipboard content. This change provides isolated temporary storage, not subprocess deadlines or script-string escaping.
