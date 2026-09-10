## Scope and evidence

The implementation adds `ask_parent`, `ask_subagent` and `send_subagent_message` through the existing coordinator and role-gated tool catalog. It reuses the existing inquiry Sideband, receiving UI row and Timeline notification inbox. No live user session or provider configuration was modified.

Scenario coverage:

| Contract scenario | Verification |
| --- | --- |
| Child clarification while its parent is busy/waiting | Coordinator harness keeps blocking spawn pending while direct-lineage inquiries complete; actor inquiry regression runs beside a regular foreground turn. |
| Parent inquiry and worktree authorization | Same authenticated delegation authority serves both directions; sibling, unrelated and nested-descendant routes are rejected. Delegation bypasses peer workspace approval, while existing peer approval tests remain intact. |
| Single receiving row | `delegated_question_updates_one_primary_view_row` correlates receipt, approval and completion into one row without taking foreground activity. Source tools retain their own normal tool result. |
| Invalid route/cancellation | Coordinator tests cover unbound source, missing/terminal/cancelled child, sibling, root-to-nested target and upward intervention rejection; the Workflow cancellation test also verifies that questions/interventions cannot target hidden Workflow-owned children. Existing inquiry cancellation/restore tests cover settlement. |
| Queued/immediate intervention | `parent_intervention_is_durable_attributed_and_has_explicit_timing` checks queued context, sticky interruption signal, safe final-turn drain and rejection after steering closes. Sampling and interruptible waits share the existing soft-preemption path; non-interruptible tool dispatch is unchanged. |
| Restore/retry | The same intervention ID deduplicates before and after consumption; replay reconstructs the pending receipt and the final Timeline validates. Attribution explicitly excludes new human authorization. |

## Regression runs

Build settings: `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`; library suites use `--test-threads=4`.

- Agent: 475 passed. The initial prompt-size failures were fixed by shortening the new guidance; original limits were retained.
- Chat-state: 466 passed, 1 ignored.
- Pager: 7,161 passed, 10 ignored. The source-tool test now expects three source calls after adding `ask_parent`; receiving inquiries still use exactly one correlated row.
- Shell: 3,772 passed, 3 ignored.
- Tools: 2,357 passed, 7 ignored.

These five suites contain 14,231 passing tests. New routing, receipt/timing, actor inquiry and UI row regressions also passed as focused tests.

Final follow-up verification used an isolated source snapshot of `d827a074` plus this change, avoiding the concurrently edited sampling-recovery change's intermediate compilation state. The shell-only library run passed 3,769 tests with 3 ignored (the combined-package run above enables three additional feature-dependent tests). This run includes the final parent-specific wait-interruption wording and a local SSE success fixture: while the regular foreground remains active, an authenticated delegated inquiry reaches the provider without workspace approval, sends no tools, receives an answer, leaves the foreground context unchanged, and produces a valid Timeline. No live-provider credentials are needed.

The final coordinator regression passed all 37 tests after adding the existing Workflow hidden-child exclusion, including ordinary parent/child communication and rejection of questions/interventions targeting hidden Workflow children.

OpenSpec strict validation passed before archive (20 items), after archive (19 items), and for all archived changes (302 items). `git diff --check` passed. Counts include the other tasks' in-progress changes; only this change was archived.

## Operational boundaries

The positive provider fixture is local; no paid/live-provider smoke test is claimed. Existing Sideband tests cover request/attempt settlement and Goal admission. A full interactive TUI subprocess exercise is not part of this verification.

Cargo artifacts are shared with the concurrent sampling-recovery and Goal/usage tasks. Cleanup is coordinated after their active builds finish; deleting the shared target during another build is prohibited. Temporary verification source is removed after the final run.
