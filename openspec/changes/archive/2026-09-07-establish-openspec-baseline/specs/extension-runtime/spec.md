## ADDED Requirements

### Requirement: MCP transport ownership
MCP 层 SHALL 负责 stdio 子进程与 Streamable HTTP 连接、工具调用和生命周期管理。

#### Scenario: 调用外部 MCP 工具
- **WHEN** 已配置 MCP server 通过支持的 transport 建立连接
- **THEN** 工具经 MCP server 层调用并分类处理连接或调用错误。

证据：`crates/codegen/mcp/src/lib.rs` — `servers`。

### Requirement: Explicit hook planning
Hooks SHALL 按事件、匹配条件、启用状态与策略形成 Execute 或 Skip 计划。

#### Scenario: Hook 被禁用
- **WHEN** 命中的 Hook 显式 disabled 或被 policy 禁用
- **THEN** 计划记录对应跳过原因，不执行该 handler。

证据：`crates/codegen/hooks/src/dispatcher.rs` — `plan_dispatch_with_policy`。
