# Verification

## Confirmed failure and repair

The promoted incomplete-usage regression first hung because an owned Goal notification retained its mutex across a durable projection ACK; taking the notification snapshot before awaiting removed that lock cycle. Its teardown then exposed unjoined compaction ownership. Joining on every invalidation would create a second, distinct cycle: the root accounting command waits for the worker, while the worker waits for the same inbox to acknowledge its Sideband bill.

Ordinary invalidation now signals cancellation and retains unfinished ownership. Explicit manual compaction, suspension, and shutdown drains join the original worker. The normal run loop and shutdown pump share the serialized Goal accounting dispatcher, keeping receipts live during a drain. Promoted-future cancellation and deadline expiry return the original handle to its existing slot.

## Executed evidence — 2026-10-19

Pinned Rust/Rustdoc 1.93.1, two build jobs, 16 MiB test stacks, incremental and dev/test debug information disabled. `/tmp/grow-v2-4-0-lib-build72.log` successfully compiles the Shell and ChatState test executables.

`shell-55cd31b2a4678c57 async_compaction --test-threads=2` from `crates/codegen/shell` passes **18 tests, 0 failed**, 6.88s; `/tmp/grow-v2-4-0-compaction75.log`. Named evidence includes:

- `async_compaction_promoted_job_with_incomplete_usage_cannot_commit`: one real root inbox dispatcher processes both incomplete usage and the worker's settlement; no parallel test-only settler hides the deadlock.
- `async_compaction_goal_budget_closes_and_settles_without_a_wait_cycle`: the budget transition uses the same accounting path, settles the bill and cannot publish a late summary.
- `async_compaction_cancelled_promotion_waiter_returns_original_owner`: the provider remains blocked while the promoted waiter is dropped; the original owner is recovered and explicitly cancelled/joined, producing one cancelled Sideband terminal and Failed compaction without Surface publication.
- Authority transition, rewind invalidation, late provider result, original deadline, original-request promotion and exact Goal charging regressions all pass in the same run.

`cancelled_drain_future_returns_the_same_unjoined_worker_owner` and `expired_drain_deadline_returns_the_same_unjoined_worker_owner` pass as named tests in `/tmp/grow-v2-4-0-shell-final62.log`. The latter invokes an actual short deadline, confirms the error and restored joinable handle, then explicitly aborts and joins the fixture worker. Shell62 itself has unrelated failures and is not a full passing package result.

Earlier cancellation fixtures assumed that signalling immediately emptied the slot, aborted promotion between its durable fact and UI publication, or released the provider before cancellation could win. They were corrected to observe the intended ownership boundaries; the cancellation/Sideband/Surface assertions remain.

Final Shell build81 and full run82 pass **4219 tests, 0 failed, 5 ignored**, `/tmp/grow-v2-4-0-shell-final82.log` (128.87s). All 18 asynchronous compaction tests and both owner/deadline tests above pass by name in that clean run. Focus82 independently rechecks the promoted waiter boundary (1/1). CLI73 plus harness76 pass the seven real process suspension/continue/export invocations in `pty77-results.json`; those process tests establish the shared suspension path, while the controlled compaction cases establish the worker and accounting failure boundaries. No process fixture is claimed to inject a busy compaction provider.

Call-site review confirms manual compact runs as its existing independent activity and awaits `cancel_background_compaction_before` before obtaining the lease; suspension passes the existing shared deadline; graceful and fatal teardown both use `cancel_and_drain_session_subagents`, whose loop selects the original worker join alongside the root command receiver and routes accounting through `dispatch_goal_accounting_command`. Thus those barriers do not block the settlement inbox they depend on. These are source-path checks, not separate OS-process fault injections. Pinned workspace fmt86 succeeds; change strict86 and full strict86 (**23/23**) pass before archive.
