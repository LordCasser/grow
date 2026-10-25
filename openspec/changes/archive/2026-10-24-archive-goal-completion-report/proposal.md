## Why

Goal 当前完成时只发布 `Goal completed.`，用户无法从 Done 状态追溯完成依据。现有规范要求整体完成审计，但实现没有保全审计报告；本次补齐该结果的持久化与查看闭环。

## What Changes

- **BREAKING** `update_goal(status=complete)` 必须提供非空 `completion_report`，说明目标达成情况、完成依据、验证结果及仍存在的限制。
- 报告随原 Goal 的完成状态一起持久确认；失败时不宣称完成。
- 点击状态栏 `Goal: Done` 在既有详情中查看完整报告，恢复和只读回放保留原文；缺报告的历史 Goal 明确显示未归档。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `behavior-goal`：完成报告归属及完成提交门槛。
- `client-surfaces`：Done 详情的报告展示与恢复。

## Impact

涉及 Goal 工具输入与提示、GoalTracker、Control 快照、Goal 通知及 typed history、Pager Goal 详情。复用现有 Goal 状态和点击入口，不新增文件存储、RPC、报告管理器或自动补写历史报告。
