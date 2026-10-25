## Context

`SessionActor::handle_goal_command` 已执行 planning 完成门槛，并经 `commit_goal_stop_or_restore` 同时提交 Goal 与退出 Goal Behavior。GoalState 是 Control 内的持久状态，`goal_notification::build_goal_updated` 及 `GrowHistoryFact::GoalUpdated` 将它投影到 Pager。现有 `GoalDetailRenderer` 支持点击打开、键盘及滚轮浏览。

## Goals / Non-Goals

保全“为什么完成”的模型报告并在 Done 状态直接查看。沿用既有整体完成审计和 authority 校验，不改变预算、自动续跑或 planning 规则，也不为已有完成状态生成未经验证的报告。

## Decisions

- 用 `Option<String>` 将报告直接归属 GoalState；新增完成操作只能传入非空原文。报告与 Complete 在同一 Control ACK 中确认，失败沿现有回滚路径恢复 Active。相比照搬 Plan 的外部 artifact 和查询接口，此处既有完整 Goal 投影已足够，内联字段避免引入第二套结果身份与读取路径。
- 工具、Goal 行为提示要求报告包含结果、完成证据/验证和限制；runtime 验证非空，不假装能用字符串结构判定业务是否完成。blocked 不能附完成报告。
- Goal 编辑重激活时清除当前完成报告；迟到用量不修改报告。完整历史仍保留原 Control 事实。
- `GoalView`、Grow 通知和 typed history 传递报告原文。完成详情中报告位于目标之后、用量之前，并扩大完成态弹窗以便阅读；保留既有滚动和关闭交互。无报告的历史 Complete 显示明确提示。
- 现有鼠标分支会因任意 Workflow 历史抢占 Goal 点击；按可见 Goal 状态优先打开 Goal 详情，避免完成报告入口被历史记录遮蔽。

## Risks / Trade-offs

- 报告随 Goal 快照复制 → 复用现有有界通知预算，不另建无限队列；报告只作为完成结果，不作为工作日志。
- 报告内容仍依赖模型证据判断 → 保留原完成门槛并明确要求具体证据，测试仅证明归档和状态原子性。
- 工作区有用户已有修改 → 仅定点修改 Goal 分支与追加文档，保留原改动。
