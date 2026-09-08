## ADDED Requirements

### Requirement: HTTP recovery outcomes retain client identity
HTTP MCP 恢复 SHALL 在传播成功或失败之前，在同一状态锁内核对捕获客户端仍为当前连接及 HTTP 配置仍存在，并核对同步读取的禁用状态。缺失、替换或禁用 SHALL 分类为 Superseded；恢复循环 SHALL 立即结束旧任务，不按名称重试替代客户端。

#### Scenario: 握手结果返回前配置替换
- **WHEN** HTTP 恢复的成功或失败结果返回时捕获客户端已被替换
- **THEN** 结果分类为 Superseded，旧循环不继续恢复替代连接。

#### Scenario: 当前连接真实失败
- **WHEN** 客户端身份和配置仍有效但恢复失败
- **THEN** 继续现有有限退避重试。
