## Verification

All commands used `cargo test --locked --offline -p pager --lib`, with incremental/debug information disabled, two build jobs and RUST_MIN_STACK=16777216.

- `unclassified_file_urls --quiet`: 4 passed (0.09s). Agent and Dashboard dispatch/peek cover missing, whitespace-only and failed original reads; non-empty original text takes precedence. Agent already-inserted bracketed text is not duplicated. Failed/dropped/persistence-failed probes do not insert fallback; Dashboard closed peek/question guards remain effective.
- `paste_key_tests --quiet`: 78 passed (0.14s), including existing successful image/file, completion/error, and prompt ownership scenarios.
- `views::dashboard::state::tests --quiet`: 245 passed (0.39s), including existing mixed-path, image-cap, stale-target, question-mode and successful attachment scenarios.

The new fixtures use root file URLs, which intentionally return no classifier entries. This tests the actual completion miss branch without allocating a 50 MB batch. The independent batch classifier budget tests establish its empty-result behavior; this is not an end-to-end oversized-clipboard UI test. No real clipboard access or Windows execution was performed. Existing macOS compact-unwind linker warning appeared; all final commands exited 0.

Both integrations invoke fallback only within attachment FullMiss and classifier None; rejected classified entries do not fall through. A successful URL-text insertion is a file completion and therefore takes precedence over a failed original text read, just as a successfully classified file already does.
