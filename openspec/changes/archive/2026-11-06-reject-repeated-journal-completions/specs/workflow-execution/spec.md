## ADDED Requirements

### Requirement: Journal fold rejects repeated completions

Workflow journal restore 与物理行投影 SHALL 与 live complete_operation 保持严格一次完成语义：替换既有逻辑条目的 completion 物理行，仅当该条目 result 仍携带与其 operation id 历史一致的 pending marker 时才被接纳。条目已完成后再收到 completion 物理行（值相同或不同）SHALL 返回既有 Sequence 错误，不修改已 fold 的结果，并保留 operation id 历史供重放关联。

#### Scenario: 第二次 completion 行被拒绝

- **WHEN** restore 或投影在同一 seq 已 fold pending marker 行与合法 completion 行，又遇到第三行 completion（值相同或不同）
- **THEN** 返回 `JournalError::Sequence`，已 fold 的 Completed 结果保持第一次 completion 的值，journal 文件不被改写。

证据：`crates/codegen/workflow/src/journal.rs` — `fold_physical_entry`；消费方 `crates/codegen/shell/src/session/workflow/manager.rs` — `validate_suspended_journal`、`Journal::load_storage`；`crates/codegen/shell/src/session/trajectory.rs` — `project_workflow_entry`。

#### Scenario: 合法 pending 到 completion 与 live 对照保持

- **WHEN** 同一 seq 依次只有 pending marker 行与单条同 kind、同 request hash 的 completion 行
- **THEN** restore 与投影得到该 Completed 结果且 operation id 历史保留；live `complete_operation` 在 Pending→Completed 后对同一 seq 的第二次 completion（同值或异值）仍返回 Divergence，不追加物理行。

证据：`crates/codegen/workflow/src/journal.rs` — `fold_physical_entry`、`complete_operation`；既有测试 `projector_folds_pending_row_into_completed_replacement`、`pending_operation_survives_restart_and_resolves_without_a_second_intent`。
