## Why
报告指出 doctor 只计第一页，以及初始化期间切换 Agent 后 search_tool 可见而 use_tool 找不到。源码确认后台初始化捕获旧 ToolBridge，而切换只从当时快照恢复，迟到结果可能发布到退休 bridge。
## What Changes
统一 doctor、注册和 descriptor 的完整 tools/list 收集；失败及 cursor 循环不报告部分成功。后台发现完成后向当前 bridge 发布，并将注册/快照发布与 Agent bridge 替换串行化；锁不覆盖远程握手/列表请求。继续检查 catalog generation 和现有资格。对工具未找到提供可证实的本会话注册诊断，不猜测服务状态或授权。
## Capabilities
### Modified Capabilities
- `extension-runtime`: 完整分页发现、当前 bridge 发布及诊断。
## Impact
mcp、shell 的 MCP 生命周期和 tools 的 use_tool 错误上下文及回归。无远端配置更改、自动重试执行或服务器部署修改；不处理报告附录的第三方服务问题。保留其他未提交改动，不自动提交。
