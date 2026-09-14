## Why
普通 full/light load 通过观察句柄读取，却在 Summary 落后于 Timeline 时修复 title/model 并争抢 writer lease。resident 重新连接可能因合法的投影滞后失败，也可能让读者意外成为写者。

## What Changes
- full/light observation 只派生内存 Summary，显式 replacement-writer load 才修复磁盘投影。
- 保留 Timeline 校验、冲突拒绝及 sideband 恢复的写者边界。
- 不修改 Summary 格式、锁协议或 resident actor 的模型状态。

## Capabilities
### New Capabilities
无。
### Modified Capabilities
- session-timeline: 普通观察与持久化修复的边界。

## Impact
shell JSONL adapter、恢复测试与开发说明。用户已批准本轮剩余审查项实施。

