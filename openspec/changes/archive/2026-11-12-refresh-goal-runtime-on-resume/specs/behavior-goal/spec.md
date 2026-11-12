## MODIFIED Requirements

### Requirement: Goal continuation state
Goal SHALL 只在 Active 状态自动续跑，并保存目标、预算、累计用量与状态。

#### Scenario: 目标暂停
- **WHEN** 目标进入 Paused、Blocked、BudgetLimited 或 Complete
- **THEN** continues_automatically 返回 false。

证据：`crates/codegen/shell/src/session/goal_tracker.rs` — `continues_automatically`。

#### Scenario: A suspended runtime restores Goal capability before execution
- **WHEN** 冷恢复完成持久执行权确认并准备解封，且会话使用恢复后的工具目录
- **THEN** 在恢复 Turn 或检查空闲 Goal 前核对 Goal 所需工具资格。符合资格的 Active Goal 沿既有准备条件继续。暂停的 Goal 保持暂停，随后用户成功执行 /goal restart 时可自动接续，无需额外用户消息。
