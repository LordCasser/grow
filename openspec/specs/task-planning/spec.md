# task-planning Specification

## Purpose
定义会话局部 TODO、父步骤弱关联与子 Agent 自主重规划、结构化依赖等待及必需交付验收。计划状态以 Timeline 为权威，执行、权限、预算和生命周期继续由各自 owner 管理。

## Requirements

### Requirement: Plans belong to a local work scope

局部计划 SHALL 归属一个 session 内的已接纳工作范围，由该 SessionActor 管理；子 Agent SHALL 自主维护自己的 TODO。步骤 SHALL 使用范围内稳定身份，新消息/turn、显示排序及继承的历史 transcript SHALL NOT 自动新建计划范围或转移所有权。简单工作 MAY 不建立 TODO。

#### Scenario: TP-01 Child decomposes one parent step
- **WHEN** 父 Agent 将一个步骤委派给 child，child 将工作拆成三个本地 TODO
- **THEN** 父项保持原身份，三个子项只归属 child；父级能查看关联但不能借父 TODO 写入直接覆盖子列表。

#### Scenario: TP-02 Follow-up stays in the current work
- **WHEN** 同一工作收到用户补充输入并进入新 turn
- **THEN** 当前计划保留；只有明确变更工作范围的命令才切换范围，历史消息中的列表不能替换当前事实。

#### Scenario: TP-03 Simple task needs no plan ceremony
- **WHEN** Agent 可直接完成一个简单请求或无 TODO 的委派
- **THEN** 无需创建 Goal 或 TODO；无 TODO 也可使用有真实依赖的结构化等待。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs::{TodoState,TodoWriteTool}`、`crates/codegen/shell/src/session/actor/spawn.rs`。

### Requirement: Delegation optionally links one direct parent step

task SHALL 可选关联调用 session 当前工作范围的一个父步骤，由 runtime 校验并补入 owner；一个父项 SHALL 可关联多个执行。该引用 SHALL NOT 改变 Task/Goal/Workflow 执行 owner 或赋予结果有效性的硬版本约束。父项编辑与 spawn 的排序 SHALL 防止 child 使用旧要求且遗漏后续变更。

#### Scenario: TP-04 Multiple executions support one step
- **WHEN** 同一父项分别委派调查和验证，之后为验证发起一次重试
- **THEN** 三次执行有不同身份和同一父项引用，结果可分别验收；先前结果不被重试覆盖。

#### Scenario: TP-05 Spawn races with a parent edit
- **WHEN** task 准入与父项内容修改并发
- **THEN** child 获得最新委派要求，或以原委派要求启动并存在确定的补发责任；不能既拿到旧要求又漏通知。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/{mod.rs,types.rs,coordinator.rs}`，现有 `SubagentOwner` 与 `immediate_parent_session_id`。

### Requirement: Effective non-completion edits request replanning

父 TODO 的内容、要求、优先级、非完成状态、删除或取消发生有效变化时，系统 SHALL 为每个直接关联且尚可处理的 child 记录一次逻辑重规划责任。仅状态变为 completed、同值重复写入及纯展示/回执更新 SHALL NOT 触发；同批还修改其他任务意图字段时 SHALL 触发。父项状态本身 SHALL NOT 等同于执行取消。

#### Scenario: TP-06 Parent changes requirements or priority
- **WHEN** 一个已关联父项的内容、验收要求或优先级发生实际变化
- **THEN** 每个关联的活跃 child 收到对应来源变化的重规划请求；未关联的 sibling 不受影响。

#### Scenario: TP-07 Completion alone differs from completion with edits
- **WHEN** 父项仅从 in_progress 改为 completed，随后另一父项在同一提交中改内容并标记 completed
- **THEN** 前者不触发重规划，后者触发；两者都不会仅凭 completed 自动取消执行或接受 child 结果。

#### Scenario: TP-08 Reopen or cancel a parent step
- **WHEN** 父项改为 pending、in_progress 或 cancelled，且状态确实发生变化
- **THEN** 关联 child 收到重规划请求；cancelled 表达任务意图变化，立即终止执行仍需要显式取消控制。

#### Scenario: TP-09 No-op and presentation writes stay quiet
- **WHEN** 重复提交相同有效字段，或只刷新 UI、排序、用量、回执或验收记录
- **THEN** 不生成重规划责任，也不额外启动模型检查。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs::{apply_replace,apply_merge,TodoUpdate}`；当前字段与更新逻辑需要扩展。

### Requirement: Replanning is child-owned and bounded by safe execution

重规划 SHALL 在原 child 的安全边界评估，由 child 调整自己的受影响 TODO 或确认原计划仍适用。系统 SHALL 保留已完成且适用的工作，不因父项编辑自动重启 child、重发原 prompt 或杀掉外部命令。处理确认 SHALL 指向准确来源；无本地 TODO 不免除评估责任。

#### Scenario: TP-10 Child changes only affected work
- **WHEN** child 已完成调查，父项新增测试要求
- **THEN** child 保留调查结果，调整剩余实现/测试步骤并确认该来源变化已处理，不重置整个列表。

#### Scenario: TP-11 Existing plan still applies
- **WHEN** child 评估父项变化后确认当前计划已经覆盖新要求，或无 TODO 的直接执行仍适用
- **THEN** 它提交带理由的 no-change 处理确认；不要求伪造 TODO 修改，也不向 grandchild 传播无变化通知。

#### Scenario: TP-12 Parent edit arrives during a command or wait
- **WHEN** child 正运行外部构建，或正在等待 task_output，收到父项更新
- **THEN** 构建不因该更新被自动杀掉，安全边界处理新指导；可中断等待接收指导，并按原交付身份保留迟到结果。

#### Scenario: TP-13 Child cannot finish the check
- **WHEN** child 已读变更但需要用户信息或无法可靠判断如何调整
- **THEN** 保留未处理状态与原因，按现有用户等待路径处理；普通回复或消息消费不冒充重规划完成。

证据与实施入口：`crates/codegen/shell/src/session/actor/{notification_drain.rs,completion_delivery.rs,tool/mod.rs}`、`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs`。

### Requirement: Replanning updates coalesce without mutating delivery identity

系统 SHALL 按来源变化和 child 去重，在未形成不可变投递批次时合并连续更新。已存在的消息 identity/payload SHALL 保持不变，Received 批次不得由发送方单方撤销。多个批次在同一安全边界消费时 SHALL 保留各自准确上下文与顺序，明确最新要求，允许一次检查确认所覆盖的已消费来源集合。已处理旧版本 SHALL NOT 清除较新的待处理版本，乱序更新 SHALL NOT 回退已确认进度。“一次” SHALL 指逻辑责任，不保证故障中的物理模型调用次数。

#### Scenario: TP-14 Rapid edits coalesce
- **WHEN** 父项在 child 开始处理前连续修改三次
- **THEN** 可将未投递变化合并，已接收批次按原身份消费并以最新有效要求作一次检查，同时保留明确覆盖确认；同一旧消息身份不换正文。

#### Scenario: TP-15 Edit arrives during replanning
- **WHEN** child 正处理变化 A，父项又提交 B，之后 A 的确认迟到或重试
- **THEN** A 的确认只关闭 A，B 保留待处理；乱序/重复投递不回退到 A，也不重复完成同一逻辑责任。

证据与实施入口：`crates/codegen/chat-state/src/timeline.rs` 的 notification 事实、`crates/codegen/shell/src/session/actor/notification_drain.rs::receive_agent_message` 的 exact receipt 检查。

### Requirement: Link changes and terminal executions converge explicitly

删除或 replace 省略已关联父项 SHALL 保留原引用及移除原因并触发评估，不得抹掉未闭合执行/等待/交付。切换工作范围 SHALL 明确处置旧范围义务。已完成或已取消 child SHALL NOT 因父项变更自动恢复；未应用变更 SHALL 可见并由父级决策。关联 SHALL 只沿直接委派边逐级传播。

#### Scenario: TP-16 Replacement removes a linked step
- **WHEN** replace 列表省略仍有活跃 child 的父项，或删除后尝试把其 ID 用于另一项
- **THEN** 保留原项 tombstone 与通知责任，不复用其身份；child 评估移除后的局部计划。

#### Scenario: TP-17 Child finishes before receiving an edit
- **WHEN** 父项变化与 child 完成/取消竞争
- **THEN** 原结果及终态保留，无法应用的变化明确显示需父级处理；不重新唤醒 child 或自动另建 task。

#### Scenario: TP-18 Replanning propagates through local edits
- **WHEN** root 修改关联 child 的父项，child 调整了自身一个关联 grandchild 的 TODO
- **THEN** child 的那次实际修改通知 grandchild；root 不直接广播，child 未修改的其他项和普通处理回执不继续传播。

#### Scenario: TP-19 New work cannot erase old obligations
- **WHEN** Agent 请求切换工作范围但旧范围仍有未处置执行或等待
- **THEN** 直接覆盖被拒绝并返回待处置引用，明确处置后才可切换；历史计划保持只读来源。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs::{interaction_route,send_to_child_route}`、TODO replace 路径。

### Requirement: Waiting names actual dependencies and wake conditions

等待 SHALL 由结构化意图绑定真实且属于允许工作关系的 task、command、question 或 permission 身份，并明确多个依赖的 any/all 条件。系统 SHALL 在同一 owner 边界检查就绪并注册等待；失败、取消、中断与未知结果 SHALL 以真实状态唤醒决策。自由文本和 TODO in_progress SHALL NOT 单独创建等待。

#### Scenario: TP-20 Dependency completes during registration
- **WHEN** 依赖在查询与等待注册之间完成
- **THEN** 等待立即看到既有结果或收到同身份事件，不丢唤醒；any/all 条件分别按声明求值。

#### Scenario: TP-21 Invalid dependency is rejected
- **WHEN** 等待引用不存在/无权访问的 ID、自身执行或可验证的委派祖先循环
- **THEN** 返回具体错误，不创建永远无法就绪的等待，也不以超时轮询隐藏错误。

#### Scenario: TP-22 Failure wakes a decision rather than success
- **WHEN** 所等 child 失败/取消，或命令被中断、结果未知、产物不可用
- **THEN** 相关等待按真实 outcome 进入可处理状态，Agent 决定检查或调整；不标记成功，也不自动重试命令。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs`、`task_output/mod.rs`、`crates/codegen/shell/src/session/actor/tool/mod.rs`。

### Requirement: Yielded work remains unfinished under its admitted owner

有效结构化等待 SHALL 使普通工作或 child 暂停推进而保持未完成，在相关事件后由现有 actor/coordinator 继续原已接纳工作。其 owner、预算、max-turns、取消及权限 SHALL 延续，不创建 Goal 或另一个调度器。普通 final 且无有效等待 SHALL 保留既有结束语义；停止后的迟到事件不得重开准入。

#### Scenario: TP-23 Child yields for its own child
- **WHEN** child 注册等待 grandchild 的有效结果并结束当前模型回合
- **THEN** child 不被提前终结或 shutdown；grandchild 结果到达后由原 child 在原限制内处理并继续自己的 TODO。

#### Scenario: TP-24 Prose does not change lifecycle
- **WHEN** assistant 最终正文说“等待测试”，但没有有效结构化等待
- **THEN** 不从文字制造非终态执行或无限续跑；UI 仍忠实展示尚未完成的计划事实。

#### Scenario: TP-25 Stop competes with a wakeup
- **WHEN** 用户停止原工作，随后依赖事件或重规划通知到达
- **THEN** 只按既有 owner 规则完成持久收尾，不重新启动模型；预算/max-turns 到限同样不得被唤醒重置。

#### Scenario: TP-31 Repeated yields consume the original execution allowance
- **WHEN** 一个无 Goal 的普通 child 连续多次等待并被唤醒，累计轮数或用量达到原配置限制
- **THEN** 原 task execution owner 的累计记录持续增长，恢复也不清零；到限按原规则结束或要求新委派，不因新 turn 自动取得全新额度。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/handle_request.rs` 的 prompt completion 与 shutdown 路径、`crates/codegen/shell/src/session/actor/idle_arbitration.rs`。

### Requirement: Deliverables are reviewed without a hard result version gate

显式必需的委派交付物和关键里程碑 SHALL 区分执行完成、结果交付和父级验收；验收 SHALL 引用可验证结果及适用验证记录。TODO 修改 SHALL NOT 自动作废旧结果或要求所有后代步骤提供证据。交付要求变化后 SHALL 保留原验收并要求明确复核；父级可接受仍满足最新要求的既有结果。需修订不等于义务已闭合。

#### Scenario: TP-26 Delivered result still needs parent review
- **WHEN** 必需 child 返回成功并交付结果
- **THEN** 父级看到待验收结果，完成父项需明确接受或有理由地解除义务；子进程成功不自动勾选父项。

#### Scenario: TP-27 Earlier result satisfies changed requirements
- **WHEN** 父项交付要求变化，但先前结果仍满足新要求
- **THEN** 父级可引用原结果说明复核通过，不要求重新运行 child 或制造相同版本结果。

#### Scenario: TP-28 Rejected work and local progress stay distinct
- **WHEN** 父级要求修改一个必需交付，同时 child 有无关局部 TODO 尚未勾选
- **THEN** 必需交付仍未闭合；无关局部 TODO 不单独成为全局完成门槛，最终判断仍覆盖原目标。

#### Scenario: TP-29 Discard an execution explicitly
- **WHEN** 同一父项多个执行中有一个不再需要，或父项被取消/移除
- **THEN** 父级明确记录取消或解除必需义务及理由，处置原等待并保留仍存在的执行 owner；不得靠删行隐藏继续运行的副作用。

#### Scenario: TP-30 Result was produced in another worktree
- **WHEN** child 在独立 worktree 交付修改，而父级工作需要在目标工作区生效
- **THEN** 验收核对集成与验证依据，未集成不能冒充目标工作已完成；系统不因 TODO 完成自动 merge。

#### Scenario: Structured final output obeys planning obligations
- **WHEN** StructuredOutput is accepted while a required deliverable, pending replan, or released live obligation remains
- **THEN** the turn returns to the model with the same concrete completion blockers as an ordinary final answer; it does not finalize the turn.

#### Scenario: TP-32 Parent removes a step while its command is running
- **WHEN** 父项被取消/移除，关联 child 正运行外部命令且通知暂不能送达
- **THEN** 记录来源责任及执行仍可能继续的事实，不声称命令已停止或必需义务已解除；通知送达后的安全边界先评估变化再派发受影响的新副作用，立即停止仍需显式 task cancel。

证据与实施入口：`crates/codegen/chat-state/src/timeline.rs::SubagentResultEvent`、`crates/codegen/shell/src/agent/subagent/handle_request.rs` 的结果/worktree 路径、`actor/goal.rs::handle_goal_command`。

### Requirement: Committed local plans have one live and resumed projection

ACP plan notifications and Todo resources SHALL project the committed Timeline plan through the existing Todo success conversion. Session startup hydration and each plan commit SHALL publish that projection; read-only planning queries SHALL NOT emit duplicate updates.

#### Scenario: Resume hydrates the committed Todo projection
- **WHEN** a session resumes with a committed local plan
- **THEN** its Todo resource and ACP plan notification show the same entries and statuses as the committed plan.

#### Scenario: Commit publishes one live plan projection
- **WHEN** a local plan commit updates Todo state
- **THEN** ACP receives one projection for that committed plan; the Todo tool result does not duplicate the notification.
