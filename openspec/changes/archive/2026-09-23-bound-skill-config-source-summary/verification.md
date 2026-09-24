## Verification

- `cargo test --locked --offline -p shell --lib extensions::skills::tests:: -- --test-threads=1`: 24 passed, 0 failed. The controlled blocking-worker tests cover timeout, capacity wait, retained permit, cancellation, panic, and empty success. The source-summary production call passes its synchronous Git/filesystem scan to that same bounded worker.
- `cargo fmt --check -p shell`: passed.
- `git diff --check`: passed.
- `openspec validate bound-skill-config-source-summary --strict --no-interactive`: passed.
- Archived as `2026-09-23-bound-skill-config-source-summary` and merged into configuration-rules; post-archive full strict and archived validation passed.

The source summary still uses the same `discover_auto_sources` function and returns the original skill list from the worker rather than cloning it. The test does not inject a slow OS filesystem call into that function; it injects a blocked worker at its execution boundary.
