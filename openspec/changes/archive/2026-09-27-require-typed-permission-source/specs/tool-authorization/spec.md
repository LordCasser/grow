## ADDED Requirements

### Requirement: Permission request source is typed at the production boundary

生产工具授权入口 SHALL 显式携带 `PermissionRequestSource::Primary` 或 `PermissionRequestSource::Child`。子会话身份 SHALL 不由可选的 subagent 展示类型推断；缺少展示类型不得将子请求变为主会话请求。权限 manager SHALL 依据显式来源选择独立权限域和撤销边界。

#### Scenario: Child has no display type
- **WHEN** 子 Agent 的权限请求没有 `subagent_type` 展示字段，但具有显式 Child 来源和 session ID
- **THEN** 请求仍使用 child 权限域，不继承主会话的 remembered grant，ReleaseChild 可撤销它。

#### Scenario: Production caller lacks a source
- **WHEN** 生产调用方构造权限请求
- **THEN** 类型接口要求它提供 Primary 或 Child 来源，不能通过可选元数据静默推断。
