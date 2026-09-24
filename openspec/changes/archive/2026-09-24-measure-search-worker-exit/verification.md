# Verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p pager --lib views::history_search::tests::dropped_worker_exits_after_large_history_request -- --exact --nocapture`: passed (1 test). The test submitted 30,000 records with 192 repeated characters each, observed request pickup, dropped the daemon, and observed worker exit within the 3-second test bound; the test itself completed in 0.01s.
- `rustfmt --edition 2024 crates/codegen/pager/src/views/history_search.rs` and `git diff --check`: passed. Note: rustfmt reflowed existing unformatted code in this shared file; review the complete diff with concurrent workspace edits in view.
- `openspec validate measure-search-worker-exit --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed (16 items).

The test observes worker exit for one in-memory workload; it does not establish a wall-clock guarantee for a specific nucleo operation. File-tree exit timing, blocking filesystem calls, and CPU/memory peaks remain unmeasured.
