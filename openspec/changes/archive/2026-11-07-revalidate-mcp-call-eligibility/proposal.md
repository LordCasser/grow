# Revalidate MCP call eligibility after initialization

## Why

Backlog MCP-F1 在当前源码仍成立：模型路径 `McpErasedTool::run` 与 Shell direct `call_mcp_tool` 只在等待初始化前取得 client；`ensure_initialized` 返回后直接向该 client 发 `tools/call`。初始化期间的移除、同名替换或禁用不会使被调用任务持有的 Arc 消失。当前 `is_current_client` 只保护错误后的 transport reset，不拦首次发送。继承服务还必须复核现有 upstream authority，不能只检查 child 的尚未 reconcile 的 map。这里记录的是已核实的静态发送链，真实旧红须在实施时取得。

## What changes

由 MCP call owner 在异步初始化完成后、单次 `tools/call` 派发前验证原 client 仍是本次 session 的当前 client、工具未禁用；选中 inherited client 时还须满足现有 upstream 绑定与工具资格。直接入口把已有 McpState 传到同一 owner，不能在 Shell 提前检查后再次留下初始化窗口。撤销后明确失败，不切换到同名 replacement，不恢复或重放旧操作。原 admitted deadline、一次取消与不确定结果语义保持。

## Impact

主要落点 `mcp/src/servers.rs`；Shell direct 调用方仅适配会话状态参数。内部共用现有 McpState/SharedMcpEligibility，不增加权限框架、registry、wire 字段或配置。SDK reverse owner MCP-F2、目录/供应商函数名上限和 elicitation 均不在本项。
