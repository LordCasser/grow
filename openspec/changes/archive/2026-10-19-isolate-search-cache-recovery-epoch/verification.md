# Verification evidence

## Current verification — 2026-10-19

`/tmp/grow-v2-4-0-shell22.log` records both focused regressions as passed:

- `session::storage::search_recovery::tests::cache_epochs_follow_one_index_through_quarantine_and_recreation ... ok` verifies root-scoped invalidation and recreation without advancing an unrelated index epoch.
- `session::storage::search_recovery::tests::concurrent_root_repairs_advance_independent_epochs ... ok` verifies concurrent repairs advance independent per-index epochs.

The parent later terminated the overall Shell test process because a separate test hung, so this evidence does not claim a Shell suite total or pass. Strict OpenSpec validation `/tmp/grow-v2-4-0-openspec-all29.log` reports **31 passed, 0 failed**, including this change. The directly covered implementation, contract, and test tasks are complete.

Focused tests in `crates/codegen/shell/src/session/storage/search_recovery.rs`:

- `cache_epochs_follow_one_index_through_quarantine_and_recreation` covers root-scoped invalidation, recreation advancing only the repaired index, and a fresh epoch remaining stable.
- `concurrent_root_repairs_advance_independent_epochs` covers simultaneous repairs of two roots advancing independent per-index epochs.

These tests do not claim broader search bootstrap or completed-marker behavior beyond the assertions named above; no broader behavior is added to their coverage here.
