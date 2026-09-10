# Verification

- Before correction, the unmodified independent-process harness reproduced a SIGBUS stack guard fault in the fixture binary on native macOS, before the foreground model request. It was not a test-worker thread or a connection to the user's existing Grow process.
- Boxing the three awaited turn futures did not resolve the unoptimized stack overhead. The experiment is completely reverted; no conversation-turn calling convention change is retained.
- Final CLI: `CARGO_INCREMENTAL=0 cargo build --locked -p cli --bin grow` passed. Log: `/tmp/grow-v2.1.6-debug-stack-build.log`. The linker reported the existing large-debug-unwind-table warning; build succeeded.
- Debug session stack is now 32 MiB; non-debug remains 8 MiB. `release-dist` inherits release, with no debug-assertions override.
- The complete independent-process coordination harness passed all ten scenario groups after the separate `align-coordination-reload-regression` fixture correction. Log: `/tmp/grow-v2.1.6-coordination-final.log`. Fixtures use temporary GROW_HOME/workspaces and --no-leader; the coordination registry is rooted under that temporary GROW_HOME. Existing user processes were not stopped or replaced.
- Final standalone shell regression: 3,788 passed, 0 failed, 3 ignored (`CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib -- --test-threads=4`). Log: `/tmp/grow-v2.1.6-shell-final.log`. The combined core invocation earlier enabled three additional synthetic-replay testkit cases through feature unification; those also passed.
- Post-archive OpenSpec validation passed: 17 current items and 308 archived changes.
