## Scope and evidence

- `crates/codegen/client-support/src/clipboard.rs::platform::get_text` now sends the existing `pbpaste -Prefer txt` command through `run_clipboard_script` with `SCRIPT_TIMEOUT` (5s); the runner caps stdout and stderr at 1 MiB each and owns process-group cleanup.
- The existing runner preserves zero/nonzero process status. `get_text` still maps empty stdout to `Ok(None)` and nonempty bytes through `String::from_utf8_lossy`.
- This verifies the `pbpaste` subprocess clause only. Native AppKit execution and image file read/decode budgets remain open in `openspec/backlog.md`.

## Validation

- `cargo test --locked -p client-support --lib clipboard_script -- --test-threads=1` — passed, 2 tests. Covered normal output/status, exact 1 MiB stdout, stdout/stderr overflow, timeout, and descendant process-group cleanup.
- `git diff --check` — passed.
- `openspec validate bound-pbpaste-text-read --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — 20 passed, 1 unrelated active change failed: `reject-cross-segment-tool-id-collisions` omits eight existing scenarios in its modified `model-sampling` requirement. The failure does not involve this change or `client-surfaces`.
