# Delta

## ADDED Requirements

### Requirement: Late tool errors do not reset replacement services
MCP 工具可恢复错误 SHALL 绑定失败调用使用的服务身份；当前 Ready 服务已替换时 SHALL 不重置新服务，而使用当前服务执行既有的一次重试。

#### Scenario: 旧调用错误迟于恢复完成
- **WHEN** 旧服务上的工具调用返回可恢复错误，而新服务已经完成握手
- **THEN** 不触发额外握手，仍按最多一次工具重试的策略返回结果。
