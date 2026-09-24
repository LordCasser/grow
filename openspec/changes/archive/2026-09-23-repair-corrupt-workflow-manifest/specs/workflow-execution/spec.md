## MODIFIED Requirements

### Requirement: Workflow restore retains the latest valid runs
Workflow 恢复 SHALL 至多返回最新 128 个通过既有恢复校验的 run，并按 Timeline 顺序交付。cleared、缺失或不满足既有恢复校验而被跳过的候选 SHALL NOT 占用有效恢复名额。恢复 SHALL 保留目录能力校验、单文件读取上限和既有致命错误处理。

#### Scenario: More valid runs than capacity
- **WHEN** Timeline 中存在超过 128 个有效恢复 run
- **THEN** 返回最新 128 个，顺序与 Timeline 一致。

#### Scenario: Recent candidates are unusable
- **WHEN** 较新候选因 cleared、文件缺失或校验失败被跳过，但较旧候选有效
- **THEN** 继续检查较旧候选，直至获得 128 个有效 run 或候选耗尽。

#### Scenario: Corrupt sidecar is repaired before writer startup completes
- **WHEN** 冷恢复使用 Timeline seed 替代无法解码的 Workflow sidecar，且 sidecar 自读取后未变化
- **THEN** 将 reconciled seed 以持久化 ACK 写回后才完成 actor 初始化。

#### Scenario: Sidecar changes during repair
- **WHEN** 冷恢复读取到损坏 sidecar 后，该文件在修复锁取得前被替换或删除
- **THEN** 修复返回可观察的错误，不覆盖新内容，也不把 actor 初始化报告为成功。

证据：`crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `load_workflow_runs_sync`；`crates/codegen/shell/src/session/workflow/store.rs` — `WorkflowRunStore::from_restored` 和 manifest 写入；`crates/codegen/shell/src/session/persistence.rs` — Workflow persistence ACK；`crates/codegen/shell/src/session/actor/spawn.rs` — restored store 初始化。
