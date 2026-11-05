## MODIFIED Requirements

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

An effective edit to a linked parent TODO's content, requirements, priority or non-completion status SHALL record one logical objective-change responsibility per eligible directly linked unfinished child. Removal, replacement omission or cancellation SHALL instead record termination-and-wrap-up guidance for each such execution. Completion alone, repeated identical values, ordering-only changes and display/receipt/review updates SHALL remain quiet. A completed status combined with other intent edits SHALL still notify. TODO status SHALL NOT itself constitute proof of child terminal state, command termination or result acceptance.

#### Scenario: TP-06 Parent changes requirements or priority
- **WHEN** content, acceptance requirements or priority changes on a linked parent step
- **THEN** only its linked unfinished children receive Grow's fixed objective-change guidance, directing them to read the current TODO and ask the parent if clarification is needed.

#### Scenario: TP-07 Completion alone differs from completion with edits
- **WHEN** one step only becomes completed and another is edited while becoming completed
- **THEN** only the latter generates objective-change guidance; neither update fabricates execution termination or acceptance.

#### Scenario: TP-08 Reopen or cancel a parent step
- **WHEN** a step becomes pending or in_progress, or becomes cancelled
- **THEN** reopening notifies still eligible executions of changed intent, while cancellation issues fixed termination-and-wrap-up guidance; an execution already told to stop is not resumed by reopening.

#### Scenario: TP-09 No-op and presentation writes stay quiet
- **WHEN** effective fields are identical or only ordering, UI, receipts, usage or reviews change
- **THEN** no objective-change or stop responsibility or extra model check is created.

### Requirement: Replanning is child-owned and bounded by safe execution

Grow SHALL produce fixed runtime-attributed objective-change and termination messages from committed planning facts and soft-interrupt the original child's sampling or interruptible wait after durable receipt. The child SHALL handle them at the existing execution boundary. Objective changes SHALL preserve applicable work, update only affected local TODOs and direct the child to ask its parent when intent is unclear. Termination SHALL direct it to stop advancing that objective, preserve work, perform necessary cleanup and settlement, and report results, remaining work and uncertainty. Grow SHALL NOT hard-cancel the child before it can report or claim an ongoing command already stopped. Handling acknowledgements SHALL identify the exact source; receiving a message or lacking a child TODO SHALL NOT discharge the responsibility.

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

## ADDED Requirements

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

Grow SHALL inject bounded planning and delegation context into the provider request using the current committed plan and validated direct delegation. It SHALL include the linked step even when it lies outside the initial summary window, its location in the overall plan, relevant child associations and directions for todo_read. A child without a local TODO SHALL still receive its delegation context. The context SHALL be counted in the effective request budget and SHALL NOT be appended to the authored task prompt, durable Messages, Surface, ordinary chat history or unsolicited user-facing notifications.

#### Scenario: First child request locates its parent step
- **WHEN** a newly admitted or resumed child makes its first provider request
- **THEN** the request contains the current delegated step and overall position, while its user prompt remains authored text and no synthetic plan chat row is published.

#### Scenario: Repeated requests refresh plan context
- **WHEN** a later request observes a changed parent plan
- **THEN** its request context reflects the current snapshot without accumulating old planning MemoryContext items or altering actual retrieved-memory persistence.

#### Scenario: Parent is unavailable
- **WHEN** a bound child cannot read the current parent owner
- **THEN** Grow identifies the unavailable source and retained delegation identity rather than presenting a fabricated empty plan.

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
