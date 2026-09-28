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

### Requirement: Goal usage exposes provider-style components
Goal SHALL durably accumulate full input, output and available cache-hit/cache-miss classifications for admitted model attempts, including descendants and sidebands, using the existing acknowledged settlement boundary. Cache-miss input SHALL mean full input minus cache-hit input for an attempt with both counts known, including cache-write input; it SHALL NOT require a separately reported miss field. When cache read is unknown, that attempt's input classification SHALL remain unknown independently of known total consumption. Known failed attempts SHALL retain usage; repeated settlement SHALL NOT charge twice. The token budget and displayed total SHALL both count full input plus output, including cache-hit input. Reasoning tokens SHALL NOT be added again to output.

#### Scenario: Cache-inclusive budget
- **WHEN** an attempt reports 500 input tokens including 200 cache hits and 80 output tokens
- **THEN** Goal total and budget consumption increase by 580 and the three components increase by 200, 300 and 80 respectively.

#### Scenario: Cache-only request
- **WHEN** a request reports only cache-hit input
- **THEN** it increases total usage and consumes the token budget.

#### Scenario: Restore and continued accounting
- **WHEN** a classified Goal is resumed
- **THEN** counters, classification availability and their recorded coverage persist, and further usage adds once under the original Goal owner.

#### Scenario: Missing usage or historical categories
- **WHEN** a provider omits usage or a restored Goal has only historical aggregate consumption
- **THEN** unavailable categories are identified without fabricated values; total is a lower bound only when total consumption itself is incomplete. Existing incomplete-usage rules prevent exact budget enforcement only for unknown total consumption, while unbudgeted work continues.

#### Scenario: Settlement fails
- **WHEN** durable Goal settlement fails
- **THEN** total and classified usage roll back together and retry cannot bypass the persistence boundary.

#### Scenario: Cache details are missing but token budget is exact
- **WHEN** Goal attempt 的 full input=500、output=80 可确认，cache read/write 没有报告
- **THEN** Goal 总量和预算消费增加 580，缓存分类保留未知，不能把 500 全部计为已知 cache miss；不因分类缺失单独关闭预算准入。

### Requirement: Goal status is queried on demand
Goal 行为提示、续跑上下文与查询工具说明 SHALL 明确优先使用已提供的目标和预算，只有上下文缺失或需要最新状态时查询，不要求例行轮询。业务完成审计 SHALL 保留原有证据要求，不能由读取 Goal 状态代替。

#### Scenario: Continuation has Goal context
- **WHEN** 续跑已提供目标和预算
- **THEN** 指令要求直接进行业务完成审计或推进工作，不为重复确认目标而调用 get_goal。

#### Scenario: Fresh budget needed
- **WHEN** 需要上下文中不存在的最新状态或预算
- **THEN** get_goal 仍可用并返回当前读模型，不自动暂停或完成 Goal。

### Requirement: Behavior availability projections follow admission changes
The Shell SHALL publish a versioned Behavior availability projection when a foreground transition or pending step-control change alters Behavior admission while the session remains available. The projection revision SHALL be allocated before reading its inputs and SHALL increase for every newly built projection. Pager SHALL accept a projection only when its revision is newer than the latest accepted revision, so a delayed older projection cannot replace newer availability.

#### Scenario: Prompt promotion refreshes an open Behavior picker
- **WHEN** a queued prompt is promoted from an idle session into a regular foreground turn while the client is attached
- **THEN** the Shell publishes the availability assessed from that foreground state before starting the turn, and Pager refreshes any open settings snapshot from the new projection

#### Scenario: Goal continuation refreshes Behavior availability
- **WHEN** idle arbitration promotes an Active Goal continuation into a regular foreground turn
- **THEN** the Shell publishes the availability assessed from that foreground state before the continuation starts

#### Scenario: Foreground enters terminal settlement
- **WHEN** a regular foreground enters Settling for its durable terminal transaction
- **THEN** the Shell publishes the availability assessed from the busy settlement state without holding the admission lock during publication

#### Scenario: Turn settlement returns the session to idle
- **WHEN** a regular turn has settled and completion arbitration admits no successor foreground work
- **THEN** the Shell publishes availability assessed from idle admission facts

#### Scenario: Manual compaction owns and releases foreground
- **WHEN** manual compaction takes an idle foreground and later completes without admitting successor work
- **THEN** the Shell publishes its busy projection at admission and its idle projection after completion arbitration

#### Scenario: Pending step control changes Behavior admission
- **WHEN** a model, Agent, or Goal step control is admitted or its queue is drained
- **THEN** the Shell publishes a projection after releasing the admission lock, and Pager displays whether Behavior selection may overtake the pending control

#### Scenario: Older projection arrives late
- **WHEN** Pager has accepted a Behavior availability projection with a newer revision and then receives an older revision
- **THEN** Pager retains the newer projection and its visible availability

### Requirement: Independent permission Sidebands coexist with live foreground attempts

Goal 用量准入 SHALL 允许独立后台权限 Sideband 与同 owner 当前 epoch、同一 active Goal 内仍在运行的前台或后台 attempt 并行。已返回但未确认结算、已认领结算、旧 epoch 的 attempt SHALL 继续阻止对应 owner 后续准入，关闭的 Goal SHALL 不接纳新请求。并发 attempt 的使用量 SHALL 各自确认且只计一次。

#### Scenario: Child permission arrives during foreground inference
- **WHEN** 主模型 attempt 正在运行且子 Agent 提交需主会话裁决的权限请求
- **THEN** 权限 Sideband 可以发出 provider 请求，无需等待主模型结束；运行中的权限 Sideband 也不阻止主会话的独立模型准入。

#### Scenario: A concurrent attempt returned but usage is unsettled
- **WHEN** 并发 attempt 已返回或开始结算但 owner 尚未确认其用量
- **THEN** 新准入等待对应结算；epoch 改变或 Goal 关闭后不利用后台身份绕过边界。

### Requirement: Concurrent Goal attempts use settled usage for admission

有 token_budget 的 Active Goal SHALL 根据已持久确认的累计用量决定是否接纳新的 provider attempt。累计值达到阈值后 SHALL 立即关闭新准入；此前已准入的并行 attempt SHALL 继续结算并完整计入累计值，即使最终总量超过阈值。权限 Sideband 与前台的既有独立并行 SHALL 保留；未知用量仍按既有精确预算规则关闭准入。

#### Scenario: Concurrent attempts cross the threshold together
- **WHEN** 前台和权限后台 attempt 均在累计值尚未达到预算时准入，两者已确认用量之和随后超过预算
- **THEN** 两次用量均准确且仅一次计入；阈值后的新准入被拒绝，既有 attempt 的结果不因后一次结算而被丢弃。

#### Scenario: First settlement exhausts budget while another attempt is active
- **WHEN** 一个已准入 attempt 的已确认用量先达到预算，另一个 attempt 仍在运行
- **THEN** 新请求不能准入，仍在运行的 attempt 可完成并按实际用量结算。
