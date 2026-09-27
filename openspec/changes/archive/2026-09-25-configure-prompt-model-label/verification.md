# Verification

- `cargo check --locked -p pager -p shell -p pager-render` and `cargo check --locked -p pager -p shell --tests` passed.
- Focused tests passed for compact/canonical prompt labels, Settings default and reset registration, live update with persistence rollback, and startup cache initialization.
- `rustfmt --edition 2024 --config skip_children=true --check` on changed Rust files, `git diff --check`, and `openspec validate --all --strict --no-interactive` passed.
- `cargo build --locked --profile release-dist --features release-dist -p cli --bin grow` passed after the final code edit. The binary was copied to `~/.local/bin/grow`; an atomic replacement was needed after direct overwrite caused macOS to kill the new process. The installed file matches the build output byte for byte, and `~/.local/bin/grow --version` returned `grow 2.2.0 (5a36c769)`.
