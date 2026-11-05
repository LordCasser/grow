## MODIFIED Requirements

### Requirement: Late tool errors do not reset replacement services

MCP 工具可恢复错误 SHALL 绑定失败调用使用的服务身份；当前 Ready 服务已替换时 SHALL 保留新服务并返回原调用的失败，不因该错误重置新服务、启动额外握手或自动重放工具调用。后续独立调用 MAY 使用当前连接，仍遵守不确定调用不自动重放的既有契约。

#### Scenario: 旧调用错误迟于恢复完成
- **WHEN** 旧服务上的工具调用返回可恢复错误，而新服务已经完成握手
- **THEN** 返回原调用失败并保留新服务，不触发额外握手，也不在旧服务或新服务上自动重试该工具调用。
