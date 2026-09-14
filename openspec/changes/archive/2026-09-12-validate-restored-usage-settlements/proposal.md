## Why

用户已批准第一批架构优化。审计 probe 确认，实时结算拒绝同一身份的冲突，恢复却忽略冲突和损坏的已知结算事实，违反 session-timeline 的 lifetime projection 契约。

## What Changes

- 恢复时严格解析已知 attempt/child 结算及 incomplete/resume 标记。
- 实时与恢复共用结算身份的重复/冲突检查，合法重复只计一次。
- 在 actor 发布、恢复事件持久化之前传播校验失败；不改写原始记录。
- 增加异常恢复与合法恢复的对照回归，更新开发者契约入口。

## Capabilities

### Modified Capabilities
- `session-timeline`: 明确已知用量事实的恢复校验与发布边界。

## Impact

只涉及 chat-state 的恢复、用量 mutation、错误类型和相关测试。普通未知 Observation 保持可扩展；不合并 Goal、Sideband 账本，不迁移持久化 schema。
