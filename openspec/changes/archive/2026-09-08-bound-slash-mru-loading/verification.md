# Verification

## Scope and evidence
Production ensure_loaded calls load_from_path with the normal store path; rank_score/last_used and touch reach that method. Tests call the same private path loader with isolated temporary files and never change GROW_HOME or user history.

New coverage:
- Actual 1 MiB valid JSON plus whitespace loads; one extra byte rejects and disables persistence; subsequent touch yields no snapshot and original bytes remain intact.
- Bounded reader consumes at most limit+1 for empty/exact/oversized input. A real file grows after metadata observation, then the same handle reads only limit+1 and rejects.
- Missing path and malformed JSON preserve existing persistence policy; 300 entries retain the newest256.
- Unix regular symlink loads; FIFO without a writer and directory reject with persistence disabled. Worker response has a2-second test guard.

## Limits
No Windows run or installed CLI/UI exercise. Encoded input allowance is not a JSON allocation/RSS budget or slow-filesystem timeout. Damaged JSON overwrite policy is unchanged; only read rejection disables persistence. Unbounded background queue and cross-process snapshot merging remain outside this change.

## Result
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib slash::mru --quiet: 14 passed, 0 failed, 0 ignored; 0.25s. Process exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
