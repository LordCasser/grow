## Context

Goal 的持久状态经 `GoalUpdated` 投影到 `GoalDisplayState`，顶部 `goal_status_line` 和点击详情共用该事实。Plan 已有 `plan_mode_active`、`plan_phase` 以及 `ShowPlan → QueryPlanArtifact` 路由，但 `should_show_plan_chip` 额外要求缓存正文，审批完成清理 view 后入口消失。

TODO 的 `PlanningRuntimeHandle` 指向 SessionActor，计划持久化在 Timeline。启动会注入 handle 并 hydrate；`apply_agent_definition` 更换 ToolBridge 后只补 Goal/Workflow 等资源，漏掉 Planning。指定 session 在上海时间 2026-10-19 15:50:18 从 grow 切换 software-architect，16:19:16 的 TODO 调用失败；唯一 ACP plan 是初始化空列表。

## Goals / Non-Goals

实现 proposal 中两个最小修复。局部 TODO 与审批 Plan 保持独立：不从批准的 Markdown 推断任务列表，不自动将 TODO 完成视为 Plan 完成。无新工具、无数据迁移，不重启用户 session。

## Decisions

1. 抽取已有 hydration 中的资源绑定为同一方法，接收目标 ToolBridge，从权威 Timeline 读取计划，绑定 actor sender、LocalPlanProjection 和 TodoState。启动调用该方法后仍发布初始 ACP plan；Agent 候选激活前调用该方法，读取失败拒绝替换，不从旧 harness 复制可能缺失的投影，也不因替换重复发布计划提交。
2. 活跃 Plan 的顶部入口只依赖现有 Behavior 投影，显示 Drafting/Review/Executing/Amending；非活跃时保留现有 retained/配置及有效缓存条件。入口点击使用已有 ShowPlan，在既有计划详情视图内展示完整正文及版本；草拟阶段尚无 artifact 时展示已有查询的无计划提示，不伪造正文或发起模型工作。用户明确要求保留 Plan 模式的 TODO task，因此 TODO 徽标与任务面板继续独立显示和打开，不被 Plan 详情取代。
3. 测试通过真实 ToolBridge 在 Agent 替换前后写 TODO 并核对 Timeline、资源及 ACP 投影；Pager 测试无正文缓存的状态与真实命中区点击，保留旧会话缓存拒绝和 Goal 展示回归。

## Risks / Trade-offs

- Agent 激活前读取规划状态失败：沿现有候选构建错误返回，旧 Agent 保留。
- 终端窄屏：沿现有 AgentStatusBar 的右侧优先裁剪规则，不扩展布局框架。
- 修复不会给已失败的历史调用补写任务；用户加载新版本并继续后由模型使用工具提交实际 TODO。
