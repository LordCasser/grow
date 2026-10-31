## ADDED Requirements

### Requirement: Delegation authorizes canonical definition identities

普通 subagent 委派 SHALL 先解析正式 definition selector，再对正式身份应用全局 toggle 与父 allow/deny。目录 SHALL 包含 native、plugin、CLI 最终 fallback 的可委派定义，按照相同优先级合并并统一过滤；普通会话重建 SHALL 保留 CLI 来源。非规范路径 selector SHALL 被拒绝，不得通过路径拼写绕过限制。

#### Scenario: An alternate selector resolves to a disabled or denied plugin
- **WHEN** bare selector 解析到限定 plugin selector，且正式 selector 被 toggle 禁用或父 deny 拒绝
- **THEN** 预检与派发均拒绝，并保留实际拒绝原因；正式 allow 列表同样对 bare 请求生效。

#### Scenario: A native selector contains path aliases
- **WHEN** selector 包含空段、`.`、`..`、绝对路径，或请求与注册路径 ID 不同的大小写拼写
- **THEN** 不解析成另一个可委派身份，不绕过正式名称上的限制。

#### Scenario: Plugin and CLI catalogs change enablement
- **WHEN** 唯一允许的 plugin 或 CLI Agent 被关闭后重新开启
- **THEN** 普通会话与子会话的 Task 目录移除后恢复该正式 ID，空目录撤下 Task；重建保留 CLI 来源，重复名称继续使用执行解析选中的定义。
