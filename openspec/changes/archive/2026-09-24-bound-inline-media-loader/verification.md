# Verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p pager --lib inline_media_ -- --test-threads=1`: passed, 13 tests (including oversized source rejection, per-view pending cap, normal async completion, and session-reset detachment).
- `openspec validate bound-inline-media-loader --strict --no-interactive`: passed.
- `rustfmt --edition 2024 crates/codegen/pager/src/app/agent_view/media.rs`: passed.
- `cargo fmt --all -- --check` was not used as the shared worktree has an unrelated concurrently edited PTY test registration that is not formatted yet.

The prepared-output ceiling is checked after the existing conversion function returns. Its transient conversion allocation remains governed by that function's existing 100 MB conversion-output cap per admitted worker; this change bounds source reads, worker count, and mailbox-retained output, rather than claiming a 16 MiB peak for conversion internals.
