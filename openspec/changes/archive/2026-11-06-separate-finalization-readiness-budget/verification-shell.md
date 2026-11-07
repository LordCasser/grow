# Shell adapter verification record

## Delivered sources

- `crates/codegen/shell/src/agent/mvp_agent/subagent_coordinator.rs` —
  `ShellChildRunner::finalization_ready` trait adapter: resolves the resident
  lifecycle-root parent via `get_session_handle(request.parent_session_id)`
  (the same handle `reconcile` feeds into `refresh_finalization_parent`) and
  delegates synchronously to the data predicate. No I/O, mutation, lease,
  epoch or deadline interaction.
- `crates/codegen/shell/src/agent/subagent/mod.rs` — crate-private
  `ShellCompletionData::finalization_ready(Option<&SessionHandle>)` plus the
  writer-level `finalization_ready_with_current_parent(Option<&ChatStateHandle>)`
  helper. Returns false when `output_write_in_flight` is set, when a retained
  `SessionThread` reports actor or persistence still running, or when neither a
  healthy current parent nor a healthy stored `parent_chat_state` exists.
  `usage_in_flight` / `receipt_in_flight` are deliberately not gated (observing
  their durable receipts is `reconcile`'s job). No state mutation.
- `crates/codegen/shell/src/agent/subagent/tests/mod.rs` — two in-module tests:
  - `finalization_ready_gates_on_parent_health_without_mutating_state`:
    missing parent → false; closed stored writer → false; healthy resident
    replacement covers stored closed → true while the stored writer stays
    closed (no adoption); closed resident falls back to healthy stored writer;
    prod `AtomicBool` output flag true → false and the flag is not cleared by
    the observation; false again → true.
  - `finalization_ready_requires_fully_retired_child_writer`: real
    `std::thread` actor retired while a real tokio persistence task
    (`SessionThread::with_persistence`) is alive → false; after the task
    settles → true. Temp thread owner drops through the reaper; no user
    session is spawned.

`cargo fmt`/`cargo` not run — the ACP fixture lock is exclusively held.
`rustfmt --edition 2024` applied directly to the two non-`rustfmt::skip`
sources; the tests module is `rustfmt::skip`.

## Final parent verification

The first unified compile exposed an incorrect `.as_ref()` on the by-value `ChatStateHandle`; the adapter now borrows the field directly. The first runtime test exposed a test-only inverted assertion (`!flag` despite the intended assertion that the flag remains true); the parent corrected that assertion without changing production state or gates.

The two new readiness tests both passed in `/tmp/grow-backlog-ready-plugin-shell-diagnostic.log`. That combined command exited101 solely because the independent AC-02 positive command-hook fixture had a nonexistent cwd; this is not reported as a green combined suite. The parent then ran the same freshly compiled Shell test binary with `agent::subagent:: --test-threads=1`: **250 passed / 0 failed, real exit0**, log `/tmp/grow-backlog-readiness-subagent-integrated.log`. This includes the two new tests and existing cancellation, timeout, finalization, ACK and cold-recovery cases. This closes the readiness integration boundary; it does not claim Windows field-machine reproduction.

Independent review `01a1af90-87ca-7500-b5b3-c33a1527029c` found no code defect in the gate/predicate. Its documentation observation was accepted: `SessionThread::is_finished` may request persistence stop after actor exit, so the final contract forbids recovery I/O/canonical mutation, lease acquisition and deadline changes while retaining that existing retirement signal. Trait, design and Shell comments now agree.

## Initial delivery status (historical)

- Syntax/format: rustfmt clean on owned files.
- `cargo test` for the two new tests and the existing finalization suite:
  **not run** — blocked by the exclusive ACP cargo window (same condition
  recorded in verification-tools.md). Source-complete and awaiting the
  parent-owned frozen-source cargo window.
- No Tools files, Activity, input admission, hooks/plugins, shared docs,
  backlog or main specs touched.
