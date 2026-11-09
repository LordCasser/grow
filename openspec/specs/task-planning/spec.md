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

Ordinary model-originated task delegation, including resume_from, SHALL require one explicit parent_step_id in the caller's current local work scope. The step SHALL exist, be unremoved and have pending or in_progress status. The runtime SHALL validate and durably bind the direct owner before dispatch. One parent step MAY support multiple distinct executions. This relation SHALL NOT transfer Task/Goal/Workflow execution ownership, permissions, or result authority. Internal Workflow-owned delegation SHALL continue using its own admitted owner without creating a fictitious TODO. Parent edits and spawn SHALL be ordered so a child receives current guidance or retains a definite later-delivery responsibility.

#### Scenario: TP-04 Multiple executions support one step
- **WHEN** a current parent step delegates investigation, verification and a later retry
- **THEN** each execution has its own identity and the same parent step reference, and results remain independently reviewable.

#### Scenario: TP-05 Spawn races with a parent edit
- **WHEN** task admission races with editing or removing its linked step
- **THEN** dispatch observes the latest valid binding or the child receives the admitted binding with a durable change/stop delivery responsibility; it cannot execute stale requirements without notice.

#### Scenario: Ordinary delegation lacks an eligible TODO
- **WHEN** a model spawn or resumed delegation omits its parent step, selects an unknown or removed step, or selects a completed or cancelled step
- **THEN** it is rejected before child, worktree or provider admission with an actionable request to select an eligible current TODO.

### Requirement: Effective non-completion edits request replanning

An effective edit to a linked parent TODO's content, requirements or priority, or reopening a completed step SHALL record one logical objective-change responsibility per eligible directly linked unfinished child. Removal, replacement omission or cancellation SHALL instead record termination-and-wrap-up guidance for each such execution. Completion alone, progress-only transitions between pending and in_progress, repeated identical values, ordering-only changes and display/receipt/review updates SHALL remain quiet. A completed status combined with other intent edits SHALL still notify. TODO status SHALL NOT itself constitute proof of child terminal state, command termination or result acceptance.

#### Scenario: TP-06 Parent changes requirements or priority
- **WHEN** content, acceptance requirements or priority changes on a linked parent step
- **THEN** only its linked unfinished children receive Grow's fixed objective-change guidance, including the revised step and requirements. The child uses the delivered payload and reads or asks only for details it needs to make a decision.

#### Scenario: TP-07 Completion alone differs from completion with edits
- **WHEN** one step only becomes completed and another is edited while becoming completed
- **THEN** only the latter generates objective-change guidance; neither update fabricates execution termination or acceptance.

#### Scenario: TP-08 Reopen or cancel a parent step
- **WHEN** a completed step becomes pending or in_progress, or a step becomes cancelled
- **THEN** reopening notifies still eligible executions of changed intent, while cancellation issues fixed termination-and-wrap-up guidance; an execution already told to stop is not resumed by reopening.

#### Scenario: TP-09 No-op and presentation writes stay quiet
- **WHEN** effective fields are identical or only ordering, UI, receipts, usage or reviews change
- **THEN** no objective-change or stop responsibility or extra model check is created.

#### Scenario: Parent updates only progress
- **WHEN** the linked step changes between pending and in_progress with identical content, requirements and priority
- **THEN** the plan advances without creating a control or a new acknowledgement obligation.

### Requirement: Replanning is child-owned and bounded by safe execution

Grow SHALL produce fixed runtime-attributed objective-change and termination messages from committed planning facts and soft-interrupt the original child's sampling or interruptible wait after durable receipt. The child SHALL handle them at the existing execution boundary. Objective changes SHALL preserve applicable work, update only affected local TODOs and direct the child to ask its parent when intent is unclear. Termination SHALL direct it to stop advancing that objective, preserve work, perform necessary cleanup and settlement, and report results, remaining work and uncertainty. Grow SHALL NOT hard-cancel the child before it can report or claim an ongoing command already stopped. Handling acknowledgements SHALL identify the exact source through the structured plan action. Routine handling SHALL NOT require a separate prose confirmation or parent message; receiving a message or lacking a child TODO SHALL NOT discharge the responsibility.

#### Scenario: TP-10 Child changes only affected work
- **WHEN** a parent adds a test requirement after investigation was completed
- **THEN** the child preserves that investigation, changes remaining implementation or verification as needed, and acknowledges the exact change.

#### Scenario: TP-11 Existing plan still applies
- **WHEN** the child determines objective-change guidance is already covered by its current work
- **THEN** it may acknowledge with a reasoned no-change decision without inventing TODO edits; this decision cannot revoke termination guidance.

#### Scenario: Stop handling records acceptance without claiming completion
- **WHEN** a child durably consumes exact termination guidance and acknowledges it through HandleReplans
- **THEN** handling records the stopping outcome as acceptance of wrap-up, rejects an ordinary no-change decision for that stop, and leaves execution, commands, results and required delivery obligations to their existing completion owners.

#### Scenario: TP-12 Parent edit arrives during a command or wait
- **WHEN** objective-change or termination guidance arrives while an external command or interruptible wait is running
- **THEN** sampling or the wait yields at its safe interruption point, existing command/result ownership remains truthful, and the child handles the fixed instruction before advancing affected new work.

#### Scenario: TP-13 Child cannot finish the check
- **WHEN** the child cannot determine how the new objective applies
- **THEN** it keeps the exact unresolved responsibility and can ask its parent; ordinary prose or message consumption does not count as handling.

### Requirement: Replanning updates coalesce without mutating delivery identity

Changes SHALL deduplicate by source and child. Undispatched ordinary objective updates MAY coalesce. A stop responsibility SHALL NOT be erased by coalescing, reopening or a newer ordinary edit. Claimed or received message identity and payload SHALL remain immutable. Multiple consumed batches SHALL retain precise provenance and order; a handling acknowledgement SHALL discharge only its cited sources, never a newer pending change.

#### Scenario: TP-14 Rapid edits coalesce
- **WHEN** three ordinary edits arrive before a child handles them
- **THEN** undispatched updates may coalesce and immutable batches remain intact, with a bounded check of the latest effective objective and exact source acknowledgement.

#### Scenario: TP-15 Edit arrives during replanning
- **WHEN** the child handles A while B arrives and A's acknowledgement is late or retried
- **THEN** A's acknowledgement affects only A and B remains pending without state rollback.

#### Scenario: Stop is followed by reopening
- **WHEN** a linked step is cancelled or removed and a later edit attempts to reopen or repurpose the work
- **THEN** the original execution retains its stop-and-report responsibility and only a separately admitted execution may take the new work.

### Requirement: Link changes and terminal executions converge explicitly

Removal or replacement omission SHALL preserve the original step tombstone, delegation links, stop delivery and unclosed waiting/review responsibilities. Step identity SHALL NOT be reused. Scope switching SHALL explicitly dispose of prior obligations. Terminal children SHALL NOT be revived by plan edits; unapplied changes remain visible to the parent. Controls SHALL follow direct delegation edges; a parent SHALL NOT write its child's local TODO list by editing its own step.

#### Scenario: TP-16 Replacement removes a linked step
- **WHEN** replacement omits a step with unfinished children or its ID is reused after removal
- **THEN** the tombstone and original links remain, reuse is rejected, and each unfinished execution receives fixed termination-and-wrap-up guidance.

#### Scenario: TP-17 Child finishes before receiving an edit
- **WHEN** completion races with parent change or removal
- **THEN** the original result and terminal state remain, unapplied control is visible and no child is restarted.

#### Scenario: TP-18 Replanning propagates through local edits
- **WHEN** root changes its linked child step and the child adjusts its own grandchild-linked TODO
- **THEN** that local edit controls its own linked grandchild; root does not broadcast beyond its direct edge and unchanged unrelated steps stay quiet.

#### Scenario: TP-19 New work cannot erase old obligations
- **WHEN** a scope switch still has undisposed executions, waits, changes or required deliveries
- **THEN** the switch is rejected with concrete references and the original obligations remain recoverable.

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

显式必需的委派交付物和关键里程碑 SHALL 区分执行完成、结果交付和父级验收；验收 SHALL 引用可验证结果及适用验证记录。TODO 修改 SHALL NOT 自动作废旧结果或要求所有后代步骤提供证据。交付要求变化后 SHALL 保留原验收并要求明确复核；父级可接受仍满足最新要求的既有结果。需修订不等于义务已闭合。删除或取消 TODO SHALL 发起关联执行的终止收尾指导，不得伪造已停止、已交付或已验收。

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
- **THEN** 系统投递终止收尾指导，父级仍需明确记录取消或解除必需义务及理由，处置原等待并保留仍存在的执行 owner；不得靠删行隐藏继续运行的副作用。

#### Scenario: TP-30 Result was produced in another worktree
- **WHEN** child 在独立 worktree 交付修改，而父级工作需要在目标工作区生效
- **THEN** 验收核对集成与验证依据，未集成不能冒充目标工作已完成；系统不因 TODO 完成自动 merge。

#### Scenario: Structured final output obeys planning obligations
- **WHEN** StructuredOutput is accepted while a required deliverable, pending replan, or released live obligation remains
- **THEN** the turn returns to the model with the same concrete completion blockers as an ordinary final answer; it does not finalize the turn.

#### Scenario: TP-32 Parent removes a step while its command is running
- **WHEN** 父项被取消/移除，关联 child 正运行外部命令且通知暂不能送达
- **THEN** 记录终止收尾责任及执行仍可能继续的事实，不声称命令已停止或必需义务已解除；送达后在安全边界停止推进原目标，完成必要结算并报告，不需要父另发一条普通控制消息来表达终止意图。

### Requirement: Committed local plans have one live and resumed projection

ACP plan notifications and Todo resources SHALL project the committed Timeline plan through the existing Todo success conversion. Session startup hydration and each plan commit SHALL publish that projection; read-only planning queries SHALL NOT emit duplicate updates.

#### Scenario: Resume hydrates the committed Todo projection
- **WHEN** a session resumes with a committed local plan
- **THEN** its Todo resource and ACP plan notification show the same entries and statuses as the committed plan.

#### Scenario: Commit publishes one live plan projection
- **WHEN** a local plan commit updates Todo state
- **THEN** ACP receives one projection for that committed plan; the Todo tool result does not duplicate the notification.

### Requirement: Agent replacement preserves session planning access

同一 session 更换 Agent 后，仍允许的 TODO 工具 SHALL 继续访问该 session 的当前局部计划，并通过原 session owner 提交更新。替换 SHALL 保留已提交步骤身份与状态，不产生新工作范围，不从历史文本重建；不能因更换工具运行环境导致 Planning runtime unavailable。计划读取失败 SHALL 阻止未准备完成的 Agent 替换。

#### Scenario: Switch Agent before the first TODO
- **WHEN** session 切换到提供 TODO 工具的 Agent，随后在已接纳工作中首次提交 TODO
- **THEN** 工具通过原 session owner 成功提交，并产生对应 ACP plan 投影。

#### Scenario: Switch Agent with an existing plan
- **WHEN** session 已有包含完成及未完成步骤的局部计划并切换 Agent
- **THEN** 新 Agent 读取同一 scope、步骤身份及状态，后续合并更新保留未修改步骤；替换本身不提交新计划。

实施入口：`crates/codegen/shell/src/session/actor/{planning.rs,model_switch.rs,spawn.rs}`。

### Requirement: Scope switches accept explicitly disposed dispatch failures

切换工作范围时，actor SHALL 使用 `LocalPlan::validate_transition_from` 的既有 transition 规则判断旧范围义务。明确记录的未派发事实可处置可选 link；必需 link 还必须有带理由的 Released review。活动或结果未知的 link、等待、未处理 replan，以及未处置的必需交付 SHALL 继续阻止切换。失败不得将执行伪造为 terminal 或生成结果。

#### Scenario: Optional dispatch failure is disposed without review
- **WHEN** 可选 child 明确未派发且没有等待或其他旧范围义务
- **THEN** 无需交付 review 即可切换 scope，link 仍保留为非 terminal 且无结果的历史事实。

#### Scenario: Required dispatch failure is released
- **WHEN** 必需 child 明确未派发并通过带理由的 Released review
- **THEN** 可以切换 scope，未派发事实不被改写为完成结果。

#### Scenario: Required dispatch failure is not released
- **WHEN** 必需 child 明确未派发但没有 Released review
- **THEN** scope switch 被拒绝，旧计划保持不变。

#### Scenario: Active or unknown execution is not disposed
- **WHEN** link 没有明确 dispatch failure，且仍活动或结果未知
- **THEN** scope switch 被拒绝，link 保持非 terminal 且无伪造结果。

### Requirement: Dispatch failures close child replanning responsibilities

明确记录 dispatch_failure 的关联 SHALL NOT 被父项后续修改重新赋予 child 确认责任。来源更新 SHALL 保留为带原失败原因的 Unapplied，关联保持非 terminal 且不生成结果。既有未闭合 replan SHALL 可依据同一失败事实由父 owner 持久闭合；恢复投影前 SHALL 完成该闭合，重复恢复 SHALL 幂等。未派发事实不自动解除必需交付验收，Released 不单独证明活跃或未知执行已结束。

#### Scenario: Parent edits a step after child startup failure
- **WHEN** child 在启动准入前失败且已记录 dispatch_failure，随后父 TODO 的要求、优先级、非完成状态或移除发生变化
- **THEN** 来源变化记录为 Unapplied，pending_source_revision 保持为空，不等待无法运行的 child；必需交付仍须明确 Released。

#### Scenario: Resume a released dispatch failure with stale replans
- **WHEN** 恢复的计划包含 dispatch_failure、Released 和遗留 Pending 或 Dispatching replan
- **THEN** 父 owner 在投影前持久闭合这些 replan，保留来源、失败原因及非 terminal、无结果事实，完成检查和 scope 切换可以通过，重复恢复不再提交。

#### Scenario: Repeat the original failure acknowledgement
- **WHEN** 同一 dispatch_failure 再次提交，但关联上仍有遗留未处理来源
- **THEN** 闭合遗留来源；已经闭合时幂等，不改写不同的失败原因。

#### Scenario: Released live or unknown child retains its obligations
- **WHEN** 关联没有 terminal 或 dispatch_failure 证据，父级只提交 Released
- **THEN** 执行和重规划义务仍可见并阻止完成，不借恢复清理伪造终态。

### Requirement: Unchanged completion blockers stop automatic sampling

同一模型循环 SHALL 至多为完全相同的完成阻塞注入一次处理提示。下一次完成尝试若阻塞未改变，SHALL 返回包含具体阻塞的 planning_completion_blocked 错误并停止模型采样和 Agent completion recovery。该错误 SHALL NOT 宣告工作完成；Goal 内部回合 SHALL 沿现有错误路径暂停自动续跑。新用户回合可以再次处理原义务。

#### Scenario: Required obligation cannot be resolved by the current agent
- **WHEN** 首次完成检查提示待处理义务，代理检查或调用工具后，相同 blockers 在下一次 final 仍存在
- **THEN** 本轮停止并保留具体义务，不再以相同提示持续调用模型，即使没有配置 max-turns。

#### Scenario: Structured output encounters the same unresolved obligation
- **WHEN** StructuredOutput 的两次完成尝试之间 blockers 完全相同
- **THEN** 使用普通 final 的同一停止规则，不发布成功完成。

#### Scenario: Agent resolves the obligation after the reminder
- **WHEN** 提示之后代理合法解除阻塞，再次尝试完成
- **THEN** 正常完成；改变阻塞集合或合法依赖等待仍沿原处理流程推进。

### Requirement: Delivery review bindings are actionable in model projections

模型可见的未闭合必需交付摘要 SHALL 提供与 review 输入同名的准确 linkId、resultId、reviewedSourceRevision，保留 resultId 的完整字符串或 null；request-only 规划 link 投影 SHALL 使用相同绑定。提示 SHALL 说明复制绑定后仍需明确选择验收决定和依据，不把 task id 或单独 seq 当成 resultId。既有有界摘要可分批显示，不能因展示截断免除剩余义务。

#### Scenario: Review a delivered result from the Todo summary
- **WHEN** 必需 child 已交付，模型从 todo_write 摘要复制绑定并附上合法 accepted 决定及验证依据
- **THEN** 原 actor 可持久接受该 review，关闭对应交付阻塞；恢复后仍保留该准确验收。

#### Scenario: Release a terminal delivery with an incorrect result identity
- **WHEN** released 携带 null 或单独 seq，但 link 已绑定不同的完整 resultId
- **THEN** actor 在持久提交前拒绝并返回该 link 可复制的准确绑定，原计划保持不变；用准确绑定和理由重试可以正常释放。

#### Scenario: Release a task without a delivered result
- **WHEN** task 尚无结果，模型复制含 null resultId 的绑定并提供 released 理由
- **THEN** 不要求虚构结果，沿原执行和等待处置规则提交；活跃执行仍可阻止完成。

#### Scenario: Locate a review beyond the bounded summary
- **WHEN** 有多于摘要上限的交付，模型对未展示 link 提交错误绑定
- **THEN** 错误回执返回请求指定 link 的准确绑定，不能返回另一项或让模型继续猜测身份。

### Requirement: Released deliveries accept later canonical results

A historical Released review recorded before a result exists SHALL remain valid when that link later receives a canonical terminal result. Its absent result reference SHALL preserve the fact that release preceded delivery and SHALL NOT be rewritten to cite the later result. Released reviews with a result reference, Accepted reviews and NeedsRevision reviews SHALL retain exact result binding. Release SHALL NOT fabricate execution completion, a result reference or acceptance. Later terminal publication SHALL continue through canonical child-reference validation and existing pending-replan disposition.

#### Scenario: Result arrives after release
- **WHEN** a parent releases a live required delivery with no result reference and a valid canonical result arrives later
- **THEN** the result can commit to that link, the historical release remains unchanged, and pending parent updates are settled by the existing terminal transition.

#### Scenario: Release leaves an unknown execution unfinished
- **WHEN** a delivery is released while execution or result finalization remains unknown
- **THEN** release alone does not mark its link terminal, clear pending replans or satisfy the execution-disposition completion gate.

#### Scenario: Referenced review still requires an exact result
- **WHEN** an Accepted or NeedsRevision review lacks a terminal result, or a review supplies a mismatched result reference
- **THEN** plan validation rejects it; allowing a historical unbound release does not admit that invalid review.

### Requirement: Native TODO reads use trusted local and direct-parent ownership

The native todo_read tool SHALL provide self and immediate-parent plan snapshots with stable TODO identities, delegation links, source revisions, waits, control progress and delivery reviews. It SHALL NOT accept an arbitrary session identity or use lifecycle-root visibility as parent authority. Reads SHALL not create or mutate plans, advance revisions or emit plan-update notifications. Unknown ownership and unavailable owners SHALL remain distinguishable from no parent or no plan. Pagination SHALL be bounded, bind each cursor to the authorized snapshot and query, and reject a stale cursor rather than mix revisions.

#### Scenario: Parent and child inspect task position
- **WHEN** the parent reads its own TODO and a child reads target parent
- **THEN** both observe the same committed step/link identities and the child's delegated step within the overall plan, without changing either plan.

#### Scenario: Nested child cannot select a different owner
- **WHEN** a grandchild reads parent or supplies a forged cursor or unrelated session identity
- **THEN** only its immediate parent's plan can be read; invalid or stale selection fails without granting root or sibling access.

#### Scenario: Plan changes between pages
- **WHEN** a plan commit occurs before the next cursor page is read
- **THEN** the old cursor is rejected as stale and the caller can restart from a coherent snapshot.

### Requirement: Planning context is constructed by Grow without user-chat insertion

Grow SHALL inject bounded planning and delegation context into the provider request using the current local plan and one initial direct-parent observation per foreground turn. It SHALL include the linked step even when it lies outside the initial summary window, its location in the overall plan, relevant child associations and access to todo_read for specific missing facts. The initial parent snapshot SHALL be presented as background and SHALL NOT instruct the child to poll, confirm or report on the plan. Ordinary change obligations SHALL be actionable from delivered guidance. A child without a local TODO SHALL still receive its delegation context. The context SHALL be counted in the effective request budget and SHALL NOT be appended to the authored task prompt, durable Messages, Surface, ordinary chat history or unsolicited user-facing notifications.

#### Scenario: First child request locates its parent step
- **WHEN** a newly admitted or resumed child makes its first provider request
- **THEN** the request contains the current delegated step and overall position, while its user prompt remains authored text and no synthetic plan chat row is published.

#### Scenario: Repeated requests refresh plan context
- **WHEN** the parent edits its plan after the child has built its first request in that foreground turn
- **THEN** subsequent request builds reuse the same parent observation without a new parent query. The child receives effective changes through typed notifications and the request still projects the current local plan. No planning MemoryContext items are accumulated and retrieved-memory persistence remains independent.

#### Scenario: Parent is unavailable
- **WHEN** a bound child cannot read the current parent owner
- **THEN** Grow identifies the unavailable source and retained delegation identity rather than presenting a fabricated empty plan.

#### Scenario: Repeated snapshots during unchanged work
- **WHEN** the child makes multiple requests without a new parent change notification
- **THEN** each snapshot provides background only. A parent progress or delivery field does not create a read, acknowledgement or reporting instruction.

### Requirement: Planning controls have internal runtime provenance and durable receipt

Fixed TODO objective-change and termination guidance SHALL be generated by the host from a verified committed source change, with exact parent/link/step/revision identity. Model-authored messages SHALL NOT forge that source. Delivery SHALL use durable Received/Consumed/Handled facts and the existing soft-interruption boundaries. Internal control payloads SHALL NOT generate ordinary coordination chat notices in live or replay surfaces. Sending, receipt, handling and actual execution completion SHALL remain distinct outcomes.

#### Scenario: Deleted TODO requests report before child closure
- **WHEN** the parent removes an unfinished child-linked TODO
- **THEN** Grow sends fixed stop-and-wrap-up guidance and safely interrupts sampling or waits so the child can settle and report before its canonical result closes the Timeline.

#### Scenario: Duplicate or lost acknowledgement
- **WHEN** the same source control is retried after its receipt acknowledgement is lost
- **THEN** exact identity/payload verification returns the existing receipt without duplicating the instruction, side effects or handling result.

#### Scenario: Guidance receipt is interrupted between durable prefixes
- **WHEN** the child durably records its exact planning responsibility and failure occurs before its notification receipt is confirmed
- **THEN** the responsibility remains unresolved, the parent retries the original immutable delivery identity, and recovery fills the missing receipt before soft interruption or a received acknowledgement; it does not discard the responsibility or restart the execution.

#### Scenario: An ordinary receipt does not acknowledge a typed control
- **WHEN** an ordinary parent message with a control's sender and message identity was consumed but the actual host planning-guidance receipt was not
- **THEN** the ordinary receipt neither collides with that typed control nor proves its consumption; handling remains unavailable until the exact typed receipt is consumed as model context.

#### Scenario: An unresolved record does not starve independent controls
- **WHEN** an earlier historical Handled record lacks exact typed consumption evidence and later records contain an authorized handled acknowledgement or a pending control for a direct child
- **THEN** the earlier record remains unconfirmed without a fabricated acknowledgement, while the delivery scan continues to those independent later records and retains each unresolved responsibility for retry.

#### Scenario: User-authored message mimics a control
- **WHEN** ordinary message text copies the fixed control wording or attempts to claim a plan source
- **THEN** it remains ordinary message data and cannot acquire typed plan-control authority or hide its own user-visible message provenance.
