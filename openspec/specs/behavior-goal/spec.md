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

### Requirement: Late child finalization respects the original Goal owner

迟到或恢复的子任务结果及用量 SHALL 保留原 Goal identity 与 definition revision。只有当前 owner 与准入仍允许时才能触发自动消费或续跑；结果提交不得重新打开已停止或已换代的 Goal。失效 owner 的通知 SHALL 沿既有持久 dismiss 规则收敛，并保留可审计结果。

#### Scenario: Goal stopped before child receipt was admitted
- **WHEN** child result 恢复成功时其原 Goal 已暂停、阻塞、完成或预算关闭
- **THEN** 原结果和用量仍归属原 Goal，不自动重开准入或恢复模型工作。

#### Scenario: Goal definition changed during finalization
- **WHEN** 回执携带旧 definition revision，而当前 Goal 已采用新 revision
- **THEN** 回执不会作为新 revision 的依赖完成或续跑信号，按既有失效 owner 规则持久收敛。

证据与实施入口：`crates/codegen/shell/src/session/actor/notification_drain.rs`、`crates/codegen/shell/src/session/goal_tracker.rs`、`crates/codegen/shell/src/agent/subagent/mod.rs::admit_subagent_completion_receipt`。

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

### Requirement: Goal continuation respects explicit readiness

Goal continuation SHALL 在既有身份/预算/控制 gate 下检查当前工作等待意图。所有当前可推进工作均明确等待时，内层准入与外层 idle 重排 SHALL 都保持事件等待，不产生重复模型调用或忙循环。存在其他可推进步骤时 MAY 继续。等待 SHALL NOT 自动变成 Goal paused/blocked，也不得累计 blocked 的连续 turn 审计。

#### Scenario: BG-01 Active Goal has only waiting work
- **WHEN** Goal Active，当前所有可推进工作都等待真实依赖，且没有新输入
- **THEN** 不采样、不持续重新调度 idle；相关依赖变化后重新核对准入，等待时间不算一次 blocked turn。

#### Scenario: BG-02 Another step can proceed
- **WHEN** 一个步骤等待 child，但同一 Goal 还有可推进的独立步骤
- **THEN** 可继续处理该步骤，不因某处有 child 在运行就冻结整个 Goal；没有显式计划时继续原全目标审计逻辑。

#### Scenario: BG-03 User control wins over readiness
- **WHEN** 依赖变为就绪时已有用户输入、Stop、挂起、权限/用户等待或预算关闭
- **THEN** 按原控制与输入优先级处理并重新核对；就绪事件不能自动回答问题、授予权限或越过关闭的执行 gate。

#### Scenario: BG-09 Update arrives while Goal or request admission is closed
- **WHEN** Goal Paused 或预算关闭，或 child 正等待真实用户/权限请求，此时父项变化和依赖结果到达
- **THEN** 只保存交付与处理责任，无新采样或自动批准；恢复/回答/授权仍由原控制与请求 owner 接纳，TODO 通知不替代它们。

证据与实施入口：`crates/codegen/shell/src/session/actor/goal.rs::{schedule_goal_on_idle,drive_goal_on_idle,start_goal_internal_turn}`、`idle_arbitration.rs`。

### Requirement: Goal completion audits explicit obligations and the whole objective

Goal complete SHALL 保留原目标整体完成审计，并检查当前范围内显式必需交付、有效等待、影响这些交付的未处理要求变化及必要收尾。未闭合时 SHALL 返回可操作的引用与原因。普通局部 TODO 或无关后代步骤 SHALL NOT 自动成为硬门槛，全部勾选 SHALL NOT 自动完成 Goal。有理由地解除某项义务 SHALL NOT 改写原目标已经达成的判断。

#### Scenario: BG-04 Checked TODOs hide unfinished mandatory work
- **WHEN** 父 TODO 都显示 completed，但必需 child 尚未交付、验收或处理影响交付的要求变化
- **THEN** complete 被拒绝并指出未闭合义务，直到接受或明确处置；旧清单状态不能代替全目标审计。

#### Scenario: BG-05 Unrelated local TODO is still open
- **WHEN** 原目标与必需交付已通过审计，某 child 留有不属于当前必需交付的局部探索项
- **THEN** 不仅因该复选框未勾选就阻止完成；仍活跃的执行和真实收尾责任按现有 owner 规则处置，不能隐藏副作用。

#### Scenario: BG-08 Three-level delegation preserves local acceptance and root authority
- **WHEN** root Goal 的必需交付由普通 Task child 承担，child 又委派 grandchild 并作局部验收
- **THEN** child 不获得 Goal mutation authority；root 依据 child 的结果及局部交付摘要作自己的验收，检查 root 必需义务和原目标，不要求 root 消费 grandchild 第二份回执或勾完所有后代 TODO。

证据与实施入口：`crates/codegen/shell/src/session/actor/goal.rs::{render_goal_continuation_from,handle_goal_command}`、`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs`。

### Requirement: Local planning never substitutes for Goal or Plan authority

TODO 变化、重规划及验收 SHALL NOT 自动创建 Goal、更新 Goal definition revision、恢复暂停 Goal 或修改已批准 Plan artifact。真正 Goal 定义变更、Plan 越界和 Workflow 执行 SHALL 继续遵守原 authority 与审批协议；弱 TODO 版本不能替代 Goal revision fence。

#### Scenario: BG-06 TODO update preserves the active Goal identity
- **WHEN** 活动 Goal 内父 TODO 修改并触发 child 重规划
- **THEN** 原 Goal identity/revision、预算归属和 child admission 保持原含义，不因 TODO 版本改变失效；真正旧 Goal revision 的迟到结果仍遵守原失效 owner 规则。

#### Scenario: BG-07 Plan refinement exceeds approved scope
- **WHEN** 局部计划调整只是执行细化，或实际超出了已批准 Plan 的工作范围
- **THEN** 前者不生成新的审批 artifact，后者遵循原重新审批路径；不能以 TODO 调整名义绕过审批或 Goal/Plan 互斥。

证据与实施入口：`crates/codegen/shell/src/session/actor/{goal.rs,behavior.rs}`、`crates/codegen/shell/src/session/goal_tracker.rs`。

### Requirement: Retained plans remain under the existing session owner
会话 SHALL 在既有 SessionActor/Behavior owner 下保留一个当前计划候选的不可变引用，并区分候选内容、Plan 执行阶段及 UI 展开状态。保留 SHALL NOT 批准、拒绝、放弃或执行计划，不新增 workflow owner；approval_pending 不得被无区别复用为所有保留文档的状态。

#### Scenario: Defer an unapproved plan
- **WHEN** 用户明确选择保留稍后审阅
- **THEN** 先确认持久候选引用，再释放当前交互占用，候选仍未批准且不会自动实施。

#### Scenario: Defer resolves a live or resumed waiter
- **WHEN** 保留决定已持久接纳，原交互为 live plan_control 或 idle 恢复审批
- **THEN** 清除 approval_pending，不产生 Execute/Revise handoff；live 路径仅结算一次延后结果并沿 EndTurn 结束，idle 路径只结束 waiter，不重新采样、重弹审批或恢复旧版本实施。

#### Scenario: Leave Plan with a retained candidate
- **WHEN** 用户按既有规则切换到其他 Behavior
- **THEN** 保留候选仍可只读发现，但不能自动抢占新 owner；再次实施需重新接纳。

#### Scenario: Select another Behavior then reload
- **WHEN** 有保留候选的会话切换到 Normal/Goal/Workflow 后保存并恢复
- **THEN** 候选按独立于 Plan runtime 的生命周期通过同一 Control snapshot 校验；不携带伪造的活跃 Plan phase，也不放宽其他跨 runtime 非法字段校验。

实施入口：`crates/codegen/shell/src/session/behavior.rs::PlanRuntime/select_behavior/finish_plan` 和 SessionActor；保留资格是新增契约，四阶段不是新增能力。

### Requirement: Later plan approval binds the reviewed submission
重新审阅后的批准 SHALL 绑定确切候选提交、artifact revision/hash 和稳定决定身份，在后端冻结快照校验成功且决定持久接纳后才进入既有执行路径。旧决定不得应用于后续提交或另一个计划周期。

#### Scenario: Candidate changes during review
- **WHEN** 用户审阅 v1 时当前候选变成 v2，随后提交 v1 的批准
- **THEN** 返回失效/冲突并要求审阅 v2，不批准 v2 或自动执行旧候选。

#### Scenario: Same text is submitted in a new plan cycle
- **WHEN** 退出 Plan 后重新提交相同正文且旧窗口回答迟到
- **THEN** 新旧提交身份可区分，旧批准不生效，新决定也不被历史 handoff 错误去重。

#### Scenario: Candidate replacement races an unresolved decision
- **WHEN** 旧候选的批准/放弃仍在接纳或 ACK 未确认，新提交尝试替换候选
- **THEN** 在同一 owner 边界先核对原决定，不覆盖其身份或未决证据；已执行/放弃的提交不能再次直接获批，重新执行需要新提交与重审。

实施入口：`behavior.rs::pending_plan_approval_snapshot/approve_submitted_plan_if_with_feedback`、`actor/tool/mod.rs::resume_plan_approval`。

### Requirement: Retained plan execution preserves existing arbitration and revision phases
批准保留计划 SHALL 复用当前 live ResampleStep 或 idle PlanHandoff，不建立第二条 turn admission；Goal、Rhai Workflow、用户 FIFO hold 和工具权限仍按各自 owner 生效。未批准计划修订 SHALL 回到 Drafting 后 submit，执行中修订 SHALL 经 Amending 和新版本重审。

#### Scenario: Approval without an active turn
- **WHEN** 合法候选在无活跃 turn 时获批
- **THEN** 通过原 durable handoff 和空闲仲裁最多启动一次实施，不把已有恢复能力重复实现。

#### Scenario: Existing work prevents immediate execution
- **WHEN** 当前前台任务、队首 hold 或准入故障阻止执行
- **THEN** 返回真实等待/拒绝原因，不绕过仲裁或激活 Goal。

#### Scenario: Revision changes implementation requirements
- **WHEN** 用户反馈改变计划要求
- **THEN** 形成新 artifact 后重新审批，不继承旧版本批准或暗改被批准正文。

#### Scenario: Approval comments and revision intent differ
- **WHEN** 用户提交批准说明或明确选择要求修改
- **THEN** 前者不改变冻结 artifact，后者进入 Drafting/Amending 修订路径；不依靠模型猜测评论语义来绕过重新审批。

实施入口：`actor/tool/preparation.rs`、`actor/notification_drain.rs`；既有执行与授权边界继续适用。

### Requirement: Plan completion retains an inspectable durable result

Complete SHALL 在原审批执行契约下要求完整 report。原始报告 SHALL 通过已有不可变存储保全，原计划身份和报告引用 SHALL 与退出 Normal 在同一 Control 提交中确认，随后才发布成功并恢复正常采样以生成简短总结。写入或提交失败 SHALL 保留原 Plan 并返回失败，不发布完成状态。完成引用 SHALL 只在下一次成功手动 Behavior 选择时清除，包括同模式选择；自动控制转换与恢复不得清除。fork SHALL 不继承父 session 的完成展示身份。

#### Scenario: Persist completion
- **WHEN** Executing Plan 完成，原始报告写入成功且 Control 提交确认
- **THEN** 当前协议为 Normal，完成引用仍可恢复，后续 Normal 回复提供简短总结，canonical 查询读取原计划及原始报告。

#### Scenario: Completion cannot be committed
- **WHEN** 报告写入或 Control 提交失败
- **THEN** 原 Executing 状态保留，不宣称完成，不发布成功总结或无效引用。

#### Scenario: Manual selection clears the completed result
- **WHEN** 完成后下一次手动 Behavior 请求成功接纳，即使目标仍为 Normal
- **THEN** 清除完成展示引用并持久确认；请求未接纳则保持原引用。

#### Scenario: Restore and fork
- **WHEN** 原 session 恢复，或从包含完成结果的历史创建 fork
- **THEN** 原 session 保留完成展示，fork 保留历史事实但不继承原 session 的完成展示身份。
