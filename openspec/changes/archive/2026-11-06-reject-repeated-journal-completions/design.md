# Design: reject-repeated-journal-completions

## 状态所有者与数据流

`Journal`（`crates/codegen/workflow/src/journal.rs`）独占 `entries`（fold 后逻辑视图）、`operation_ids`（每 seq 的 operation marker 历史，与 `entries` 等长）与可选 `storage`。物理行进入共享 fold 的两条入口：

- restore：`load_from_storage` ← `load_storage`（manager.rs:674 resume）与测试用 `Journal::load`；
- 只读投影：`project_physical_entry` ← Trajectory `project_workflow_entry`（trajectory.rs:2698）与 manager `validate_suspended_journal`（manager.rs:428）。

live 写入不走 fold：`begin_operation`（push pending marker 行）→ `complete_operation`（append completion 行后原地替换 `entries[index]`）。live `complete_operation` 在替换前要求 `existing.result` 仍带同一 pending marker（journal.rs:334 起），否则 Divergence 且不落盘——所以 live 是严格一次完成，journal 文件天然不存在两个 completion 行；能出现该行序的只有外来/被篡改/手工构造的 journal。

## 决策

- 守卫放共享 `fold_physical_entry`：restore 与投影必须共享同一语义，Trajectory/manager 消费投影且不可复制 fold 逻辑（父交接既定）。替换分支条件由“`operation_ids[index]` Some + incoming 非 pending”收紧为“`operation_ids[index]` Some 且 `pending_operation_id(&existing.result)` 与之相同 + incoming 非 pending”。
- 用 marker 相等而非仅 `is_some`：marker 与 operation 历史的一致性正是“同一 operation 的合法 completion”的判据；外来 journal 中 marker 与历史不一致的行不应当作合法 completion。
- 第二次 completion 复用既有 `Sequence` fall-through 错误（`index: entries.len(), expected: entries.len() as u64, actual: entry.seq`）：Sequence 表达“物理行序违反 fold 顺序契约”，不新增公共错误类型；Divergence 保留给 replay 时的调用偏离。失败发生在任何修改之前，已 fold 结果与 `operation_ids` 原样保留。
- 不把 live 改 idempotent：本项为 loader/projector 补缺，live 语义与错误风格不动；不扩 `JournalStorage`/durable barrier 接口（F4 另项）；不改 `prune_trailing_host_error` 的显式 truncate（其恢复的 pending result 与 marker 相等守卫兼容）。

## 调用方与失败边界

- restore 遇第二次 completion 行 → `Journal::load_storage` 返回 Err，manager resume/resume 校验按既有 Journal 错误路径拒绝，journal 文件不被 fold 改写（truncate 仅属 torn tail 与 prune 机制）。
- Trajectory 投影遇该行序 → `project_workflow_entry` 返回 Err；增量刷新的既有回退（`replay_workflow_prefix` 重放已提交前缀）对合法前缀不受影响，错误最终上抛为 Trajectory 不一致。manager `validate_suspended_journal` 上抛为 "Workflow journal is inconsistent"。
- 每个物理行对同一投影实例只投喂一次（trajectory 按 offset 增量、rebuild 从 0 重放、validate 循环单遍），不存在重复投喂依赖旧“同值覆盖幂等”的路径。
- 合法路径不变：单条 pending→completion 折叠、正常顺序 `record`、`replay`/`replay_operation` 的 operation 历史关联、`agent_reservation_count` 等均不受影响。

## 验证强度

Tier 2（持久化接口与跨模块行为）：新回归经 `Journal::load_storage`、真实临时文件 `Journal::load`、`Journal::memory` + `project_physical_entry` 三个共享入口断言第二次 completion（同值/异值）拒绝与原结果不变；live `complete_operation` 的 Pending→Completed→same/different 对照断言两者语义一致。cargo 测试因 WS02 窗口被父占用由父执行（见 tasks/verification）。
