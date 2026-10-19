## Why

Backlog 源码核查确认：普通 `McpErasedTool::try_call_tool` 在 transport 和部分 HTTP MCP 错误后通过 `recover_and_retry` 再发一次原 `tools/call`。当前 SDK 错误没有可证明“工具请求尚未发送”的 Grow 接纳事实；服务可能已完成写入，只丢失回复。相同参数或工具只读提示不能证明可安全重放。

## What Changes

- 已开始的普通 MCP 工具调用失败后不自动重发；传输中断和超时明确保留执行结果未知的信息。
- 需要恢复的旧 transport 只按原实例条件失效，下一次独立调用沿已有初始化/恢复入口连接。
- 正常结果、业务 `isError`、确定的参数/认证拒绝和原 request/peer 的取消通知继续走现有边界。
- 不开启暂缓的 MRTR，不增加协议循环、幂等推断或第二套恢复状态。

## Capabilities

- Modified: `extension-runtime`。

## Impact

`mcp/src/servers.rs` 普通工具派发、其恢复测试及开发者说明。失败后模型会看到明确未确认结果，需先核对服务状态，再决定是否发起新的操作。
