## ADDED Requirements

### Requirement: Respawn completion belongs to the installed client
stdio MCP 重启 SHALL 在最后异步监听器初始化之后、发送工具刷新和返回成功之前，在同一状态锁内确认已安装客户端身份仍为当前 owned client。失配 SHALL 返回 Superseded，且不借用替代客户端身份发送刷新。

#### Scenario: 监听器初始化期间连接替换
- **WHEN** 重启已安装客户端，等待监听器初始化期间同名客户端被移除或替换
- **THEN** 旧任务返回 Superseded，不发出该次工具刷新或重启成功状态。

#### Scenario: 其他服务器配置变化
- **WHEN** 监听器初始化期间只有其他服务器配置变化，当前 owned client 身份保持
- **THEN** 本次重启仍可完成工具刷新并返回成功。
