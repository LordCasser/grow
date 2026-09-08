## ADDED Requirements

### Requirement: Initialization guards cover result commit waits
MCP 初始化 SHALL 保持取消恢复守卫有效直到取得结果写入状态锁；撤销守卫与写入结果之间 SHALL 无异步等待。

#### Scenario: 提交锁等待期间取消
- **WHEN** 可重建连接的握手结果已完成、提交等待状态锁，锁释放后初始化 future 被取消
- **THEN** 取消守卫恢复 Pending，不遗留 Initializing。
