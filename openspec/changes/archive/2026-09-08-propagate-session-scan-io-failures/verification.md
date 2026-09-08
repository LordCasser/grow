# Verification (main, 2026-09-08)

All Cargo commands used --locked --offline -p shell --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2 and RUST_MIN_STACK=16777216.

- operational_scan_errors_preserve_cleanup_candidates: red 0/1 (0.17s), green 1/1 (0.32s). On this macOS uid 501 host, all four real chmod-000 cases ran: cwd directory, session directory, summary, and long-CWD marker. Each confirms permission denial with an OS open probe, then verifies listing and cleanup return PermissionDenied, no deletion callback, and both candidate directories retained. Permissions are restored before assertions.
- scan_opened_sessions: 5 passed.
- list_sessions: 7 passed, including corrupt-summary exclusion and existing sorting/hidden behavior.
- cleanup_: 6 passed (5.28s), including resource limits, writer ownership, refreshed/hidden/invalid states and duplicate discovery.
- bootstrap_many_sessions_under_descriptor_limit: 1 passed (3.03s), all 96 sessions indexed under child fd limit 64.

Total selected tests: 20 passed, 0 failed. Existing linker compact-unwind warning remains. Changed-file git diff --check passed.

Search failure scenario was verified by source ordering: reindex_all propagates list_sessions failure before indexing, prune_missing_if_claim_owner and completion-marker writes. No permission-fault injection was performed through that entire search entry point; the real permission regression covers the shared scanner and cleanup entry point. The Unix permission test explicitly skips root execution and is not compiled on Windows; current host execution was non-root, and native Windows behavior was not tested.

NotFound/InvalidData exclusions retain existing behavior. The change does not redefine whether already invalid session data should remain in a search projection, add retries, or rewrite history. All fixture writes/deletions and permission changes used temporary directories; no user session or installed binary changed.

Strict validation: pre-archive 17/17; post-archive all 16/16, archived 252/252. With no cargo/rustc process remaining, cargo clean removed 7,372 files / 2.7 GiB. Final available disk: 59 GiB.
