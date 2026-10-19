# Verification evidence

## Current verification — 2026-10-19

`/tmp/grow-v2-4-0-shell22.log` records `session::actor::coordination::tests::source_startup_admission_survives_rebuild_without_duplicate_append_or_notice ... ok`. This exact actor test covers first durable admission and notice projection, reconstruction from canonical Timeline, exact retry without duplicate append/notice, and conflicting target or question returning typed `Conflict` without Timeline mutation. Although the parent later terminated the overall Shell test process because another test hung, this named test completed successfully; no Shell suite total is claimed.

`/tmp/grow-v2-4-0-openspec-all29.log` reports strict validation **31 passed, 0 failed**, including this change. The directly covered implementation, contract, and regression tasks are complete; no task is inferred from package totals.

The focused actor test in `crates/codegen/shell/src/session/actor/coordination.rs` is `source_startup_admission_survives_rebuild_without_duplicate_append_or_notice`. It covers first durable admission and UI notice projection, restoring the canonical Timeline, exact retry without another notice, and conflicting target or question returning typed `Conflict` without changing the Timeline.

The source-admission acceptance case is verified by the named test above. The interrupted Shell run does not establish an overall Shell suite pass.
