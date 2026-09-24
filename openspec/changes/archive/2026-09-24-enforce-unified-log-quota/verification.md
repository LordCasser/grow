# Verification

- `cargo test --locked --offline -p diagnostics --lib unified_log::tests`: 31 passed. New tests cover a 200-record burst of about 6.25 MiB inside one maintenance interval, two independent concurrent writer descriptors, an untrimmable 5 MiB file, and a replaced path. Every successful burst/concurrent append leaves the shared file at or below 5 MiB; retained lines parse as complete JSON.
- `cargo test --locked --offline -p diagnostics --lib`: 69 passed.
- `cargo test --locked --offline -p shell --test test_leader_stdio_integration unified_log_redirect_precedes_test_start`: passed.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate enforce-unified-log-quota --strict --no-interactive`: passed.

This capacity guarantee covers Grow writers that honor the inode advisory lock. External noncooperating writers and a pre-existing untrimmable file cannot be brought under the cap by this append path; the latter is not grown further.
