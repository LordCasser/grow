# Verification

- `cargo test -p chat-state read_only_identity_probe_matches_quarantine_across_surface_boundary --lib` passed (1 test).
- `cargo test -p chat-state response_admission --lib` passed (4 tests).
- `cargo test -p chat-state quarantine --lib` passed (5 tests).
- Cargo used `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216` for the repository's stack and disk constraints.
- `cargo fmt -p chat-state` and `git diff --check` passed.
- `openspec validate --all --strict --no-interactive` passed before and after archive; `openspec validate --archived --strict --no-interactive` passed after archive (506 changes).
