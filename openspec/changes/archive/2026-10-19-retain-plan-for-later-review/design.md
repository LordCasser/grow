## Context

Grow 已有不可变计划正文、revision/hash、完整 BehaviorSnapshot 的审批 CAS、恢复后重建 waiter 和无活跃 turn 的 PlanHandoff。现有四阶段由 BehaviorCoordinator/SessionActor 持有，pending registry 本身不持久化。

静态差异：Pager 无审批 view 时读取 legacy plan.md；select_behavior/finish_plan 重置 PlanRuntime。磁盘存在 blob 不等于候选仍可发现或有执行资格。上游 KeptPlan/after-turn review 提供 UX 参考，但默认 gate 关闭；不能只复制 ExecutePlan 按钮。

## Goals / Non-Goals

目标：一会话一个当前候选，可收起、明确保留、退出后找回、重新审阅，再批准一次执行。版本变化和未知 ACK 不产生错误审批或重复执行。

非目标：重做四阶段/恢复执行、多计划库、独立 scheduler、自动 Rhai 生成、Goal 激活、长期预授权、布局 Dock 化。用户之后审阅并批准才决定执行，不是现在批准未来自动跑。

## Decisions

### Reuse Grow mechanisms, add only retained-plan semantics

此 change 主要是 Grow 自身的产品协议，没有需要引入的外部计划执行框架。已经存在的四阶段、immutable artifact/hash 与大小校验、Behavior CAS、Timeline/Control 提交、恢复审批、ResampleStep/PlanHandoff 和 GC 所有权必须复用；不重新实现存储、幂等服务、调度器或计划执行器。

新增仅限候选跨交互生命周期的引用/资格、defer 决定及收尾、稳定决定身份、canonical 查询与 UI 入口。读模型与决定接到已有会话控制入口，不为收起/保留/打开/批准各造一个模型工具；UI 收起仍是本地状态。正文读取复用标准库/既有 artifact helper 和异步 Runtime，不另引数据库、文件索引或内容存储。候选提交身份区分计划周期，artifact hash 证明正文，决定身份用于提交核对，三者不互相代替，也不各自成为独立 owner。

### Same owner, separate artifact and presentation

内容版本、执行协议阶段和 UI 展开状态分离。收起是 UI 状态，不增加第五 PlanPhase。保留候选引用由同一 SessionActor/Timeline owner 持久接纳，UI 缓存和可变 plan.md 不成为权威。

approval_pending 是已有 transport/恢复 gate，不把它直接改成“所有保留文档都待审批”。明确保留操作先持久化候选资格，再释放当前 waiter；不得通过伪造 reject、approval transport error 或成功批准结束等待。解除 live tool-loop 的确切 typed disposition 需在实现时接入现有 actor，不能靠 UI 直接结束 turn。

保留成功后 `approval_pending=false`，没有可执行的 Execute/Revise handoff；live 路径为原 plan_control 结算一次明确的延后结果，并沿已有 EndTurn 语义结束本次计划交互，不 ResampleStep、不重新采样并弹回审批。执行中 amendment 的保留也不恢复执行旧正文。idle 路径只提交保留事实并结束 waiter。若候选/控制提交失败或 ACK 未确认，不先销毁 waiter 或宣称保留成功；若提交成功后进程崩溃，恢复依据保留事实和原工具终态补齐合法收尾，不自动重建已延后的审批。再次审批由明确的审阅/执行动作重建，普通查看不建 waiter。

当前 `BehaviorSnapshot::runtime_fields_match_selection` 禁止非 Plan 携带 Plan runtime 字段，`select_behavior`/`finish_plan` 会清空这些字段。因此跨 Behavior 保留需在同一 SessionControl/Timeline 提交中增加生命周期独立的可选候选引用；不能只停止清空原字段或放宽全部 snapshot 校验。候选引用与 Plan phase 仍由一个 owner 原子变更，不另建文件索引或持久 registry。实现前明确 Control architecture/schema 版本和恢复校验，同步所有 Control 消费者；不增加旧格式兼容分支。

保留候选可以跨离开 Plan 保持只读可见；进入其他工作遵循现有 Behavior switch。重新执行候选必须重新核对当前 owner、重建审批关系，并经既有仲裁；候选不自动抢占其他 turn/Goal/Workflow。

### Canonical read and immutable revisions

/view-plan 与所有入口通过后端候选引用解析 immutable artifact，校验大小/hash/UTF-8，显示审阅版本。没有 legacy plan.md 也应可查看。缺失/损坏显示不可用，不从旧消息拼凑“相同”正文。

复用既有有界异步读取；Pager render/按键路径不做同步文件读取。返回值携带 session、UI binding/查看请求身份和候选提交版本，迟到结果不能覆盖切换后会话或新版预览。评论绑定原审阅版本，候选替换时保留并标记过期，不自动挂到新正文相同行号。

关闭预览或接收新的真实审批时使旧查看请求失效，避免迟到正文重开已关闭窗口或覆盖正在审批的版本。评论复用既有 artifact preview 快照保存 session/binding、submission、revision/hash 和正文；恢复同一审批时沿用评论，不以同 hash/revision 推断跨周期身份相同。新审批只接纳匹配身份的评论，旧版本评论仍保留且不渲染在新正文上。

初始被要求修改的计划回 Drafting 后 submit；执行中修改进入 Amending，新的正文形成新的版本后重审。仅收起/重新打开不增版本；反馈若改变实施要求必须形成新正文，不能悄悄更改已批准内容。

用明确的“要求修改”动作表达实施要求变化，不让后端或模型猜测一段批准评论是否改变契约。批准可携带不修改正文的说明；修改内容必须走修订动作和新提交，不能靠批准 feedback 覆盖冻结 artifact。

### Decision identity and handoff

绑定 session、候选提交身份、revision/hash、稳定决定身份以及当前请求/控制 owner。后端冻结快照 CAS 决定唯一获准版本，旧 UI 和双客户端竞争应得到失效/冲突结果。

现有 PlanRuntime 重置会使 revision 不能被假定为全会话唯一；跨周期重新绑定同正文必须有可区分的提交/决定身份。优先复用已有 durable event identity，必要时采用会话单调提交序号；不新建 workflow owner。

live approve 沿当前 ResampleStep，idle approve 沿 durable Control → PlanHandoff → 原通知/输入仲裁。接纳失败保留候选和评论，ACK 未确认核对原身份，不换新 prompt 重发。handoff 已消费不重复执行，旧周期 handoff 不作用于新候选。

稳定决定身份绑定不可变 action、候选和 feedback；同身份同内容返回原接纳状态，同身份不同内容在改变状态前拒绝。状态查询先于原决定入队完成时，“暂未找到”仍是未确认；明确未接纳需要原操作拒绝或旧 writer 完整退出后由合法 owner 核对，不能照抄输入接口却遗漏这个屏障。沿用现有 Control 提交和查询模式，不把决定编码成 HumanIntent 来获得幂等。

候选的替换/放弃与批准共享同一串行化边界；先解决未确认决定，不能用新候选覆盖掉它。已执行或放弃的提交不再有当前批准资格，历史正文可保留；再次执行须新提交并重审。正在执行的冻结引用不因另一份预览或 UI 评论变化而被改写。

保留候选预览打开时，其可见的 `r review` 是该 surface 的显式动作；残留的行为切换确认不得在根输入路由中先吞掉此键。Review 仍通过现有 session binding、候选 submission 和稳定 decision ID dispatch，后端准入/持久化拒绝继续显示为未确认或拒绝，不允许客户端乐观打开审批。

### HumanIntent, Goal and Rhai boundaries

审批按钮响应已绑定控制请求；普通用户文字仍走原 HumanIntent 接纳。新“稍后执行”入口携带稳定决定身份，不同时发审批回答和普通 Interject。反馈只进入一次决定/交接。

Plan 是协作协议和人审文档；Rhai 是验证后的 Definition/Run/journal；Goal 决定长期自动续跑与用量；队列决定已接纳输入的路由。四者不互相冒充。计划批准不改变工具授权、队首 hold、Goal owner 或存储故障准入。

### Recovery and retention

查看候选只读，不隐式恢复执行、启动 writer 修复或采纳决定。恢复核对 artifact、候选、决定、handoff 消费和 execution owner；不存在 runtime handle 不证明没有 durable 决定。

Session-scoped Plan query/review 通过既有 session/load barrier 解析 Actor。冷恢复期间 Actor 可能已注册到路由表，但在 suspended-tree 所有节点完成 release 前仍不可处理这些请求；lookup 必须等待 load guard，而不能把“路由已存在”视作恢复完成。

候选引用保护必要 artifact，纳入既有 GC 所有权和引用核对；不能另按 UI 活跃度/TTL 删除。当前 working tree 的输入 ACK、观察及恢复保全契约是实施依赖，尚不能用固定 HEAD 链接证明其已发布。

保护范围包含承载正文的 session 目录，而不只是 blob；整会话 stale cleanup 同样核对有效候选和未确认决定。放弃后仍有历史/恢复引用时不删除；扫描后新增引用须在删除边界重新核对。

fork/导入为新 session 时不继承来源审批、waiter 或可执行 handoff；如保留正文用于审阅，须以目标会话的新提交重新批准。Fork 会建立新 Timeline，可将 Plan runtime 和 retained candidate 规范化清除。`grow/session/import` 是不可变账本镜像，不能静默改写 Control 或 notification；在发布目标目录前，只要 Timeline 仍引用可再次审阅的 Deferred/Reviewing candidate、pending approval、未解决的 PlanApproval Interaction、AwaitingApproval/Amending/Executing Plan phase、`last_plan_handoff` 或未消费 PlanHandoff，就拒绝导入，并说明目标 owner 需建立新提交并重新批准。Approved/Abandoned 等已闭合 candidate 只保留审计 metadata，当前 Normal 且没有 handoff/receipt 时照常导入；导入不会因此获得审阅或执行资格。rewind 保持同一 owner 的当前 Control/未消费 handoff 责任，不删除合法待交接项；分支选择不复活已消费决定或使旧审批窗口重新有效。无法验证来源时拒绝执行，不另造计划回退引擎。版本号/hash 只证明审阅正文，不能证明 workspace 未变化；执行仍走当时的工作区、工具授权和既有前置检查，不增加全仓快照锁。

## Risks / Trade-offs

一个候选替换另一个时必须先有持久身份事实，旧版本可留历史但不可继续作为当前审批。Plan 评论批次按完整 artifact identity（session、binding epoch、submission、revision、hash）绑定；同一 identity 继续编辑原批次。用户开始给另一 identity 写新评论时，先将旧评论按原 artifact 正文格式化，并标明原 submission/revision/hash，追加到原 composer 后交给既有 stash；用户对另一 identity 作出审批决定时，同样把旧评论追加到 `PlanApprovalViewState::stashed_prompt`。追加只改变草稿正文与 cursor，保留附件及 chip offsets；旧批次随后清空，新评论才绑定新 identity。旧评论绝不混入新审批 feedback，也不自动发送。无法确认旧 artifact identity 时，保全为明确标记的未绑定旧笔记，不伪造来源。草稿/评论是用户数据，未知接纳时不能乐观清除。保留候选跨 Behavior 的 schema 与 typed disposition 必须在后端设计确定后再开放 UI。

## Delivery and Validation

P0 对齐 canonical 读取并回归已有恢复审批；P1 收起和入口；P2 保留引用、释放 waiter、恢复审阅与既有 handoff 闭环；P3 扩充修订/恢复/GC 覆盖。

若 P2 开放跨 Behavior/周期绑定，身份冲突、未知 ACK、defer 崩溃窗口、Control 恢复和回收保护同时是发布门槛，不留到 P3 才补。验证矩阵见 acceptance。先复用既有 plan_approval_resume 与计划 PTY 夹具，再加真实保留流程；不能仅运行纯 UI 测试就宣称闭环完成。

## Open Questions

需在实现前确定 typed defer response 与既有 EndTurn 的具体接点、候选 Control schema/投影及查询/决定 API、决定身份如何复用现有事件，以及 rewind 下候选保留/失效的具体规则。多计划库和“批准后无限期稍后自动跑”不在本次授权内，不以默认值悄悄加入。
