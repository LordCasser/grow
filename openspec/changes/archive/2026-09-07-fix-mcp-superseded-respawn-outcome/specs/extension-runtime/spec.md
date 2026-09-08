# Delta

## ADDED Requirements

### Requirement: Superseded respawns do not publish failure or clean tools
stdio MCP 重启发现其配置已被替换时 SHALL 返回独立失效结果，恢复循环 SHALL 结束旧任务而不推送失败、不继续重试或删除服务器工具。

#### Scenario: 最后一次握手配置失效
- **WHEN** 最后一次重启握手发现配置 generation 或内容已变化
- **THEN** 不产生该次失败/耗尽状态，不执行耗尽工具注销；之前真实失败记录保留。
