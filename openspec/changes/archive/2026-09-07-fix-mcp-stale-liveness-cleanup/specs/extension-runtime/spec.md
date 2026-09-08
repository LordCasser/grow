# Delta

## ADDED Requirements

### Requirement: Superseded liveness watchers cannot clear replacements
MCP 旧 liveness watcher SHALL 在共享锁内确认自己未被替换取消后才能清理槽位；已取消的旧 watcher SHALL 不清理新句柄或发送该次关闭事件。

#### Scenario: 状态检查期间替换
- **WHEN** 旧 watcher 等待状态检查时其 handle 被替换，随后旧 watcher 进入退出清理
- **THEN** 新 handle 保留且其 token 未被旧清理取消；新 watcher 仍可自行清理退出。
