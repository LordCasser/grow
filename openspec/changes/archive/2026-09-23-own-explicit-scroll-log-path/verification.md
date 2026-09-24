# Verification

- `rustfmt --edition 2024 crates/codegen/pager/src/input/scroll_log.rs` — completed; unrelated formatting changes were reverted to keep the source diff scoped.
- `cargo test -p pager --lib input::scroll_log::tests::scroll_log_second_recorder_cannot_truncate_owned_target_or_alias` — passed (1 test).
- `cargo test -p pager --lib input::scroll_log::tests` — passed (7 tests), covering the new ownership regression, lazy truncation, byte limit, symlink target behavior, special-file rejection, and independent generated paths.
- `openspec validate own-explicit-scroll-log-path --strict --no-interactive` — passed.
- `git diff --check -- crates/codegen/pager/Cargo.toml crates/codegen/pager/src/input/scroll_log.rs` — passed.

The Pager test build emitted an existing linker warning that `__eh_frame` exceeded the compact-unwind encoding limit; the test binary linked and all selected tests passed.
