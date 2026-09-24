# Verification

- Release run [36031134530](https://github.com/LordCasser/grow/actions/runs/36031134530) failed on both Windows targets in `pager` with six `E0658` errors from `std::os::windows::fs::MetadataExt` (`windows_by_handle`). The same run passed all five Linux asset jobs; macOS and OHOS results are still pending.
- The local `quarantine_leaves_a_replacement_entry_after_validating_open_source` regression passed with `cargo test --locked --offline -p pager --lib quarantine_leaves_a_replacement_entry_after_validating_open_source` on macOS. This local invocation used Homebrew Rust 1.98.1; release CI pins Rust 1.93.1.
- `cargo metadata --locked --offline --no-deps --format-version 1`, `git diff --check`, and `openspec validate --all --strict --no-interactive` passed.
- A local `cargo check --target x86_64-pc-windows-msvc` with pinned Rust 1.93.1 could not reach Grow: the macOS host lacks Windows C headers required by `ring` (`assert.h` missing). Native Windows release CI is the target compilation check.
- The corrected release jobs are pending.
