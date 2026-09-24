# Verification

## Scope and evidence

The loader and writer share `MAX_STORE_BYTES` (1,048,576). `MruSnapshot::write` rejects before directory or temporary-file creation, and `persist_async` rejects before initializing or submitting to the process-wide writer. `SlashController::record_command_use` marks its shared `SlashMru` dirty again when that handoff fails.

Tests added:
- A snapshot exactly 1 MiB publishes through the existing atomic path; a snapshot one byte larger is rejected, leaves the existing destination byte-for-byte unchanged, and creates no leftover temporary file.
- The actual `SlashController::record_command_use` entry receives a command large enough to exceed the encoded allowance; it leaves the store dirty and creates no destination file.

## Results

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=1 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib slash::mru --quiet`: 19 passed, 0 failed, 0 ignored; 0.22s. The linker emitted its compact-unwind section-size warning.
- `openspec validate bound-slash-mru-snapshot-writes --strict --no-interactive`: valid.
- `git diff --check`: passed.

The Cargo run used the shared `target/debug` tree with one job; no build output was cleaned. This verifies the Rust unit-test entry, not an installed CLI/UI run.

## Limits

The complete JSON byte vector is still allocated before the size check. A map that remains over 1 MiB stays dirty and will be rejected again on later command-use attempts; this change does not prune entries by encoded size. Once an acceptable snapshot is handed off, subsequent filesystem failures retain the existing best-effort worker behavior.
