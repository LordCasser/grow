## ADDED Requirements

### Requirement: Model session discovery lists online primary peers

主 Agent 的 `list_active_sessions` SHALL 只枚举同一 OS 用户和 GROW_HOME 下协调运行时可发现的其他在线 primary session。

#### Scenario: Primary session discovery with default arguments
- **WHEN** 主 Agent 调用 `list_active_sessions({})`
- **THEN** 返回当前可发现的其他在线 primary session，不枚举磁盘历史或 child session。
