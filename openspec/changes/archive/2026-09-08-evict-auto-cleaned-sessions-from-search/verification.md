# Verification

## Implementation
cleanup_stale_sessions_sync reports successful deletions through a callback while retaining its existing counts. The callback runs only after delete_if_still_stale returns Ok(true). The production wrapper captures the same root used by the adapter and enqueues each identity through SEARCH_INDEX_MANAGER. Both production callers run the synchronous wrapper in Tokio spawn_blocking; the underlying storage method has no global-home dependency. Existing asynchronous/debounce semantics are unchanged.

## Tests
All commands use cargo test --locked --offline -p shell --lib with CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_BUILD_JOBS=2, RUST_MIN_STACK=16777216.

- auto_cleanup_reports_keys_for_search_eviction: 1 passed / 0 failed, 0.64s. Creates an actual expired session under a temporary root, builds and queries a real index row, performs TTL deletion, checks returned identity, and dispatches through a local instance of the existing SearchIndexManager/worker. The query becomes empty after debounce. A separate absent-index branch invokes the same missing-summary handler and verifies no index is created. An earlier direct-handler version also passed (0.15s); the final version adds actual queue dispatch.
- cleanup_rechecks_activity_after_scan_under_writer_lease: 1 passed / 0 failed, 0.28s. Existing refreshed/future/corrupt/identity/expired/held/skip fixtures remain valid; callback identity is reported only for successful expired deletion, never for held/skipped sessions. Corrupt/refreshed same-entity rechecks use the private recheck helper; the callback's placement after Ok(true) is source-verified for those branches.
- cleanup_ttl: 3 passed / 0 failed, 0.00s. Invalid cutoff inputs now additionally assert no deletion callback is invoked.
- test_evict_removes_row_and_never_creates_index: 1 passed / 0 failed, 0.01s.

No process-global cleanup against user history was run. The production Once wrapper's startup scheduling and captured-root enqueue were source-inspected, not driven by resetting global config in tests. No installed binary was replaced. The existing macOS compact-unwind linker warning remained. Scoped git diff --check passed.

Pre-archive strict all validation passed (17 items). No cargo/rustc process remained before cargo clean; 7372 files / 2.7 GiB removed.
