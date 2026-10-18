## Why

Grow 已有 TODO、task/subagent、Goal 自动续跑、父子消息与持久结果，但这些机制尚未形成完整的任务规划闭环。TODO 是可整表替换的工具资源，task 没有步骤关联；父任务改了要求，子 Agent 不会因此调整自己的计划。模型等待子任务时，现有 Goal idle 续跑也缺少明确的依赖等待条件。嵌套 child 的完成回执被抑制，进一步影响逐级交付。

本次采用用户确认的弱关联：父 TODO 表达交付意图，子 Agent 管理自己的 TODO。父项发生非完成修改时，系统可靠通知直接关联的子 Agent，由它检查并调整局部计划。关联不把两份 TODO 合成一张表，也不要求版本一致才能接受结果。

## What Changes

- **BREAKING**：TODO 的运行时事实改由所属 SessionActor 经 Timeline 提交，工具资源、模型提醒和客户端都读取该投影。整表替换不能抹掉尚未闭合的执行、等待和通知责任。
- 每个 session 保有自己的局部计划和工作范围。task 可以关联当前 session 的一个父 TODO；一个父项可以关联多次执行，子 Agent 自行拆解自己的步骤。
- 父 TODO 内容、要求、优先级、非完成状态、取消或删除发生实际修改时，触发子 Agent 重规划检查。仅标记完成、重复写入或展示更新不触发；内容修改与完成同批提交仍触发。
- 重规划请求按来源变化与 child 去重，未消费更新可合并；已消费不等于已处理。子 Agent 在安全边界更新自己的 TODO，或明确确认原计划仍适用。终态 child 不自动复活，旧结果不自动作废。
- 引入有实际 task/command/question/permission 身份的等待意图；等待通过现有事件机制唤醒，不靠反复模型轮询或 idle 忙循环。普通任务与子 Agent 只继续原已接纳工作，不自动创建 Goal。
- 普通 task 的结果交付到直接委派它的 session。嵌套结果沿父子层级验收，根 session 的生命周期观察不再代替直接父级的模型回执。
- 委派交付物与关键里程碑区分“结果已交付”和“已验收”。Goal 完成保留全目标审计，并核对显式必需义务；普通局部 TODO 不全部升级为完成硬门槛。
- UI、headless、压缩上下文和恢复使用同一局部计划事实；turn 结束不再把尚未完成的 `in_progress` 临时显示成完成。
- 与 `suspend-session-tree` 衔接：冻结时保留局部计划、关联、等待、未处理重规划与未验收结果；恢复检查中断命令，不自动重放副作用。

## Capabilities

### New Capabilities

- `task-planning`：局部计划、弱委派关联、重规划触发与确认、显式等待及交付验收。它是行为契约分区，不要求增加独立 Planner 服务或调度器。

### Modified Capabilities

- `session-timeline`：计划事实与通知责任持久化、消费/处理分离、直接父级结果交付、恢复与清理依赖。
- `behavior-goal`：基于真实等待的续跑准入、完成义务核对及原 Goal 权威隔离。
- `tool-authorization`：局部计划和关联的 owner 校验，内部重规划通知的权限边界。
- `client-surfaces`：真实进度、嵌套计划、弱关联和等待展示。
- `context-compaction`：当前局部计划及未闭合责任的有界上下文投影。

## Impact

涉及 TODO/task 工具和 bridge、Shell SessionActor/idle arbitration/subagent runner、coordinator 的直接父级路由、ChatState Timeline 与 inbox、ACP/Grow 投影、Pager/headless、compaction reminder。具体入口和当前实现差异见 [design.md](design.md)。后续实现需同步更新开发者说明与工具描述，不提前改写主规范。

依赖 [suspend-session-tree](../suspend-session-tree/proposal.md) 提供真正的同身份树级冻结恢复；本 change 不重复定义 Ctrl+C、writer drain 或命令中断协议。普通运行下的规划能力可先独立实现，但冻结验收必须等该依赖完成。

不包含：独立 Planner/Verifier Agent、通用 DAG/调度平台、自动创建 Goal、TODO 自动生成 Rhai Workflow、跨 session 任意改计划、文件锁调度、自动合并 worktree、命令重放、保留审批 Plan 的另一个 change，或所有 Resources 的存储迁移。

## Status

2026-10-18：仅编写 OpenSpec change，尚未实现或验证产品行为；不归档、不修改其他 change。本次文档验收与后续运行验收分别记录在 [verification.md](verification.md) 和 [acceptance.md](acceptance.md)。
