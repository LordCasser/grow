## Source and release identity

- Feature and fix changes, including their archived OpenSpec records, were committed as `a72fb70d`; the Minimal welcome and PTY fixture follow-up was committed as `921ba791`.
- `Cargo.toml` and all 33 workspace package entries in `Cargo.lock` resolve to `2.3.0`; `cargo metadata --locked --no-deps --format-version 1` succeeds.
- `crates/codegen/shell/changelogs/2.3.0.md` documents the `grow export` CLI contract change and uses an absolute compare URL. The shell changelog points to both 2.3.0 and the previously released 2.2.2 notes.
- `git ls-remote --tags origin refs/tags/v2.3.0` returned no existing remote tag at preparation time.

## Local validation

- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -- --test-threads=4`: 12,962 passed, 0 failed after the Minimal welcome fix. The shell bootstrap recovery test also passed in this parallel run.
- All 20 selected PTY cases from `.github/workflows/core-regression.yml` passed with the rebuilt 2.3.0 CLI. The separate Plan revision PTY case also passed.
- `cargo build --locked -p cli --bin grow`: passed. The local macOS linker emitted a compact-unwind warning for `__eh_frame`; no build error occurred.
- `target/debug/grow --version`, `target/debug/grow export --help`, and `target/debug/grow replay --help`: passed; the version is 2.3.0 and the documented arguments are present.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed.
- `openspec validate --archived --strict --no-interactive`: 567 archived changes passed before this release-preparation archive.

The tagged release preflight must run after archive, commit, and local annotated tag creation because `scripts/validate-release.sh` requires a clean committed tree with the tag at HEAD. The local toolchain is Rust 1.98.1; the official CI uses Rust 1.93.1 and remains the cross-platform release executor.
