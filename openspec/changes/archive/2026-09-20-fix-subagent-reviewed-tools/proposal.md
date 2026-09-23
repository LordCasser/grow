## Why

子 Agent 当前能看到 runtime 注入的 `write`，却因它不属于 authored identity 而只能永久拒绝；若直接把它加入 authored toolset，ReadWrite/All 初始 RWX 又会使整文件覆盖免审执行。与此同时，权限请求把 `write` 和局部编辑都降级成 `search_replace` + path，主 Agent 无法确认自己审核的具体操作与最终 permit 绑定的是同一调用。

## What Changes

- 工具 descriptor 可以显式声明子 Agent 调用必须逐次经过权限 Gate；该声明同时控制 hard eligibility、能力目录和 initial-RWX fast path。
- `write` 声明为逐次审核工具：只要它在最终 ToolBridge 中存活，子 Agent 可以发现并提交精确调用，但初始 RWX 不直接放行；`search_replace` 保持普通匹配式编辑行为。
- 子 Agent 的权限请求保留实际 wire tool identity、canonical argument hash，以及有界的操作摘要；整文件覆盖摘要包含目标、内容字节数、摘要和明确截断的预览。
- Auto 模式继续使用主会话上下文作单次判断；Ask / AlwaysApprove、显式 deny、protected path 和普通权限规则继续沿既有 PermissionManager 语义执行。
- 能力目录明确区分默认可用、因 RWX 投影而 locked、descriptor 要求逐次审核及永久禁止；可审核项指向精确调用 Gate，visible forbidden 说明当前 child 不可获批及 `ask_parent` 的父级处理通道。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `tool-authorization`: 增加 descriptor-owned 子 Agent 逐次审核、真实调用证据和调用绑定 permit 的行为契约。

## Impact

- 影响 `tool-protocol` descriptor、Grow `write` 工具、ToolBridge descriptor 投影、Shell 子 Agent capability/preflight、Workspace PermissionManager/Auto judgment、Agent 架构说明及相应测试。
- 不新增持久化 grant、会话 capability mutation 或 `ask_parent` 授权协议；不处理 `search_replace` 的跨进程文件版本冲突，该债务继续独立跟踪。
