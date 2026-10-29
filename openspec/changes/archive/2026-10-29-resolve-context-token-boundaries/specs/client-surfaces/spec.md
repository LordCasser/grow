## ADDED Requirements

### Requirement: Restored usage remains consistent through resume presentation
冷恢复后的 usage SHALL 从已持久化账本到恢复快照、Pager 累计状态和 usage 分段展示保持一致；旧快照不得回退已展示的消费、完整性、缓存覆盖或恢复分段。

#### Scenario: Durable ledger reaches the resumed client
- **WHEN** 含多次调用、缓存字段可用性、未知消费与 resume segments 的账本恢复，并发送正常恢复快照
- **THEN** Pager 状态和 usage lifetime/segments 与该恢复账本一致，不将当前上下文压力当累计消费。

#### Scenario: An earlier snapshot arrives after the restored snapshot
- **WHEN** 新恢复快照已接纳，来自同一会话的早期延迟 usage 快照随后到达
- **THEN** 不回退累计消费、缓存覆盖或恢复分段，也不把已知不完整状态伪造成完整。
