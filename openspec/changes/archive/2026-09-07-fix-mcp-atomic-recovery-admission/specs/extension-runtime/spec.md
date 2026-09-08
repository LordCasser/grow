# Delta

## ADDED Requirements

### Requirement: Concurrent recovery admission is atomic
MCP recover SHALL 在同一状态锁内检查 Ready 并重置为 Pending，使竞争的恢复调用加入同一个后续握手。

#### Scenario: 两个恢复竞争 Ready
- **WHEN** 两个恢复调用在锁竞争下尝试恢复同一个 Ready client
- **THEN** 仅一次 Ready 到 Pending 重置，随后两者共享新握手产生的服务。
