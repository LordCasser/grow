# Verification

Executed 2026-11-06 in the parent-granted main shared-target compile window (Cargo reserved to this task; window released after the run). Commands (env `PATH` per grant, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, shared `target/`):

- `cargo +1.93.1 test --locked --offline -p workspace --lib client_fs -- --test-threads=1` — **18 passed; 0 failed** (0.25s), exit 0. Includes all three WS-02 regression tests via the public `WorkspaceOp::execute` path: `stat_hash_follows_same_size_same_mtime_rewrite`, `read_file_returns_current_bytes_after_same_ms_rewrite`, `stat_and_read_follow_atomic_replacement_with_preserved_mtime`.
- `cargo +1.93.1 test --locked --offline -p workspace --lib envrc -- --test-threads=1` — 7 passed; **1 failed**, exit 101: `envrc::tests::test_non_utf8_dir_name_loads` panicked at `crates/codegen/workspace/src/envrc.rs:364:34`, `unwrap()` on `Os { code: 92, kind: Uncategorized, message: "Illegal byte sequence" }`. Out of this change's scope (parallel dirty `envrc.rs` work); reported to the parent, not fixed here. The lib test target itself compiles.
- `cargo +1.93.1 test --locked --offline -p workspace --lib file_state -- --test-threads=1` — **41 passed; 0 failed** (5.62s), exit 0.

Candidate fix run before the cargo window:

- Read-result assertions corrected to the real wire types: `ClientFsReadFileRes::size` is `u64` and `hash` is `String` (only `FsStatRes` wraps them in `Option`), so the 6 assertions across the two `read_file` regression tests now compare plain values; all stat assertions keep `Option`.
- The same-ms rewrite fixture mtime is pinned to `UNIX_EPOCH + 1_700_000_000s + 100ns` (set via `set_mtime` after writing v1); the rewrite restores it to +101 ns. "Same millisecond" is now deterministic instead of depending on the wall clock at fixture-write time (a real-clock +1 ns bump could straddle a millisecond boundary).

Scope notes: production code untouched by the fix (tests-only edits in `client_fs.rs` `mod tests`); no `cargo fmt` run; no commit/archive (parent-owned).

## Parent acceptance

Parent inspected the three-file production patch and the corrected public-operation fixtures, and retrieved the full output of task `01a1af28-af7d-7bb0-b96b-77989fdd190f`. The ClientFS invocation ran 18 tests with zero failures and exit 0; the combined shell command's final exit alone was not used as proof, since the independent envrc invocation returned 101. `docs/development.md` now links the ClientFS contract and explains the repeated-hashing cost and concurrent-writer limit. No pre-fix execution was performed for the three new fixtures; the old-path mismatch is supported by the inspected memo-hit path, while the reported passing evidence is for the current implementation. macOS is the tested platform; other platforms are not claimed tested here.

Archive completed as `2026-11-06-read-client-files-with-current-digests`. Post-archive `openspec validate --all --strict --no-interactive` passed 23/23 items, `openspec validate --archived --no-interactive` passed 874/874, and `git diff --check` passed. Logs: `/tmp/grow-backlog-ws02-specs.log`, `/tmp/grow-backlog-ws02-archives.log`.

