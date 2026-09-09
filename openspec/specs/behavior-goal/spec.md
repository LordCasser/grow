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

### Requirement: Idle control completion refreshes Behavior availability
Shell SHALL 在空闲会话的 model、Agent 或 Behavior 控制结束并释放前台占用后发布当前 Behavior 可用性，客户端不能永久保留临时忙碌投影。

#### Scenario: 尚未发送消息时切换控制
- **WHEN** 用户在尚未发送消息的会话完成 model 或 Agent 切换
- **THEN** 支持的 Goal 和 Workflow 选择按当前空闲事实可用，无需先发送或停止 turn。

#### Scenario: Behavior 控制完成
- **WHEN** 空闲会话完成 Behavior 切换
- **THEN** 后续选择依据释放控制占用后的可用性。

证据范围：`crates/codegen/shell/src/session/actor/model_switch.rs`、`session_mode.rs`。

### Requirement: Root owns the shared Goal admission lifecycle
Only the root session SHALL synchronize the shared Goal provider admission window from its durable Goal tracker. Descendants SHALL consume that window for admission and usage settlement without replacing its lifecycle from their local tracker.

#### Scenario: Spawn a child during an active Goal
- **WHEN** an active root Goal spawns a descendant whose local Goal tracker is empty
- **THEN** descendant initialization preserves the root Goal identity and both sessions can admit requests under that identity.

#### Scenario: Descendant synchronizes while root admission is closed
- **WHEN** root admission is inactive, exhausted, or usage-incomplete and a descendant initializes or synchronizes local control
- **THEN** the descendant cannot reopen admission or replace its root state.

#### Scenario: Root lifecycle transition
- **WHEN** the root durably pauses, restarts or exhausts its Goal
- **THEN** its shared admission window follows that transition and stale Goal requests remain rejected.

### Requirement: Late usage preserves stopped Goal lifecycle
Usage admitted under a Goal SHALL remain attributable after it stops. Recording late unknown usage SHALL preserve the existing stopped status and SHALL NOT preempt unrelated foreground work. Only a currently Active Goal with an explicit token budget may be stopped by incomplete usage.

#### Scenario: Late unknown usage after completion or another stop
- **WHEN** an admitted attempt settles without usage after its Goal became Complete, Blocked, BudgetLimited or Paused
- **THEN** the incomplete-usage marker is persisted while the stopped status remains unchanged and unrelated foreground work is not preempted. An already-paused Goal may still stop a retry belonging to its own retiring turn.

#### Scenario: Unknown usage during an active unbudgeted Goal
- **WHEN** an Active Goal has no explicit token budget and usage is unknown
- **THEN** usage remains a lower bound, admission remains open and the Goal continues.

#### Scenario: Unknown usage during an active budgeted Goal
- **WHEN** an Active Goal has an explicit token budget and usage is unknown
- **THEN** provider admission closes and the Goal pauses at its safe step boundary.

### Requirement: Goal token budgets are explicitly opt-in
Goal SHALL have no token spending limit when creation omits token_budget. Persistence, restore, continuation and usage settlement SHALL preserve the absence of a budget without substituting a default cap. Editing an existing explicitly budgeted Goal without a budget argument SHALL preserve that explicit budget until the user removes it.

#### Scenario: Create and restore without a budget
- **WHEN** a Goal is created without token_budget and later restored
- **THEN** its token_budget remains absent and cumulative token usage does not impose a spending limit.

#### Scenario: Incomplete usage without a budget
- **WHEN** an unbudgeted Goal receives missing provider usage
- **THEN** it retains a lower-bound usage ledger and can continue without an inferred token limit.

#### Scenario: Preserve an explicit budget during objective editing
- **WHEN** an explicitly budgeted Goal is edited without changing the budget
- **THEN** the existing explicit budget remains until an explicit budget-removal operation.

### Requirement: Delegated turns preserve complete Goal ownership
A Goal-owned descendant turn SHALL record both goal_id and the matching definition_revision from its inherited immutable Goal context when its local tracker has no matching owner revision. Timeline SHALL continue rejecting partial or mismatched ownership evidence.

#### Scenario: Child starts with an empty local Goal tracker
- **WHEN** the shared admission window names a Goal and inherited context identifies that same Goal
- **THEN** the descendant TurnStarted contains the Goal id and inherited revision and can cross the durable admission boundary without creating a local Goal runtime.

#### Scenario: Inherited context belongs to another Goal
- **WHEN** inherited context does not match the shared window Goal id and no matching local tracker supplies the revision
- **THEN** turn admission fails without inventing a revision or committing an invalid TurnStarted.

### Requirement: ACP background tasks retain complete Goal ownership
The ACP terminal backend SHALL retain goal_id and goal_definition_revision from background task admission in all task snapshots and completion notifications. Missing ownership SHALL remain absent; the adapter SHALL NOT invent or discard a revision.

#### Scenario: Goal-owned background command completes
- **WHEN** an ACP background task is admitted with a Goal id and definition revision
- **THEN** task lookup and completion notification preserve both fields unchanged for durable notification admission.

#### Scenario: Unowned background command
- **WHEN** an ACP background task has no Goal owner
- **THEN** its snapshots retain no Goal id or revision.

### Requirement: Shared budget windows do not confer delegated Goal ownership
A non-Goal child SHALL NOT acquire Goal turn ownership merely because its shared budget admission window names an active Goal. Child turn owner inference SHALL respect the delegated_goal provenance stamped from SubagentOwner. Root turn inference and shared provider admission enforcement SHALL remain intact.

#### Scenario: Ordinary Task child observes a parent Goal
- **WHEN** a non-Goal child with no inherited Goal context starts a turn while the shared window names an active Goal
- **THEN** it commits an unowned turn identity instead of an incomplete Goal owner.

#### Scenario: Goal-owned child starts
- **WHEN** delegated_goal is true
- **THEN** its turn retains matching Goal id and revision validation and cannot bypass invalid ownership as an ordinary Task.
