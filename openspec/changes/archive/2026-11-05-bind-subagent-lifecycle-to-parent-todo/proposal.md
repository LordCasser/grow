## Why

用户要求 TODO 成为普通子任务的委派前提，并通过它掌握子任务归属、改变目标和终止工作。现有 LocalPlan 已有稳定步骤、执行 link、tombstone、来源修订和交付验收，但关联仍可选；删除只产生一般 replan，子任务上下文通过 prompt 拼接。另有实际偏差：local_planning_context 虽注释 request-only，Shell 把它合入 memory_reminder，ChatState 随后写为持久 MemoryContext。

## What Changes

- 普通模型 spawn/resume 必须指定当前可委派 TODO，运行时先持久确认关联再派发；一个 TODO 支持多次独立执行。
- 新增 Grow 原生 `todo_read`，主/子 Agent 可读自己的计划，子 Agent 可读其直属父计划；模型不提供任意 session 身份。返回计划位置、执行关联、真实待处理/验收事实与有界分页。
- Grow 从 canonical 计划和直属委派身份构造模型请求内的上下文，包含关联步骤和整体位置，不拼用户 prompt、不落用户聊天或 ACP 正文。上下文照常计入输入预算。
- 编辑 TODO 产生固定目标变化通知；删除、replace 省略或 cancelled 产生固定终止收尾通知。复用当前软中断和 durable receipt，不调用硬取消提前关闭子 Timeline。已投递事实不修改，终止不被稍后重开步骤撤销。

## Capabilities

### Modified Capabilities

- task-planning：强制委派关系、只读查询、内部上下文及编辑/删除的不同控制语义。
- session-timeline：请求内规划上下文与带运行时来源的内部控制事实。
- client-surfaces：原生todo_read的plan_read分类及canonical工具元数据闭合词表版本同步。

## Impact

tool-types 规划类型、tools TODO/Task/backend、ChatState request assembly、Shell planning/coordinator 与原生工具注册/权限投影及相关用户说明。沿用 LocalPlan/Timeline/coordinator，各自保持事实、接纳与执行所有者；不新建全局任务调度器，不让父直接改 child 本地 TODO，不把删行当作进程已经结束或结果已经验收。内部 Workflow 子任务仍由 Workflow owner 管理，不强加虚构 TODO。

本 change 以用户明确的新要求替换旧规范的可选绑定及“删除仅评估、需另行取消”的部分，不更改结果持久确认、权限、预算、Step 和命令结算边界。现有 dirty 和被宿主取消的 backlog 候选保留，另行验收，不混为本次通过。
