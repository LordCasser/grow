## ADDED Requirements

### Requirement: Deny before automatic approval
权限管理 SHALL 在 always-approve 快速放行前执行显式 deny 策略。

#### Scenario: 显式禁止命令
- **WHEN** 调用匹配 deny 规则且会话为 always-approve
- **THEN** 调用被拒绝。

证据：`crates/codegen/workspace/src/permission/manager.rs` — `policy_deny`。

### Requirement: Conservative shell access
shell 调用 SHALL 从冻结命令投影所需 RWX；无法解析或未知可执行程序按 All 处理。

#### Scenario: 未知程序
- **WHEN** 命令无法证明属于已知只读观察命令
- **THEN** shell_required_access 返回 All；已知无外发的观察命令可返回 ReadExecute。

证据：`crates/codegen/shell/src/session/actor/tool/authorization.rs` — `shell_required_access`。

### Requirement: Immutable delegated ceiling
子 Agent SHALL 将后代请求能力与创建时上限取交集，并绑定继承 MCP 的服务端及 client identity。

#### Scenario: 后代请求更大权限
- **WHEN** 子 Agent 请求创建权限更大的后代
- **THEN** constrain_mode 返回交集；不匹配初始 client_id 的 MCP binding 不可委派。

证据：`crates/codegen/shell/src/session/subagent_capability.rs` — `DelegableCapabilityCeiling`。
