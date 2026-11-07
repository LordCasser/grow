# Reject repeated journal completions

## Why

Session F5（非 F4；用户已授权全部非延期 backlog，见 `openspec/changes/complete-nondeferred-backlog`）：live `complete_operation`（`crates/codegen/workflow/src/journal.rs:334`）要求 `existing.result` 仍携带同一 pending operation 的 marker，否则返回 `JournalError::Divergence`，即严格一次完成；但共享 `fold_physical_entry`（`crates/codegen/workflow/src/journal.rs:456`，restore `load_from_storage` 与只读投影 `project_physical_entry` 共用）只检查 `operation_ids[index]` 历史 Some 与 incoming 非 pending，不检查 `existing` 是否仍 pending，因此 `Pending→CompletedA→CompletedB` 的物理行序列会把已完成结果静默覆盖为 B。journal 的持久屏障因此弱于 live 语义：restore（`Journal::load_storage`，manager resume 使用）和 Trajectory / manager 消费的 `project_physical_entry` 都能接受 live 永远写不出的第二个 completion 行。

## What Changes

- `crates/codegen/workflow/src/journal.rs` `fold_physical_entry`：completion 替换分支增加守卫——仅当已 fold 条目的 `result` 仍携带与 `operation_ids[index]` 相同的 pending marker 时，才允许同 seq、同 kind、同 request hash 的非 pending 物理行作为合法 completion 替换；此后任何第二次 completion（同值或异值）落入既有 `JournalError::Sequence` 拒绝路径，不修改已 fold 结果，`operation_ids` 历史保留供重放关联。
- 不新增公共错误类型；不改 live `complete_operation`（本项是给 loader/projector 补缺，不是把 live 改 idempotent）、`prune_trailing_host_error` 的显式 truncate 修复机制、正常顺序 `record` 与合法 pending→completion 路径。
- 新增 Tier2 测试：`Journal::load_storage`、真实临时文件 `Journal::load`、`project_physical_entry` 与 live `complete_operation` 的 Pending→Completed→same/different 对照；断言拒绝后内存原结果不变、正常一次完成保持。

## Impact

- 代码：仅 `crates/codegen/workflow/src/journal.rs`（fold 守卫 + 同文件测试）。
- 契约：`openspec/specs/workflow-execution/spec.md` 由本 change delta 新增 strict-once completion requirement。
- 行为影响：restore/resume 与 Trajectory / manager 投影对含第二次 completion 行的 journal 从“静默覆盖”变为显式 `JournalError::Sequence` 拒绝；live 写入路径不存在产生该行序的合法来源（第二次 `complete_operation` 先被 Divergence 拒绝，不落盘），正常 journal 不受影响。
- 非目标：F4 journal durable barrier 接口（独立后续项）；Shell 侧代码、backlog、其他 change 文件均不改。
