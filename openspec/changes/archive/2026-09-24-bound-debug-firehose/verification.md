# Verification

- The production debug target now has one attributed stream. The tracing layer formats at most 65,536 bytes per complete line and uses a 64-record try-send queue; only one worker owns the file. The default path is `$GROW_HOME/debug/firehose.txt`; explicit paths use the same bounded writer and their existing filter choice. The per-session sink map, worker-guard registry and latest-link publisher were removed.
- Each append checks the opened inode's live size while holding its exclusive cross-process lock. On overflow it retains a bounded complete-line tail in place; on a known path replacement it reopens or drops the record. The 32 MiB limit assumes cooperating appenders. A noncooperating external appender, or replacement after the identity check, is outside this diagnostic writer's guarantee. Legacy per-session files are age-pruned only when their cooperating shared lock is free; the new live stream is exempt.
- `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked --offline -p diagnostics --lib -- --test-threads=1`: 53 passed. Focused cases include blocked worker/queue loss and two-second flush timeout, concurrent processes sharing the 32 MiB cap, complete-line retention, path replacement, session attribution, multiline escaping and legacy sweep.
- After rebuilding the current CLI, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --test test_debug_logging -- --ignored --test-threads=1`: 6 passed. The tests now install a mock model in the sandbox actually used by headless and agent runs, so they reach the log assertions; default, explicit and disabled modes are covered.
- `cargo check --locked --offline -p shell -p pager -p diagnostics` and `cargo build --locked --offline -p cli --bin grow` passed. `cargo fmt --all -- --check`, `git diff --check` and strict active OpenSpec validation passed before archive. Archived and post-archive validation results are recorded below.

## Archive

- `openspec archive 2026-09-24-bound-debug-firehose --yes` merged two requirements and removed three obsolete per-session requirements.
- Post-archive `openspec validate --all --strict --no-interactive`: 15 passed, 0 failed. `openspec validate --archived --strict --no-interactive`: 464 passed, 0 failed. `git diff --check`: passed.
