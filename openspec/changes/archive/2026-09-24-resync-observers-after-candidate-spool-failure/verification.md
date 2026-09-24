The leader's bounded spool write failure was induced end to end by unlinking the listening socket and its directory after a client had attached, then sending a candidate above the 8 MiB rollover threshold. The observer stayed attached; a discarded attempt sent no candidate or resync, an accepted attempt sent only `grow/leader/resync_required`, and independent updates continued. A second accepted failure during an in-flight `session/load` delayed resync until after the load response. A separate spool test corrupts a later record header and verifies a read error after one accepted record was emitted.

Pager tests route a root resync into a full replay window and confirm that successful completion replaces a stale transcript even without a replay marker. A child resync targets the root; a failed load restores the original transcript. Existing mixed-client and replay-overlap regressions also pass.

- `cargo check -p shell -p pager --tests`: passed.
- `cargo test -p shell leader::server::tests:: --lib`: 135 passed.
- `cargo test -p pager leader_resync_ --lib`: 2 passed.
- `cargo test -p pager app::acp_handler::tests:: --lib --quiet`: 335 passed, 1 ignored, after separate stale permission-audit assertion repair.
- `openspec validate --all --strict --no-interactive`: 18/18 passed before archive.
- `git diff --check`: passed.

The finite spool budget is per candidate, not a global process or disk budget. Generic non-Pager clients receive the resync extension and must use canonical `session/load` themselves; Grow Pager performs it automatically.
