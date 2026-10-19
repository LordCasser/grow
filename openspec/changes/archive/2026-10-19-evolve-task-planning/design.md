## Context

目标是让 Grow 在多层委派、需求调整、等待与恢复时保留清楚的工作意图。用户希望父 TODO 和子 Agent 的计划保持弱关联：改父项会要求子 Agent 重新考虑自己的计划，但父项不直接改写子 TODO，也不因版本变化强制重跑工作。

以下“当前”均为源码核对结果，“改为”均为本 change 的未来要求。核对 HEAD 为 `dd034d2dae82c03bd9715a3f19d1c17b49402a80`，产品代码基线为 `0a0dd07cd3aa35d39365ae971df8f9b316185c9d`。

### Current implementation

| 机制 | 当前事实与入口 | 本次补齐 |
| --- | --- | --- |
| TODO | `crates/codegen/tools/src/implementations/grow_build/todo/mod.rs::{TodoState,apply_replace,apply_merge,TodoWriteTool}` 在 Resources 写列表；replace 清表，未知 merge ID 可用 ID 补内容 | SessionActor 提交局部计划，稳定范围/步骤身份、有效差异及关联责任随事实保存 |
| 资源持久化 | `crates/codegen/tools/src/registry/types.rs` 的 finalize 路径调用 `persistence.rs::save`；`shell/src/session/storage/mod.rs` 保存 `resources_state.json` | TODO 不再有第二份可写权威；其他 Resources 保持原机制 |
| 委派 | `tools/src/implementations/grow_build/task/{mod.rs,types.rs}` 已有 Task/Goal/Workflow owner，尚无 TODO 引用 | task 的可选父步骤引用；不替换执行 owner |
| 嵌套 child | `task/coordinator.rs` 保存 `immediate_parent_session_id`，将 lifecycle parent 归到 root，并设 `surface_completion=false` | 保留 root 生命周期 owner，另按已验证直接委派边交付结果 |
| 等待 | `task/coordinator/query.rs` 已有事件 waiter；`task_output/mod.rs` 使用有期限的阻塞查询；`actor/completion_delivery.rs` 处理等待被 steering 打断后的交接 | 复用等待器，补上跨 turn 的显式等待意图与 child 非终态；不能把现有有限等待当作已有无限续跑 |
| 子任务终态 | `shell/src/agent/subagent/handle_request.rs` 等待一次 prompt 返回后组装结果并关闭 child；后台命令通知可能转交 parent | 增加明确的工作等待分支，避免 child 只说“等结果”就被当作完成；真实终态仍走原收尾 |
| 父子消息 | `task/coordinator.rs::{interaction_route,send_to_child_route}` 校验直接父子；`actor/notification_drain.rs::receive_agent_message` 在 active steering 接收，先查重复回执；同 identity 不能换正文 | 重用消息通道，增加类型化重规划来源与处理确认；普通消息准入不扩展到任意失活 child |
| Goal | `actor/goal.rs::{schedule_goal_on_idle,drive_goal_on_idle,start_goal_internal_turn,handle_goal_command}` 已有 owner/预算/控制栅栏与全目标审计；无真实等待 gate，外层 idle 可再次调度；complete 不核对计划义务 | 同时修改内层准入与外层重新调度，显式必需义务参与完成核对 |
| UI | `actor/turn/settlement.rs::emit_turn_end_plan_cleanup` 把 transient `in_progress` 显示成 completed；`session/acp_conversion.rs::acp_plan_update` 没有稳定步骤 ID | 真实状态投影，Grow/meta 携带稳定关联；通用 ACP 保守降级 |
| 压缩 | `actor/compaction.rs` 与 `helpers/compaction_context.rs` 从 TodoState 提取提醒 | 改读当前权威投影，保留等待/重规划/结果引用，不注入整棵树 |
| 结果 | `chat-state/src/timeline.rs::{SubagentSpawnEvent,SubagentTerminalEvent,SubagentResultEvent}` 及 subagent reconciliation 已有 durable 身份链和 immutable output 引用 | 复用原结果，补直接父级交付和验收判断，不再造 ResultStore |

表中除首行明确给出完整路径外，`tools/`、`shell/`、`chat-state/` 均位于 `crates/codegen/`。具体符号随实现可调整，行为及 owner 边界不可省略。

### Relationship to the reference research

参考仓库 `LordCasser/solaris-agent` 的 docs 核对版本为 `69df9e4b3150f18980d4eaaa4d0b694f67b0b90d`。借鉴的是局部规划、明确等待、结果交付与最终目标审计这些边界；Grow 的实际 owner、Timeline 和工具是落点。没有采用通用持久化重跑、独立规划服务或每个步骤版本都成为执行许可的设计。本文行为以本次用户确认和 Grow 代码为依据，参考材料不作为权限或实现已存在的证据。

## Goals / Non-Goals

要实现：局部计划能调整、变更不丢、等待不空转、交付可验收、恢复有依据。简单请求仍可直接完成，不要求先建 TODO；子 Agent 可以把一个父项拆成自己的多个步骤。

不建立全局任务图、自动评分系统或新的跨 session 事务服务。TODO 不是执行权限；“所有方框勾完”不是 Goal 完成证明。此阶段不实现代码，也不承诺当前版本已经具备文中能力。

## Decisions

### 1. One local plan owner, separate execution owners

每个 session 的 SessionActor 是局部计划唯一写入者，ChatState Timeline 保存可恢复事实。`todo_write` 改成类型化命令适配器；Resources 可保留只读兼容投影供既有调用方迁移，但不能继续写入另一份 TodoState。这里不要求兼容旧存储，也不顺带迁移其他工具资源。

最小信息直接附着于现有计划项、task 和 Timeline 事件：

| 信息 | 含义与 owner |
| --- | --- |
| 工作范围 | 原已接纳用户工作或 Goal identity；actor 绑定当前范围。新一条消息、工具调用或 turn 不自动新建范围 |
| 步骤引用 | `(session, work_scope, step_id)`；显示顺序不是身份，删除后不把原 ID 分配给另一项 |
| 局部计划 | 内容/要求、优先级、业务状态、可选验收要求；父子各自维护 |
| task 关联 | task 记录一个可选的直接父步骤引用及委派时来源变化标记；一个步骤可有多次 task 执行 |
| 重规划进度 | 来源变化、目标 child、不可变批次身份、待处理/已处理依据；版本仅用于差异、去重与顺序 |
| 等待与验收 | 真实执行/问题/权限身份；结果引用及父级决定。都依附当前工作，不另建通用任务数据库 |

task 的 session/owner/Goal revision 仍由 runtime 注入。模型只能选择当前范围内的步骤，不可指定任意 session owner；关联在 spawn 时绑定，不因排序、同名项或范围切换偷偷重挂接。子 Agent 继承的历史 transcript 不会复制父 TODO 的当前所有权。

`todo_write` 保持统一入口，可提交计划更新、等待意图、重规划处理确认和交付验收。等待类命令不要求存在 TODO 列表。精确 Rust/JSON 类型在实现时确定，但以上命令必须结构化，不能从 assistant 末尾文字提取。`task` 仅新增可选父步骤参数，`task_output` 保留查询/有限等待职能；不增设一组产品级 Planner 工具。

整表 replace 必须按稳定 ID 计算差异：省略旧项即明确移除，保留 tombstone 和未闭合责任；不能借 replace 躲过通知。切换工作范围必须显式处理旧范围的运行、等待和未验收交付；拒绝无处归属的直接覆盖。无需长期保留每次完整列表副本，按既有 Timeline 保留政策保存必要事实即可。

### 2. Weak links and the exact trigger

系统比较一次成功提交前后的有效字段，不用另一次模型调用判断“是否值得通知”。

| 父项变更 | 是否请求子 Agent 重规划 | 说明 |
| --- | --- | --- |
| 内容、要求/验收条件、优先级变化 | 是 | 任一有效值变化即触发；不做自然语言相似度过滤 |
| 状态改为 pending/in_progress/cancelled，或从 completed 重新打开 | 是 | 用户要求的非完成修改；重复赋同值不算变化 |
| 删除，或 replace 中省略 | 是 | 发送移除原因和原引用；先留 tombstone |
| 仅状态变为 completed | 否 | 不取消 child、不宣告其结果已验收 |
| 改内容并同时 completed | 是 | “完成”不能掩盖同批非完成修改 |
| 相同内容/状态再次写入 | 否 | 无有效差异 |
| UI 刷新、纯展示排序、统计/回执/用量/验收记录 | 否 | 不是父项任务意图的修改；若同时改验收要求，仍触发 |

关联只作用于直接 child。三种操作必须区分：父 TODO `cancelled/removed` 是任务意图变化；显式 task cancel 请求原执行 owner 停止执行；解除必需交付义务是父级验收决定，需理由及对仍活跃工作的处置。前者不隐含后两者，后两者也不能伪造已完成结果。

取消/移除通知让 child 评估停止哪些工作，并明确返回处理决定；需要立刻停止执行时仍使用显式取消控制。消息尚未送达时，child 可能继续此前已授权工作，这是弱关联的边界，不能声称已经停止。收到取消/移除的指导后，child 在下一安全边界先处理变更，再派发受该项影响的新副作用；在途外部命令不因此自动 kill。父级在仍有必需执行时不能只删除行就宣告完成。

已完成或已取消的 child 不接受新的重规划执行，不自动创建替代 child。父级看到“变更未应用于已结束执行”，自己决定接受既有结果、另发 task 或放弃交付。一个父项关联多个 child 时，分别通知仍活跃的关联执行，终态执行保留历史来源。

### 3. Durable notification and child-owned replanning

处理链：

1. 父 actor 提交计划变化，同一来源事实记录需要通知哪些已关联 child；失败就不声称计划已成功更新。task spawn 与来源变化在父 owner 下排序，保证新 child 要么拿到最新委派快照，要么形成补发责任。
   Link 的持久 ACK 返回同次绑定后的现有 LocalPlan 快照；TaskTool 只把所选父项及 session/scope/revision 附入原委派 prompt，随既有 Spawned/执行 recipe 保存。修改先提交时使用该快照，绑定先提交时由后续 Planning 事实保留原 child 的 replan 责任；仅保存 review revision 而不把要求交给 child 不构成快照交付。
2. 通过现有直接父子 inbox 投递类型化 parent guidance，包含父步骤引用、此前/最新有效要求及变化原因；正文受现有消息限额约束，长内容使用可验证来源引用，不截掉关键要求后声称处理完成。
3. **尚未形成不可变投递批次的变化合并到最新版本**。已有批次的 payload 不变，同身份重试不得改正文；已经 Received 的批次不撤销、不改写。若多个批次在同一安全边界已可消费，按原身份与顺序保留准确上下文，明确最新有效要求；child 可一次检查最新要求，并确认本次已消费且被覆盖的来源集合。不能只在发送方标记旧消息失效。child 已开始处理后才到达的新修改留作后继请求。
4. 子 Agent 在下一安全边界收到重规划指导。可打断 `task_output` 等待以便查看；不因 TODO 编辑杀掉外部测试/构建、不重新提交原 task prompt。正在运行的命令完成后再按新要求判断其产物用途。
5. 子 Agent 检查自己的计划，保留仍适用的完成项和结果，只调整受影响部分。通过同一计划入口提交局部变更与 `handled(source_change)`；若无需修改，提交带理由的 no-change 确认。没有本地 TODO 时，可明确确认直接执行仍适用，或建立所需的小计划。
6. runtime 核对确认来自正确 child、对应已消费的来源集合；局部改动与处理确认一并持久化。确认到旧版本不能清掉较新待处理版本；最新要求的一次检查可明确覆盖此前已消费变化，不能覆盖尚未消费的后继变化。无法完成检查可记录原因并保持未处理，不能仅凭消息已读或普通终态文字自动确认。

“一次更新”指每个来源变化对每个 child 的一个逻辑处理责任；允许把尚未处理的连续修改合并。它不保证故障中只有一次物理模型调用。源写入 ACK 不明时先核对原事实，投递 ACK 不明时重试原 identity，不重复变更父项或重新执行工具。

Received、Consumed、Handled 是不同边界。Consumed 继续和准确的 agent-message context 同一持久提交；合法同身份冻结恢复发现 Consumed 但未 Handled 时，通过当前未闭合工作投影继续处理，**不重新消费或重新注入同一历史消息**。该投影携带来源引用、最新有效要求和未处理状态，不复制一份新的历史收件事实。子 actor 已处理、父级还没收到确认时，核对原确认完成交接。

`Updated.plan_revision` 引用处理该变化的 child 局部计划版本。child 保存自己的 Handled 时检查它不超出本地版本；父级保存同一回执时只核对委派和来源身份，不将 child 版本与父计划版本比较。两者独立演进，child 修改次数较多不能阻断确认或清掉父级后继责任。

若 child 正常完成/取消而没有 Handled，或普通崩溃恢复已经验证旧实例结束并提交 interrupted 终态，则记录变更未应用，由父级处理原结果和新要求，不能继续唤醒该 child。尚无法确认旧实例状态时保留未确认责任；不能为清掉通知提前制造终态。这与有效冻结 checkpoint 的同身份恢复严格分开。

child 修改自己的关联步骤才继续触发 grandchild；child 确认无需改动就停止传播。根节点不直接广播给整棵树，内部完成/回执不会反向改父项，不形成通知环。

### 4. Explicit waiting without another scheduler

局部计划业务状态与执行活动分开：计划项仍可 `in_progress`，但当前执行活动是等待 task、后台 command、用户回答或权限。`in_progress` 本身不等于正在采样，也不作为自动等待推断。

等待意图绑定现有真实 ID，验证所属工作及允许关系。对多个依赖明确记录 `all` 或 `any` 条件；失败、取消、中断、结果不可用也属于可唤醒的状态变化，唤醒后由 Agent 决定下一步，不把它们当作成功依赖。不存在的 ID 立即拒绝，自等待或可证明的委派祖先循环拒绝，不静默挂住。

优先使用已存在的 task/command waiter；注册与检查当前结果在相同 owner 边界完成，防止“刚查完尚未结束，刚注册之前已结束”的丢唤醒。用户/权限等待由原请求 owner 报告，不允许 TODO 伪造一个用户已同意的回答。

普通任务与 child 可以在当前 turn 通过结构化等待意图交还执行权；模型 turn 可正常结束，runner 的工作 disposition 仍为等待，不生成最终 child result，不 shutdown 原 child。这不改变 provider terminal/PromptTurnOk 的原始证据。依赖变化后在原 owner/工作范围内继续，受原预算、max-turns、取消及权限约束。

跨 yield/wakeup 的累计用量与执行轮数由原 task/work execution owner 持有并持久恢复；每次新模型 turn 不能重置原执行的剩余额度。遵从原有已配置限制，不新增默认 Goal 预算；到限按既有到限终态/需新委派处理。不生成新 Goal、不启动第二个常驻调度器。无有效等待意图的普通 final 仍按既有完成路径处理；文字“我在等”不改变生命周期。

Goal Active 且有其他可做的已声明步骤时仍可推进；所有当前可推进工作都显式等待时才停采样。内层 continuation admission 与外层 idle 重排都检查这个 gate，直到相关事件、用户输入或有效重规划使工作重新可推进。无显式计划/等待时保留现有 Goal 模型审计，不让 host 猜测业务下一步。

用户/Stop/挂起及已接纳输入仍先于自动续跑。等待用户/权限期间，重规划通知可持久排队，不能自动回答、批准或恢复模型。停止后的迟到事件只收尾，不复活原任务。

### 5. Direct-parent delivery and meaningful acceptance

区分三个身份：root lifecycle owner、直接委派者、Task/Goal/Workflow 执行 owner。嵌套 task 的完成结果沿已经验证的直接委派边交给直接父 session；root 继续观察生命周期、汇总用量，但不会因此获得另一份要处理的模型结果。

复用 canonical child result、parent terminal reference、durable receipt。等待工具与异步通知争用同一交付身份，最多一次逻辑模型消费；查询/历史查看仍可重复。失败的结果持久化、正文缺失或父级接纳失败分别可见，不能把“进程返回”冒充“已交付”。

本 change 对普通 `task` 的嵌套委派不再以 `surface_completion=false` 抑制直接父级交付。真正无会话交付消费者的内部执行、Workflow journal 自有结果仍按原协议；不能把所有 internal child 都改成普通通知。

验收也逐级进行：child 验收自己委派的 grandchild，在自身 canonical result 中携带可验证的局部交付摘要/引用，明确尚未闭合的必需义务；root 验收其直接委派 child 的交付。root Goal 完成 gate 只检查 root 声明的必需交付及其验收决定，不把所有后代 TODO 扁平化为 root 的硬门槛。普通 Task child 即使共享 root Goal 预算也不因此获得 Goal mutation authority，局部验收只修改自己的工作事实。root 可读该摘要核对交付，而无需接收 grandchild 的第二份模型结果。失败/提前结束的 child 同样保留未闭合摘要，不能静默宣称其必需子交付都已接受。

子 Agent 交付结果后，父级对显式必需的交付物/关键里程碑作接受、需修订或取消义务的决定，引用真实结果与验证记录。结果成功返回不自动完成父 TODO。普通局部步骤不强制附证据；host 检查引用存在与必需责任已处理，业务是否满足仍由 Agent 审计。

TODO 修改不自动废弃旧结果，也不自动撤销既有验收。相关交付项显示要求已变化、需要复核，父级可以记录现有结果继续满足要求。若需求被实质修改，必须对最新要求作出复核决定，不能靠旧验收记录静默关掉必需义务；这只是验收责任，不是结果的版本匹配门禁。

父级标记 completed 不会绕开关联执行：仍在运行、尚未交付或要求变更后待复核的**必需**交付保持可见并阻止工作宣告完成，直到明确接受、需修订或有理由地解除义务；“需修订”本身仍未闭合。无需该执行时必须明确处置它及其等待责任，不能只从列表删掉。解除义务不撤销原用户目标，Goal 的全目标审计仍必须成立。

同一父项多个 child 可以比较、组合或重试，不要求所有结果都采纳。被放弃的执行要明确取消，或转为不阻塞当前完成的后台责任并保留 owner；不能留隐形副作用。worktree 结果还须核对目标工作区是否实际集成并验证；计划本身不新增自动 merge 或全局文件互斥。

### 6. Interaction boundaries

| 现有机制 | 交互与用户可见行为 |
| --- | --- |
| Goal definition revision | TODO 变化不改 Goal revision、不撤销已准入子任务权限；真正改 Goal 定义仍遵守原 revision fence，旧 Goal 结果不自动作为新 Goal 的唤醒 |
| Goal blocked/paused/预算 | 等待不是 blocked audit 的一轮，也不是 pause；预算/显式暂停关闭准入时只保留责任，不因重规划/结果到达重新打开 |
| 全目标完成审计 | `update_goal(complete)` 核对当前必需交付、有效等待、影响必需交付的待处理变更及必要收尾；逐级交付摘要支持语义审计。禁止只按局部 TODO 勾选完成，无关子步骤不升级为门禁 |
| Plan 审批 | Plan artifact 的审批/执行阶段独立；活动 Goal 与 Plan 的互斥保留。执行细化不自动修改已批准 artifact；真正越出批准范围仍走原审批 |
| `retain-plan-for-later-review` | 单独未完成 change；本次不存储待审批 Plan、不实现跨 Goal 切换候选 |
| Workflow | Run/journal 仍掌握执行权；TODO 是说明与局部进度，不驱动 Rhai。不得借弱关联绕过 Workflow 的固定快照与父子通信限制 |
| Scheduler | 仍处理时间型任务；依赖就绪用现有 actor/coordinator 事件，不转成反复定时模型轮询 |
| 权限与用户问题 | 父项更新是 agent guidance，非人类输入、权限证明或一次性 permit；工具调用继续经过原 Gate |
| 用户 steering | 先处理控制和已接纳输入，之后重检计划、Goal 和等待；旧 continuation 不跨新要求执行 |
| 父 child messages | 重用 Received/Consumed 与直接边；失活节点只允许既有回执核对，自动重规划不扩大普通 Send 的准入 |
| 冻结恢复 | 依赖 `suspend-session-tree`。整个树先恢复身份/关联/等待/未处理变更，再开放允许的执行；暂停的 Goal、awaitUser 与权限等待原样保留 |
| 中断命令 | 已完成产物保留；Interrupted/OutcomeUnknown 唤醒“需检查现场”的工作，禁止只因 TODO 仍 in_progress 自动重跑命令 |
| 冷崩溃恢复 | 没有有效冻结 checkpoint 时仍走既有 orphan/interrupted 收尾；本 change 的计划投影不赋予自动复活旧 child 的权力 |
| fork/rewind/`resume_from`/export/replay | 历史只作来源；不能因复制 TODO 文字或回退 UI 获得原执行/通知所有权。新 child 若关联步骤须重新由当前调用者验证 |
| 资源回收 | 未交付变更、未处理确认、等待与结果验收的必要引用参与既有保留规则；处理/覆盖确认/取消/终态不可应用后可收敛，不永久保留无主责任 |

### 7. UI and context

Grow 投影携带 session、工作范围、步骤、关联 task、等待依据和结果状态。用户能看到“运行中”“等待子任务/命令/用户”“已交付待验收”“需复核”“完成/取消”；这些是多个事实的展示，不必扩成一个巨大状态枚举。允许展开 child 的自己的计划，不合并成 root 的同一张表，也不把子项数量换算成虚假完成百分比。

移除 turn-end 假完成投影。通用 ACP 若只能表达 pending/in_progress/completed，则等待仍保留 in_progress 并提供可用说明；不能为清掉 spinner 编造 completed。Pager、headless、resume 和只读 replay 都遵从同一身份与已交付事实。

模型每步只需当前局部计划、直接关联变化、等待和待验收结果摘要及必要引用。已完成细节可压缩；未处理的重规划不能随 summary 消失。异步压缩在发布和下一步准入时重检当前投影，历史摘要不能覆盖新事实。读页面、query、export、replay 或做压缩本身不触发重规划执行。

## Failure and Recovery

| 竞争/失败 | 必须保留的结果 |
| --- | --- |
| 父项提交失败或 ACK 不明 | 不声称更新成功，不先发“已更新”指导；按原身份核对确认后补投递 |
| 父项已提交，通知尚未送出就退出 | 来源事实可重建未交付责任；无需跨两个 session 原子事务 |
| 重规划处理中又改父项 | 旧确认只关闭旧批次，新变化继续待处理，不用同 ID 换正文 |
| 投递时 child 完成/取消 | 保留结果和变更，记录未应用及原因；不启动新 child |
| child 已消费，提交计划/确认前冻结 | 恢复未闭合工作，不重复消费消息；不能断言物理模型只运行一次 |
| child 已确认，父级 ACK 丢失 | 对账原确认，同一义务关闭一次，不强制 child 再规划 |
| 注册等待时结果已到达 | 立即评估就绪，不丢唤醒、不重复模型消费 |
| 依赖失败/取消或命令结果未知 | 显式交付状态，解除无意义等待并进入检查/决策，不当成功 |
| 通知/存储容量耗尽 | 保留已提交责任并施加有界反压；拒绝无法持久接纳的新写入，不静默丢变更、不无限排模型轮次 |
| 恢复遇到缺失身份/损坏引用 | 显示无法确认，停止相应自动处理；保留证据，不猜测引用或执行命令 |

## Implementation Order

1. 先落局部计划命令/Timeline 权威和投影，再接稳定 task 关联及有效差异。
2. 接重规划交付、合并、处理确认与恢复核对；此时即可验证弱关联核心闭环。
3. 修直接父级结果路由，接明确等待与 runner 非终态，再接 Goal 准入及必需交付验收。不能只改 prompt 避免轮询。
4. 更新 UI/压缩/文档；在 `suspend-session-tree` 完成后进行树级冻结联合验收。

每阶段验证失败都保留未完成任务，不因静态规范通过而勾选运行验收。所有实现和运行场景在 [tasks.md](tasks.md)、[acceptance.md](acceptance.md) 中列出；本次只完成文档准备。
