## Why

`resume_from` 在派生子 Agent 启动前把来源 active Surface 的粗估值与目标窗口的 80% 比较。已验证的长历史因此会在普通请求投影和自动压缩介入前被拒绝。
派生 Seed 按设计移除来源 prompt 编号，现有压缩选区也会遗漏这部分历史。

## What Changes

- 将派生恢复的窗口准入交给子 Agent 现有的请求投影、压缩与采样准入。
- 允许无本地编号的继承用户回合为压缩提供边界。
- 保留来源 cut、展示继承和来源会话的不可变性。

## Capabilities

### Modified Capabilities

- `session-timeline`：派生恢复的历史和首个请求准入。
- `context-compaction`：继承历史的压缩选区。

## Impact

涉及 Shell 的子 Agent bootstrap、恢复回归和开发说明。
