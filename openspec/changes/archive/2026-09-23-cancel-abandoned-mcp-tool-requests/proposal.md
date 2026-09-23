## Why

Grow 目前用外层 `tokio::time::timeout` 包住 `rmcp::RunningService::call_tool`。超时或 turn 取消只丢弃本地 future，不保证 `notifications/cancelled` 及原 JSON-RPC request id 到达 stdio/HTTP MCP server；远端副作用性或昂贵操作可能继续运行。上游 grok-build 已增加 request-handle + Drop guard；Grow 固定的 rmcp 2.2 也具备此 API，但 Grow 的重试、服务身份和 ACP direct 调用边界不同。

## What Changes

- MCP 层提供一次 `tools/call` 的 cancel-aware helper，主调用、恢复后的唯一重试及直接 `grow/mcp/call` 共用它。
- 请求发出后将 request id 和发起时的 peer 绑定；Grow 用同一绝对期限覆盖发起请求及等待响应，并由唯一 Drop guard 在超时或 future 被丢弃时 best-effort 发取消通知。rmcp handle 不启用自身 timeout，避免与 guard 重复通知；已收到终态后解除守卫。发起阶段未取得 id 时不伪造通知。
- 保留超时不自动重放副作用性工具、最多一次既有恢复重试以及旧服务不能重置替代 Ready 服务的契约。
- 测试真实 rmcp 对端收到的 request id/取消次数；ACP reverse bridge 当前不转发无 id 通知，此限制明确记录而不扩成另一项协议工程。

## Capabilities

### Modified Capabilities

- `extension-runtime`: stdio/HTTP 在途 MCP `tools/call` 被宿主放弃时发协议取消通知，同时维持现有 transport 恢复归属。

## Impact

- 生产：`crates/codegen/mcp/src/servers.rs` 单次调用 helper 与主路径；`crates/codegen/shell/src/extensions/mcp.rs` direct 路径调用该 helper。
- 测试：`mcp` 和 direct extension 测试；开发者说明链接 `extension-runtime`。不修改 ACP reverse bridge 通知协议、授权、工具目录或业务工具重试次数。
- 与 `preserve-mcp-result-content` 可共享读取入口但不改变结果形状；实施时协调 `servers.rs` 的写入所有权，避免覆盖并行修改。
