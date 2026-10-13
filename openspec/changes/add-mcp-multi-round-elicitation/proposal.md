## Why

用户需要 MCP 工具在同一次逻辑调用中补充结构化输入，或引导用户完成外部 URL 交互。Grow HEAD `7b7d56ab98a686223bc659567c79c1363729cd66` 已有 MCP transport 身份、取消/恢复约束和会话 pending registry，但没有完整 Form/URL 产品路径。

2026-09-23 的归档核查已确认规范与 SDK 前提成立；不再等待标准成熟。本次用户授权建立提案，不授权实现产品代码。真实第三方服务尚未选定及验证，不能用 SDK 或 mock 支持代替互操作结论。

## What Changes

- 升级官方 rmcp 并完成依赖/feature 兼容，使用 SDK 的 MCP 2026-07-28 MRTR：在原 logical tool call 内接通用户交互；wire id、inputResponses、opaque requestState 的协议机制由 SDK 提供，不自研协议循环。
- 提供 Form 与 URL consent/waiting 的后端到 TUI 闭环，区分 accept、decline、cancel、外部操作完成和结果未确认。
- 沿现有 SessionActor、工具调用及 pending registry 绑定交互身份，新增有界交互队列和统一焦点裁决；不采用上游 single-slot 静默替换。
- 保留 Grow 的 transport generation、工具授权和取消边界；敏感凭据不进入 Form，URL 打开与业务完成分离。
- 明确首版不自动跨进程重放未完成的 MCP 写调用，不新增 workflow owner 或人类 prompt 路由。
- 将 per-request 能力声明、整轮接纳、单次 continuation、等待期间准入复核和原结果类型保全列为最小闭环；不沿用通用 recover-and-retry 重发已可能执行的请求。

## Capabilities

### New Capabilities

无；扩展既有 capability，不建立通用 HITL 工作流框架。

### Modified Capabilities

- `extension-runtime`：MRTR、Form/URL、调用关联、取消与等待、安全及支持能力声明。
- `client-surfaces`：表单、外部交互、有界排队、会话焦点与草稿保全。
- `session-timeline`：MCP 交互和原工具调用的事实关联、终态及恢复边界。

## Impact

入口为 `crates/codegen/mcp/src/servers.rs`、`shell/src/session/pending_interaction.rs`、Shell MCP 事件/工具 dispatch、Pager 会话交互和共享协议类型。实现时更新开发者说明，并按实际事件形状决定 Timeline schema 是否需要升级；不能声称 v25 已有新交互事件。

保留顶部 task 布局；不采用统一 Dock，不跟进 Memory、模型→上下文窗口→推理强度连贯选择、Remote/Agent Host 或其他大型 TUI 功能。Rhai workflow、Goal 和人类输入队列的 owner 不变。

## Evidence and Status

上游仅针对两项主题评审至 `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`，详见 [评审边界](../record-upstream-review-2bdd1d6/review.md)。本提案及 delta 是未来要求，尚未实现；[验收矩阵](acceptance.md) 的 M-01～M-25 未执行。
