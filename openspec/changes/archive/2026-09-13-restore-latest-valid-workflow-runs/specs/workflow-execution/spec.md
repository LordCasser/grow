## ADDED Requirements

### Requirement: Workflow restore retains the latest valid runs
Workflow 恢复 SHALL 至多返回最新 128 个通过既有恢复校验的 run，并按 Timeline 顺序交付。cleared、缺失或不满足既有恢复校验而被跳过的候选 SHALL NOT 占用有效恢复名额。恢复 SHALL 保留目录能力校验、单文件读取上限和既有致命错误处理。

#### Scenario: More valid runs than capacity
- **WHEN** Timeline 中存在超过 128 个有效恢复 run
- **THEN** 返回最新 128 个，顺序与 Timeline 一致。

#### Scenario: Recent candidates are unusable
- **WHEN** 较新候选因 cleared、文件缺失或校验失败被跳过，但较旧候选有效
- **THEN** 继续检查较旧候选，直至获得 128 个有效 run 或候选耗尽。

证据：crates/codegen/shell/src/session/storage/jsonl/mod.rs 的 load_workflow_runs_sync 及 tests.rs。

