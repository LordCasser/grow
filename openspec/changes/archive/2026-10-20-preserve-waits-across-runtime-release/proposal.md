## Why

审计 A1 已确认恢复放行到新人工交互 attempt 建立之间存在窗口；再次挂起会丢失 checkpoint 中仍未完成的等待，并伪造 OutcomeUnknown。需要区分物理 attempt 退役与逻辑等待完成。

## What Changes

ExecutionReleased 仅保留匹配 checkpoint 明确保留的人工等待；新 attempt 接管，真实解决与 ToolResult 优先。增加跨周期重挂起回归。

## Impact

局限于 suspension 的 Timeline 交互折叠与定向测试，不新增持久格式或运行 owner。
