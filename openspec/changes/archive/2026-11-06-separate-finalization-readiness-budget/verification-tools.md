# Tools Coordinator verification record

## Implemented (this delivery)

- `ChildRunner::finalization_ready(&self, &SubagentRequest, &Self::CompletionData) -> bool`
  added to `crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs`
  with a default `true` (synchronous, no I/O / append / mutation / deadline change).
- `advance_finalizations` in `crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`
  now calls `finalization_ready` after the existing in-flight / attempts >= 3 /
  deadline gates and after resolving the completed child, and before
  `pending.attempts += 1`. A `false` observation `continue`s the tick: no attempt,
  no reconcile, no deadline renewal. After admission, the original reconcile
  checks and failure budget semantics are unchanged.
- Paused-clock tests in `coordinator_tests.rs` (TestRunner gains an
  `finalization_ready` flag, default `true` so all other tests keep default-host
  behavior):
  - `not_ready_observations_skip_ticks_then_converge_within_original_period`:
    six not-ready ticks dispatch nothing and spend no budget; readiness turning
    true inside the original 30s period admits exactly one attempt which commits
    the canonical completion. The query Option is matched via `if let Some`
    (type fix from parent review) and the actor is aborted before returning.
  - `three_admitted_attempts_exhaust_budget_without_fourth_dispatch`: three
    admitted uncommitted attempts are followed by no fourth dispatch and the
    pending responsibility remains queryable.
  - `readiness_after_original_deadline_does_not_dispatch_until_new_turn` (added
    per parent review so the deadline gate is not masked by budget exhaustion):
    readiness stays false across the original 30s deadline (zero dispatches,
    zero attempts spent), then turns true; several recovery ticks plus a
    read-only query keep `reconcile_calls == 0`. Explicit new-turn reopening
    remains covered by the existing
    `finalization_cycle_deadline_survives_queries_until_new_admission` test.

## Final parent verification

`cargo +1.93.1 test --locked --offline -p tools --lib -- not_ready_observations readiness_after_original_deadline three_admitted_attempts finalization_cycle_deadline_survives_queries uncommitted_success_stays_finalization_pending reconciliation_panic_keeps_finalization_pending --test-threads=1` completed with **6 passed / 0 failed, real exit0**. Log `/tmp/grow-backlog-readiness-tools.log`. Shell integration and independent review are recorded in verification-shell.md. No clock period or admitted attempt limit changed.

## Initial delivery status (historical)

- Not run: `cargo` (build/test/fmt) is forbidden this round because the Cargo
  target is exclusively held (first the ACP fixture coder, then the parent's
  sampler window). Entrypoints the parent should run in the unified Tools+Shell
  window:
  - `cargo test` covering at minimum:
      - `not_ready_observations_skip_ticks_then_converge_within_original_period`
      - `three_admitted_attempts_exhaust_budget_without_fourth_dispatch`
      - `readiness_after_original_deadline_does_not_dispatch_until_new_turn`
      - `finalization_cycle_deadline_survives_queries_until_new_admission`
      - `uncommitted_success_stays_finalization_pending_instead_of_becoming_failed`
      - `reconciliation_panic_keeps_finalization_pending`
- Performed instead: direct `rustfmt` on owned files; the previously flagged
  `if let Ok(snapshot)` type error is corrected to `if let Some(snapshot)`;
  gate-order review against the design's sequencing requirement.

## Out of scope here

Shell adapter (`finalization_ready` implementation), docs/backlog sync, spec
archive — owned by parent / separate coder as scheduled.
