# Verification

## Old red（静态分析：本 worktree 不能运行 Cargo，WS02 窗口被父占用）

四个新测试均针对共享 `fold_physical_entry` 的替换分支。旧条件为 "`operation_ids[index]` Some + incoming 非 pending + kind/hash 相同"，对第三行 completion（同值或异值）全部成立，因此旧行为是静默覆盖并返回 Ok：

- `restore_rejects_a_second_completion_for_a_completed_operation`：旧行为 `Journal::load` 返回 Ok 且结果为 "second"，测试期望 `Err(JournalError::Sequence { expected: 1, actual: 0, .. })` → 旧红新绿。
- `load_storage_rejects_a_second_completion_for_a_completed_operation`：第三行与第二行同值，旧条件同样成立 → 旧 Ok 覆盖，新 Err → 旧红新绿。
- `projector_rejects_a_second_completion_and_keeps_the_first_result`：旧行为不返回错误且结果被覆盖，`replay_operation` 得到最后一次 completion → 断言 `Err(Sequence)` 在旧代码失败 → 旧红新绿。
- `live_complete_operation_rejects_a_second_completion`：live 分支（journal.rs:334 的 pending marker 检查）本次未改动，旧代码即 Divergence → 对照/回归守卫，新旧均绿，用于证明 fold 与 live 语义一致。

既有测试不受影响（逐项核对）：合法单次 completion 路径（`projector_folds_pending_row_into_completed_replacement`、`pending_operation_survives_restart_and_resolves_without_a_second_intent`）只喂一次 completion；`projector_rejects_another_physical_row_for_completed_seq` 走无 operation 历史的 Sequence 路径不变；prune/record/torn-tail 等路径未触碰。Shell 侧 Trajectory/manager 每个物理行对同一投影只投喂一次（trajectory.rs 按 offset 增量、validate 循环单遍），不存在依赖旧覆盖行为的调用。

## 已执行

- `rustfmt --edition 2024 --check crates/codegen/workflow/src/journal.rs`：先报 4 处 `matches!`/数组宽度换行差异，`rustfmt --edition 2024`（仅本文件）修复后复检 `RUSTFMT_CLEAN`（exit 0）。未运行 workspace/cargo fmt。
- `openspec validate reject-repeated-journal-completions --strict --no-interactive`：`Change 'reject-repeated-journal-completions' is valid`（openspec 1.11.0，exit 0）。
- 最终 diff 自查：实现仅 `fold_physical_entry` 替换分支收紧为 marker 相等守卫（`operation_ids` 经 `Option::as_ref`、`existing.result` 经 `pending_operation_id(...).as_deref()` 比较），fall-through `Sequence` 错误、live `complete_operation`、`prune_trailing_host_error`、`record` 均未改动。

## 父级实际验证与验收

父整合专属 patch 并核对 `fold_physical_entry` 守卫、live对照与Trajectory/manager消费者后，在task `01a1af36-ebf0-7192-bb4c-a7c3c7548a8e` 的首条命令执行 `cargo +1.93.1 test --locked --offline -p workflow --lib -- --test-threads=1`。实际 **69 passed / 0 failed，exit 0**；四条新增测试均出现在实际通过列表，完整日志 `/tmp/grow-backlog-journal-f5.log`。同一组合命令后续独立child integration编译失败（缺mut fixture）不作为Workflow失败或通过证据。

父核已有合法单次完成、prune、torn-tail及engine取消/恢复回归均保留；开发说明同步。未重新执行旧源的四条新fixture，上文old-red仅是源码机制分析，不能作本次运行结果。未跑Windows或全仓跨crate运行；默认CLI在包含本修复的整合源码构建成功（task `01a1af3c-80c9-79b3-bedc-95bed611e0a1`）。F4 durable-prefix屏障仍是独立待办，不因本项通过而关闭。

## 边界与未执行项

- Coder 的隔离 worktree 未运行 Cargo；父已运行上述69项库回归，clippy未运行。
- 未验证 Windows 现场与全量跨 crate 回归（Trajectory/manager 消费方无代码改动，行为收紧仅影响含第二次 completion 行的外来 journal）。
- 不改 Shell、backlog、其他 change；F4 durable barrier 接口未触碰。

## 最终文件 SHA256（rustfmt 之后）

- `crates/codegen/workflow/src/journal.rs`：`50860f56d3cb586d434780ca2ef30cc2f26e4b3e72584e3fc207a4d9a43e0f2c`

git status 确认本次专属改动仅两处：`M crates/codegen/workflow/src/journal.rs` 与新增目录 `openspec/changes/reject-repeated-journal-completions/`（proposal/design/tasks/verification + specs/workflow-execution/spec.md 共 5 个 md）。worktree 中继承的其他 dirty 状态未触碰、未导出。
