# Verification

## Static and formatting checks

- Read `behavior-goal` retained-plan and later-approval requirements, the archived F1 audit and probe, and the current coordinator/snapshot restore implementation.
- `rustfmt --edition 2024 crates/codegen/shell/src/session/actor/session_mode.rs crates/codegen/shell/src/session/actor/tool/mod.rs` — passed.
- `git diff --check` — passed.
- Adversarial review checked unknown Behavior rejection, durable-write-before-live-mutation ordering, manual completed-plan clearing versus GoalLifecycle preservation, and cold-restore submission identity. The retained candidate and counter are copied through coordinator selection semantics; Plan runtime fields remain reset. No additional owner or behavior path was introduced.

## Runtime tests

The centralized test binary reported:

- `plan_finish_projection_tests` filter — 8 passed.
- `session_mode::desired_state_tests::successful_control_intent_switch_persists_retained_plan_and_submission_counter` — 1 passed.
- `retained_plan_review_gate_tests` — 12 passed.

The filter includes the deferred-candidate/cold-restore identity and GoalLifecycle completed-plan preservation regressions. The deferred-switch test incorporates both archived F1 probes: it verifies the confirmation gate, latest Control after Deferred → Normal, and a distinct next submission after coordinator restore. The completed-plan tests verify the manual dismissal rule and GoalLifecycle preservation while keeping the submission counter monotonic. The failure-path test verifies that an unacknowledged durable selection leaves live and persisted Control state unchanged.

## OpenSpec validation

- `openspec validate preserve-plan-state-on-behavior-switch --strict --no-interactive` — passed before the final verification/tasks and developer-guide edits.
- Consolidated `openspec validate --all --strict --no-interactive`: 23 passed before archive. Post-archive validation is recorded in the closeout section.

## 集成补充

`retained_plan_review_gate_tests`：12 passed，0 failed。与 Plan projection 8 项及 ControlIntent 1 项合计 21 项通过。

## OpenSpec 归档核对

2026-10-19 已通过 `openspec archive <change> --yes` 归档；全部 tasks 完成。归档前全量严格校验 23/23；归档后 `openspec validate --all --strict --no-interactive` 16/16，`openspec validate --archived --strict --no-interactive` 634/634。F2 先于 F3 归档，核对主规范同时保留 cursor merge 与 child lifecycle/实际历史顺序场景。开发说明和 backlog 已同步，无 MCP 实现修改。
