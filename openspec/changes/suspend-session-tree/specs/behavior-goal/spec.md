## ADDED Requirements

### Requirement: Session suspension gates Goal execution without changing its status

会话挂起 SHALL 关闭该树的执行准入但保留 Goal identity、definition revision、业务状态、预算与累计用量。恢复 SHALL 只重新开放挂起前仍 Active 且满足既有预算、权限和存储条件的 Goal；Paused、Blocked、BudgetLimited、Complete SHALL 不被挂起恢复激活。挂起离线时间 SHALL 不计入实际运行耗时，已有与迟到用量仍按原 attempt/Goal 身份结算。

#### Scenario: Active Goal and descendants are suspended
- **WHEN** Active Goal 所属 root 及 child 因退出而挂起
- **THEN** Goal 保持 Active 的业务状态但无新执行，恢复后先续接原工作再进入既有自动续轮，不重复计量。

#### Scenario: Goal is already stopped or loses admission
- **WHEN** Goal 原本 Paused/Blocked/BudgetLimited/Complete，或恢复核对发现预算耗尽、用量不完整阻止精确预算准入
- **THEN** 恢复保持相应限制，不将会话恢复视为 Goal restart 或预算重置。

#### Scenario: Explicit cancellation competes with suspension
- **WHEN** 用户明确 Stop Goal/Cancel task 的决定已持久接纳后又触发挂起
- **THEN** 保存真正停止的状态，后续 resume 不撤销该决定。

目标入口：`crates/codegen/shell/src/session/actor/goal_support.rs`、`goal_tracker.rs` 与现有共享 Goal admission window。
