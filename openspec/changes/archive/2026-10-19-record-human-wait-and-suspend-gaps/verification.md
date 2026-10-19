# Verification

## Scope and implementation

The original backlog had only volatile PendingInteraction state and ToolStarted could precede human permission. Timeline now records typed Question, Permission and PlanApproval attempt Opened/Resolved facts at the actual reverse-request boundary. Opened must commit before dispatch; unconfirmed Resolved withholds the answer. Facts carry call/attempt/kind/resolution, without question, answer or permission bodies. Automatic policy creates no human wait. Suspend closes the old attempt, and rebind uses a new identity.

Activity subtracts only exactly matched interaction intervals and validated same-operation tree suspension/release intervals. Resumed is not execution release. Missing/reversed/mismatched history remains unknown; overlapping work remains protected. After all splits, only fragments strictly longer than 30 seconds compress. Developer architecture links the durable contract.

## Executed evidence — 2026-10-19

Pinned Rust/Rustdoc 1.93.1, bounded build/test settings. The last complete suites pass: Shell82 **4219/0/5 ignored**, ChatState82 **564/0/2**, Storage42 **362/0/1**, Pager31 **7495/0/12**, Workspace32 **874/0**.

- `interaction_attempts_require_exact_single_open_resolution_pairs` passes in ChatState82, rejecting duplicate/mismatched/unopened resolutions and duplicate opens.
- Actual Question worker tests pass in Shell82: `question_worker_resolves_user_outcomes_with_opened_durable_before_acp`, `question_deadline_requires_closed_receiver_and_expired_shared_deadline`, `question_worker_never_reports_answer_when_open_or_resolution_persistence_fails`, and `suspended_question_attempt_is_closed_and_fresh_attempt_can_reuse_call_id`. Explicit cancel does not become timeout; elapsed original deadline plus closed receiver does.
- Workspace32's `permission_interaction_observer_matches_real_prompt_boundaries` exercises real PermissionManager/AcpPrompter calls, including automatic decisions with no human interval.
- `plan_approval_reverse_request_requires_durable_open_and_resolution` passes **1/1** in focus82, including exact error-path assertions: Opened failure sends no request; Resolved failure withholds Approved after one request and leaves only the matching durable Opened, no handoff.
- `plan_approval_transport_and_malformed_responses_close_the_exact_attempt` and `suspended_plan_approval_rebinds_with_a_fresh_attempt` each pass **1/1** in focus87. The actual Plan request closes failures as Failed; transport closure during suspension records Suspended and a rebound actor uses a distinct attempt that closes as Answered. These cover reverse-request facts; immutable approval-decision Control CAS is separately verified by retain-plan.
- Storage42 passes `durable_interaction_wait_is_removed_but_unresolved_attempt_stays_unknown`, `only_a_verified_plan_wait_is_removed_from_its_execution_span`, `offline_span_requires_exact_operation_and_tree_membership`, and `overlapping_protection_is_coalesced`. Pager31 passes `confirmed_runtime_offline_time_has_its_own_gap_kind`, `overlapping_execution_remains_protected_during_runtime_offline_span`, and `offline_splits_keep_short_fragments_and_preserve_monotone_mapping`.
- Real pending Plan approval cold resume passes PTY78 **1/1**. All seven PTY77 runs pass: root/tree suspend/continue/export, three partial-leaf releases, concurrent resume, ordinary history continue. These establish actual exit/rebind behavior; the controlled actor tests establish the typed attempt failure boundaries.

Logs use `/tmp/grow-v2-4-0-` plus the suite names above, `focus82-plan_approval_reverse_request_requires_durable_open_and_resolution.log`, both named `focus87-*` logs, `plan-pty78.log`, and `pty77-results.json`/individual logs. Component tests do not claim Linux/Windows process or external MRTR interoperability. The updated architecture document directly links this change's spec.

## Historical diagnostics and final checks

Earlier Question fixtures inspected a dead actor instead of the committed persistence log after an injected failure; they were corrected before Shell82. Pager's older 20-second fixture was replaced with a meaningful gap while retaining the strict 30-second rule. Final format92 and strict/archive results are recorded by the root before archive; no future run is counted as passed here.

Final pre-archive checks: format92 succeeds for the changed Shell/harness sources; full strict92 passes **22/22**, `/tmp/grow-v2-4-0-openspec-all92.log`. Every delta scenario has the named component or process evidence above; unrelated main-feature matrix gaps remain in their own active changes.
