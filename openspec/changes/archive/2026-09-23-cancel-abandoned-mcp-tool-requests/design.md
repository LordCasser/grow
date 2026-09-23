## Context and evidence

`mcp/src/servers.rs::McpErasedTool::try_call_tool` 和 `recover_and_retry` 使用外层 `tokio::time::timeout`；`McpClient::call_tool` 为 direct 调用提供无内置限时的路径，`shell/src/extensions/mcp.rs::handle_call` 再套外层 timeout。Grow 当前 rmcp 2.2 的 `Peer::send_cancellable_request` 提供请求 ID 与 `await_response`，但 `PeerRequestOptions::with_timeout` 的到期分支会异步发送 `notifications/cancelled`；若 future 在这次发送中被丢弃，另一个 Drop guard 会重复通知。现有 `extension-runtime` 要求晚到超时不得覆盖替代 Ready 服务，超时不得重放调用。`mcp/src/acp_transport.rs` 的半双工桥丢弃无 id 通知，故本次可保证的远端接收范围是 stdio/HTTP。

## Decisions

1. **一次请求一个 helper。** 构造 `ClientRequest::CallToolRequest`，用发起服务的 peer 发送可取消请求并取 `RequestHandle.id`；`await_response` 的成功、MCP 错、timeout 映射为当前调用返回类型，不吞掉业务 `isError`。不要在 helper 中做重连/重试；外层现有恢复逻辑决定是否再调用一次 helper。
2. **精确绑定与一次通知。** handle 创建后启动绑定该 peer/id 的 Drop guard；`PeerRequestOptions::no_options()` 不启用 rmcp 内部 timeout，Grow 用同一绝对期限覆盖 `send_cancellable_request` 与 `await_response`。发起阶段到期还未取得 id 时不伪造通知；取得 id 后到期或 future 被取消/丢弃都只由该 guard best-effort 异步通知 `notifications/cancelled`。正常完成时解除守卫。发送失败仅记诊断，不改变原本取消/错误结果；若 runtime 已不可用，守卫不阻塞 Drop。
3. **超时预算与恢复保持原义。** 主调用、唯一 retry 和 direct 调用沿用各自现有配置预算，不因 helper 额外获得无限期限；`ServiceError::Timeout` 计为 timeout，HTTP reset 仍必须带失败时捕获的 `McpService` 作为身份条件，且不重试 timed-out 工具。服务替换后绝不按 server name 向新 peer 发旧取消。
4. **ACP bridge 限制单列。** 该 transport 的通知转发缺失需要扩展 ACP SDK bridge，属于独立 change；本次不得用“所有 MCP transport 都会收到取消”表述。内部发起可为一致性复用 helper，但端到端接收承诺限于 stdio/HTTP。

## Risks / trade-offs

- MCP cancel 是提示，server 可以忽略；宿主不能把通知成功等同于远端副作用已撤销。
- Drop 无法同步等待通知送达；transport 关闭/进程退出时只能 best-effort。
- timeout 与 drop 竞态必须维持单一 guard 所有权，测试验证同一 request id 最多一次本地发送。Grow 不启用 rmcp handle 自身的 timeout 通知。

## Verification and rollout

使用 in-process rmcp duplex server 捕获完整 JSON-RPC request id 与通知，测试正常完成、MCP 业务错、timeout、future drop、retry 第二轮、旧服务替换与通知失败。direct 入口验证预算和同一 helper。记录实际测试和 ACP 限制；完成相关开发者说明后严格校验/归档。
