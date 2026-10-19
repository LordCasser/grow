## Why

session `01a15323-b21a-7403-906e-72048e8b7594` 切换 Agent 后，Plan 审批成功，但 `todo_write` 返回 `Planning runtime is unavailable`，没有产生非空 TODO 投影。代码只在启动时绑定 Planning runtime，Agent 更换后遗漏；此外顶部 Plan 入口依赖本地正文缓存，Executing 阶段关闭审批后会失去入口。前者违背 session-owned planning 的既有契约，后者遗漏可发现的阶段展示。

## What Changes

- 启动和 Agent 替换共用当前 session 的 Planning runtime 与 Timeline 计划投影绑定，使 TODO 在切换后仍可读写。
- Plan 活跃时在既有顶部状态栏展示当前阶段，入口不依赖 TODO 或正文缓存；点击沿现有不可变 artifact 查询打开预览。
- 补充切换后实际 TODO 调用、投影保留、无缓存阶段显示与点击路径回归。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `task-planning`：Agent 替换保持 session 局部计划可用和既有状态。
- `client-surfaces`：Plan 顶部阶段入口独立于正文缓存和 TODO，保留原查询及审批边界。

## Impact

Shell 的 planning hydration、Agent 激活与启动；Pager 的 Plan chip 与现有顶部渲染。复用 Goal 使用的 AgentStatusBar，不创建新状态 owner、通知协议或详情框架，不修改权限、审批或 Goal 生命周期，不篡改用户历史 session。
