## Why

主 Agent 每次采样都收到本地 TODO 摘要，即使 TODO 未变更。这会让模型反复播报相同的规划确认。

## What Changes

- 由主 Agent 自主决定何时读写 TODO，普通采样不注入计划快照。
- 继续将已提交的有效 TODO 变更通知追加给受影响的直属子 Agent。

## Capabilities

### Modified Capabilities

- `task-planning`: 普通采样与 TODO 变更通知的触发边界。

## Impact

SessionActor 请求组装、开发指南及规划回归测试。
