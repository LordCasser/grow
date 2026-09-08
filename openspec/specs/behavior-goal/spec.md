# behavior-goal Specification

## Purpose
定义会话协作协议与长期目标的不同职责。覆盖 Behavior 切换结果、Goal 自动续跑状态、连续阻塞审计以及预算用量归属，作为修改跨 turn 生命周期的验收边界。

## Requirements

### Requirement: Behavior is a session protocol
Behavior SHALL 是会话级互斥协作协议；切换按 runtime facts 判定，并以明确结果返回。

#### Scenario: 切换响应
- **WHEN** 用户请求切换 Behavior
- **THEN** 返回 applied、in_flight、superseded、confirmation_required 或 rejected 对应结果。

证据：`crates/codegen/shell/src/session/behavior.rs` — `BehaviorChangeOutcome`。

### Requirement: Goal continuation state
Goal SHALL 只在 Active 状态自动续跑，并保存目标、预算、累计用量与状态。

#### Scenario: 目标暂停
- **WHEN** 目标进入 Paused、Blocked、BudgetLimited 或 Complete
- **THEN** continues_automatically 返回 false。

证据：`crates/codegen/shell/src/session/goal_tracker.rs` — `continues_automatically`。

### Requirement: Consecutive blocker audit
Goal SHALL 在连续三个 Goal turn 报告同一规范化阻塞后进入 Blocked，同一 turn 不能重复计数。

#### Scenario: 阻塞重复报告
- **WHEN** 相邻 Goal turn 连续三次报告相同 blocker
- **THEN** 前两次保存审计计数，第三次转为 Blocked；更换 blocker 重新计数。

证据：`crates/codegen/shell/src/session/goal_tracker.rs` — `report_blocked`。

### Requirement: Goal usage identity
Goal SHALL 按稳定 goal_id 归属用量；缺失 provider 用量时累计值只能表示下界。

#### Scenario: 预算用量不完整
- **WHEN** 有精确 token budget 的目标存在未计量调用
- **THEN** 不完整用量阻止继续按精确预算消费；无预算目标可使用下界账本。

证据：`crates/codegen/shell/src/session/goal_tracker.rs` — `usage_blocks_budget`。
