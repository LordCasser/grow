## Why

子 Agent 在每次模型请求末尾仍收到自动规划快照，即使没有变更也会重复确认。父 TODO 变更已经由持久通知通道交付，可以去掉子 Agent 的常驻快照。

## What Changes

停止对子 Agent 自动查询和投影计划。有效父 TODO 变更在安全采样边界追加现有通知；其他请求继续执行。按需 todo_read 仍由现有授权入口提供。

## Capabilities

### Modified Capabilities

- `task-planning`：子 Agent 的规划输入由变更事件驱动。
- `context-compaction`：区分压缩时的责任投影和普通子 Agent 请求。
