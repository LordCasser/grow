## Bash source-script regression

The probe extracted the current `DUMP_BASH_STATE_SCRIPT` from `crates/codegen/tools/src/computer/local/shell_state.rs` and ran it with GNU Bash 5.3.20. With exported `env_vars` and `__grow_dump_allexport` sentinels, it captured a dump, unset both variables, replayed the emitted shell payload, and checked both values and `declare -x` attributes. The current script passed. A controlled copy restoring the pre-change locals before `export -p` failed those same assertions, confirming red/green behavior.

A second source-script check set caller positional parameters before invoking `dump_bash_state` and confirmed both values and the argument count survived the call. This verifies that storing `$-` in the function's positional scope does not alter its caller's positional state.

## Production regression

`test_large_function_snapshot_roundtrips_with_allexport_bash` covers exported `env_vars` and `__grow_dump_allexport` values. It exercises two production `prepare_command`/fd dump/replay rounds with allexport enabled and disabled, while preserving a function larger than 1.2 MB.

Rust 1.93.1, shared `target/debug`:

```sh
RUSTUP_TOOLCHAIN=1.93.1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 \
  CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off \
  CARGO_PROFILE_TEST_SPLIT_DEBUGINFO=off cargo test --locked -p tools --lib computer::local::shell_state::tests -- --nocapture
```

Result: **29 passed, 0 failed, 0 ignored**. The new large-function test passed through the production descriptor path. The existing target had no matching Rust fingerprint, so Cargo rebuilt dependencies before running tests; this was a cache miss, not a test failure. Full log: `/tmp/grow-backlog-bash-tests.log`.

`openspec validate fix-bash-env-export-capture-shadow --strict --no-interactive` passed. This check preceded the integrated archive; final archive checks follow below.


## Integrated completion — 2026-10-20

- Full Python discovery with Rust 1.93.1: **29 tests OK, 1 skipped** (the existing OHOS-only smoke case on macOS). This run includes all 16 runner tests and the existing release metadata contracts. Existing Windows storage CI discovers the real cross-platform process-tree fixtures; hosted/native Windows was not run locally.
- Bash production descriptor tests: **29 passed**. Shell watcher tests: **17 passed**; targeted config-watch/owner tests **14 passed**; disconnect tests **15 passed**, all with zero failed/ignored. These scoped sets overlap and are not added as unique test totals. Pager non-test type-check passed. No compiler warnings in these runs.
- All three changes were archived with their deltas merged. Strict current validation: **16 passed**, archived validation: **648 passed**, zero failures. YAML, changed Rust formatting and diff hygiene checks passed; the unrelated pre-existing test formatting was preserved.
- Read-only identity checks confirm the existing v2.4.1 tag object/source and public release with all 11 assets match the preceding immutable receipt. MCP branch, active MCP change, stash, root Cargo.toml/Cargo.lock and Git index remain unchanged. No commit, push or release performed.
- An agent's accidental fresh debug target under target/release-dist was stopped and its own 2.8GB removed. The existing active default target cache was reused for verification; no global/user caches or release-dist artifacts were removed.
