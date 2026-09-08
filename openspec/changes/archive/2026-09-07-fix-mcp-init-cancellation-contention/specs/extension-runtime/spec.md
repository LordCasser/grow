## ADDED Requirements

### Requirement: Initialization cancellation survives state lock contention
MCP 初始化取消 SHALL 在短时状态锁竞争结束后完成目标状态恢复，不得因 try_lock 失败丢弃清理责任。等待握手、通知或其他异步 IO SHALL 不持有客户端状态锁。

#### Scenario: 其他线程暂持状态锁
- **WHEN** 初始化取消时另一线程暂时持有客户端状态锁，随后释放
- **THEN** 取消恢复完成，状态从 Initializing 转为 Pending 或 Empty，不遗留中间状态。
