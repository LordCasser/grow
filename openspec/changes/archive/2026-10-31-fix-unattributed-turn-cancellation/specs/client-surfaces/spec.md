## ADDED Requirements

### Requirement: Cancellation markers do not infer the actor
Pager 的共用取消标记 SHALL 显示中性的 `Turn cancelled` 及耗时，不根据 `cancelled` stop reason 或本地 cancelling 状态声称用户发起了取消。

#### Scenario: Host cancellation is presented
- **WHEN** RPC 或 durable terminal 带来 cancelled 终态，而展示事件没有操作者来源
- **THEN** 取消标记不含 `by user`，两路先后到达仍只显示一次终态。

#### Scenario: Explicit user cancellation is presented
- **WHEN** 用户取消的回合完成正常终态结算
- **THEN** 继续显示同一中性取消标记，输入恢复、队列处理和终态去重保持原规则。
