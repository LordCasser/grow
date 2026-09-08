# Delta

## ADDED Requirements

### Requirement: Late tool timeouts preserve replacement services
MCP 工具超时 SHALL 仅在当前 Ready 仍是该调用使用的服务时重置 transport；新服务或进行中的握手 SHALL 不被旧超时覆盖。超时 SHALL 不自动重放工具调用。

#### Scenario: 旧请求迟到超时
- **WHEN** 旧请求超时时其他恢复已经安装新 Ready 服务
- **THEN** 返回超时错误，保留新服务且不增加握手或工具重试。
