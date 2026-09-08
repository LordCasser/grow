# tool-authorization Specification

## Purpose
定义实际工具调用与可见工具目录之间的授权边界。覆盖显式 deny 优先、冻结 shell 命令的保守 RWX 投影，以及子 Agent 向后代委派时不可扩大的能力与 MCP identity 上限。

## Requirements

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

### Requirement: Web allowlist preserves path identity
web_fetch 静态允许列表 SHALL 仅对主机执行大小写、www 前缀和尾点规范化；路径匹配 SHALL 保留大小写和末尾点，并以完整路径段作为前缀边界。

#### Scenario: 不同路径不共享静态许可
- **WHEN** 配置 example.com/Docs 或 example.com/docs.
- **THEN** 对应路径及其子路径匹配，但 /docs 不因路径规范化而获得该条目的静态许可。

#### Scenario: 带路径主机规范化
- **WHEN** 配置 WWW.Example.COM./Docs
- **THEN** example.com/Docs 匹配，example.com/docs 不匹配。

### Requirement: Redirect targets require a new authorized call
web_fetch SHALL 不自动请求 HTTP 跳转目标，而返回 RedirectRequired、原始 URL 与解析后的目标 URL，要求新的工具调用经过正常授权；同主机也不例外。

#### Scenario: 同主机路径或端口变化
- **WHEN** 已授权 URL 返回指向不同路径或端口的 Location
- **THEN** 返回目标提示且不发送目标请求；目标被单独调用时经过普通验证与授权路径。

#### Scenario: 无效跳转目标
- **WHEN** Location 不可解析、包含凭据或使用不支持的 scheme
- **THEN** 返回跳转错误，不请求目标。

#### Scenario: 客户端展示
- **WHEN** 工具返回 RedirectRequired
- **THEN** 模型提示包含目标 URL 和新调用说明，ACP 将该次未取得内容的调用标记 Failed。

### Requirement: Question request cancellation preserves the worker
ask_user_question coordinator SHALL 在通知 Hook 阶段当前接收端关闭时结束该请求并继续服务后续问题，释放当前 pending guard；服务关闭与 Hook 失败仍遵从既有退出规则。

#### Scenario: 取消问题后再次提问
- **WHEN** 第一个问题在通知 Hook 等待前或等待中失去接收端，随后另一个有效问题到达
- **THEN** 后一个问题仍可经 ACP 返回响应，不因前一个请求取消而失去 coordinator。

### Requirement: Question option labels are unambiguous
ask_user_question SHALL 在发送请求前拒绝同一道题内完全相同的选项 label，并返回参数错误；不同题之间相同 label SHALL 允许。选项 ID 或描述差异 SHALL 不使重复 label 合法。

#### Scenario: 同题重复选项
- **WHEN** 同一问题的两个选项 label 相同
- **THEN** 工具返回参数错误，不向 coordinator 发送请求。

#### Scenario: 不同题共用选项文本
- **WHEN** 不同问题均有同名选项，但各题内部 label 唯一
- **THEN** 不因跨题重复拒绝请求。

### Requirement: Stable per-client permission cache keys
Remembered permission storage SHALL derive per-client filenames from the SHA-256 digest of the exact UTF-8 client identifier. An absent identifier SHALL use permission.toml; a missing per-client file SHALL retain the shared-file fallback. Legacy sanitized per-client names SHALL NOT be probed as a fallback.

#### Scenario: Previously colliding identifiers
- **WHEN** clients foo/bar, foo\\bar and foo_bar persist distinct grants
- **THEN** each client reloads its own grants without overwriting the others.

#### Scenario: Empty and long identifiers
- **WHEN** a client identifier is empty or contains a long Unicode string
- **THEN** it maps to a fixed-length filename distinct from the shared filename.

### Requirement: Permission read errors do not import shared grants
Permission state loading SHALL allow shared-file fallback only when the client-specific read reports NotFound. Other read errors, including invalid UTF-8, SHALL use default remembered state and SHALL NOT rewrite the failed source. This does not override independent configured policy or permission modes.

#### Scenario: Invalid UTF-8 client cache
- **WHEN** a shared cache contains grants and a client-specific cache contains invalid UTF-8
- **THEN** loading that client returns default remembered state and preserves the invalid source bytes.

#### Scenario: Client cache is a directory
- **WHEN** a client cache path is a directory and its read fails
- **THEN** loading returns default remembered state without importing shared grants or altering the directory.

### Requirement: Permission cache IO is bounded
Permission cache reads SHALL admit only ordinary files of at most 1 MiB, checking metadata on the opened handle and limiting actual bytes before UTF-8 and TOML parsing. Rejected reads SHALL preserve the source and use default remembered state without shared fallback. Unix opening SHALL not wait for a FIFO writer. Serialized writes over 1 MiB SHALL fail before atomic publication, preserving the previous destination.

#### Scenario: Exact and excessive bytes
- **WHEN** valid serialized state is exactly 1 MiB or a source exceeds that limit
- **THEN** the exact-boundary state loads and the excessive source is rejected; actual read consumes at most limit+1 bytes.

#### Scenario: Special source
- **WHEN** a Unix permission cache is a FIFO without a writer
- **THEN** reading rejects it without waiting for a writer and preserves the FIFO.

#### Scenario: Oversized write
- **WHEN** a serialized permission snapshot exceeds 1 MiB
- **THEN** writing returns an error and leaves the prior destination bytes unchanged.
