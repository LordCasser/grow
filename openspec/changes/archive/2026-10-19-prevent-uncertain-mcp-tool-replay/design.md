## Evidence

当前 `try_call_tool → should_recover_service_error → recover_and_retry` 对 TransportClosed/TransportSend 和部分 HTTP MCP error 二次调用 `call_tool_cancel_aware`。`McpClient::call_tool` 的 direct 入口原本不重放；本 change 将普通注册工具对齐这一安全边界。官方 rmcp 3.5.1 仍持有每轮 request ID/peer，但 Grow 目前没有可以在上述失败上确认“未发送”的 durable dispatch 证据。

## Decisions

1. 原始工具调用只派发一次。参数、只读提示、错误 code 或新 wire ID 不授权重放。
2. 可恢复连接错误仅条件重置捕获的旧 service；不重置已替换的新实例，不在失败调用里重新握手并调用工具。下一次明确调用复用 `ensure_initialized`，不是原调用重试。
3. 传输丢失和 deadline 结果说明调用可能已产生效果。保持现有工具错误与取消 guard，不把不明结果标为成功，也不发送第二份取消通知。
4. 不修改工具 discovery、SDK initialization、用户显式 reconnect 或 MRTR 门禁。已知的业务/参数/认证失败按原信息返回。

## Verification

受控 server 先计数写入、再丢弃回复，断言 Grow 不第二次发送；另验证下一次显式调用能重新连接。保留正常结果、业务错误、timeout、取消、旧实例被替换及同名新实例的回归。只读/幂等注解不得放宽此规则。
