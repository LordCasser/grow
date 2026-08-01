# Dynamic Workflow 期望与现状(Drafting)

> **Status**: Drafting(记录用,无实施动作)
> **Date**: 2026-07-31
> **Scope**: grow-shell / xai-workflow / grow-pager 的 Workflow 行为与工作流引擎
> **结论**: 接受当前实现状态,不做行为改动;行为层命名已由 "Dynamic Workflow" 修正为 "Static Workflow"。

## 1. 背景与目的

用户 2026-07-31 提出:当前 "Dynamic Workflow" 行为与预期不一致。经调查确认,当前实现是
「行为层动态编排(主 Agent 每阶段编写并启动脚本,跑完再决定下一阶段)+ 引擎层静态确定性执行
(Rhai 脚本,运行内不可由 LLM 修改,host 调用 journal 记录、暂停/恢复全量重放)」,
与用户预期的「计划本身动态(任务图、执行中修订、多子 Agent 协同)」不是同一个模型。

本文档记录:用户的实际期望、Claude Code 的 dynamic workflows 参考设计、当前实现现状与差距,
以及本次「接受现状 + 命名修正」的决策。它是后续任何对齐工作的讨论基线,**不代表任何实施承诺**。

## 2. 用户的实际期望

按用户原意记录:

- 形成一个**动态的**计划:根据实际执行情况编排、修改;
- **多个子 Agent 协同执行**;
- **主 Agent 动态编排计划**。

即:计划是一个活的工件,由主 Agent 在运行时生成,并随执行结果被持续修订,而非预先写死。

## 3. Claude Code 的 Dynamic Workflows(参考设计)

基于公开发布信息整理:

- **主 Agent**:在运行时把目标拆解为任务图(节点 = 任务,边 = 依赖);
- **任务 Agent(task agent)**:每个任务由专属子 Agent 执行,携带名称、prompt、工具集、模型与
  `output schema`;相互独立的任务并行执行;
- **动态再规划**:主 Agent 拿到结构化结果后,增删任务、重排顺序、改变依赖、重试失败任务;
- **可视化与恢复**:工作流图视图展示任务状态;工作流状态持久化、可断点恢复。

> 附注:本部分基于公开设计知识。博客
> (https://claude.com/blog/introducing-dynamic-workflows-in-claude-code)与 commit
> (github.com/claude-code-best/claude-code@58ee6419)因调查环境网络限制未直接核实,
> 具体实现细节(工具名、schema、UI 形态)未在本记录中承诺。

## 4. 当前实现现状(2026-07-31,已改名 Static Workflow)

四层结构:

1. **行为层(Static Workflow Behavior)**:主 Agent 获得 `workflow` 工具与
   `prompts/behaviors/static-workflow.md` 提示词;每阶段:scout → 编写一个确定性 Rhai
   工作流脚本 → 启动**至多一个**运行 → yield(不轮询、不 sleep-wait)→ 收到完成通知后
   检查结果 → 决定/修订下一阶段。
2. **引擎层(xai-workflow)**:Rhai 脚本;确定性执行(禁 `eval`/`timestamp`/`sleep`);
   host 调用按 `seq+hash` 记录 journal;暂停/恢复全量重放;脚本与 args 运行内不可变;
   `agent()` 单发、`parallel()` 平铺扇出(≤1024 项);`phase()`/`complete()`/`pause()`/
   `await_user()`/`budget()` 等 host 函数。
3. **运行层**:`WorkflowManager`(每会话 ≤4 活跃运行、agent_budget/max_concurrency 双限)、
   `WorkflowTracker`(阶段、agent 行、预算、历史、result_summary)、`WorkflowRunStore`
   (script.rhai + journal.jsonl + manifest)、`/workflows` 运行列表与
   `/workflow-run pause|resume|stop|save`。
4. **相邻行为**:Deep Research(私有工作流 + 强制终态报告契约);Goal(固定控制循环:
   planner → 主 Agent 分轮工作 → verifier/skeptic 面板验证 → strategist 调整 → 循环)。

**命名约定(本次修正后)**:用户可见命名统一为 "Static Workflow";wire id 与内部标识
**保持不变**——`"workflow"`(ACP SessionModeId / BehaviorId / tool name)、
`SessionMode::Workflow`、`PromptMode::Workflow`、`BehaviorState::Workflow`、
包名 `xai-workflow`。

## 5. 差距对比

| 维度 | 期望 / Claude Code | 当前实现(Static Workflow) |
|---|---|---|
| 计划形式 | LLM 运行时生成并持续修订的任务图 | 无持久化计划工件;每阶段一段 Rhai 脚本,"计划"存在于对话上下文 |
| 执行中动态修改 | 编排者根据结果增删/重排/重试任务 | 运行内脚本不可变 + journal 确定性重放,LLM 不可中途介入;自适应只发生在运行之间 |
| 并行模型 | 依赖 DAG 的任务调度 | `parallel()` 平铺扇出;每阶段至多一个运行 |
| 子 Agent 形态 | 有身份/工具集/输出契约的任务 Agent,支持嵌套 | 脚本一次性 spawn 的匿名子 Agent;workflow 仅顶层,子 Agent 不再嵌套(MAX_SUBAGENT_DEPTH=1) |
| 编排循环 | 编排者持有计划、持续决策 | 主 Agent launch → yield → 检查 → 下一阶段;无可见的计划状态可修改 |
| UI | 工作流图视图(任务状态、依赖) | `/workflows` 运行列表(阶段/agent 行/预算),无图 |
| 可靠性取向 | checkpoint + 恢复 | 确定性 journal 全量重放(更强,代价是运行中不可自适应) |

## 6. 决策记录(2026-07-31)

- **接受当前实现状态**,不做行为改动。
- **命名修正**:用户可见命名 "Dynamic Workflow" → "Static Workflow"(显示标签、行为提示词、
  picker、/workflow 命令描述、错误消息、文档);wire id / 枚举变体 / tool name 不变。
- **未来候选方向**(仅记录,不实施,未排序):
  - 小:为 Static Workflow 行为增加持久化的阶段计划工件,让主 Agent 修订可见;
  - 中:引擎增加运行内 replan 能力(与 journal 重放机制冲突,需计划快照入 journal);
  - 大:主 Agent 维护任务图,`spawn_task` 并行派发
    带工具/输出契约的任务 Agent;现有 Rhai 引擎保留为任务内部执行器。
