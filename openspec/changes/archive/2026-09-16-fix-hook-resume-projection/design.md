## Context

Timeline HookLifecycleProjection 已有 occurrence_id 与 HookCause::Tool.call_id。PublishControlState 在对话重放之后调用 publish_completed_hook_projections，但补发 tool_name/prompt_id 为 None，且复用会 close_rewind_window 的实时发送。Pager 为避免误挂最后一个工具，将恢复中的每个 Hook 作为尾部独立行。实时路径又按 last_tool_call_entry_id 推测工具归属。

ACP tracker 只保存正在执行工具，完成后移除；恢复时完成 Edit 还可能合并到相邻同文件 Edit。因此只增加 DTO 身份而不保留展示归属不能修复问题。

## Goals / Non-Goals

目标是恢复可读且可检查的 Hook 展示、精确归属及纯观察快照。范围不包含持久 schema 迁移、Hook 执行重构、按墙钟拼接 Timeline 与 updates 的伪时间全序。

## Decisions

1. 共用纯投影构造，HookExecution 增加 tool_call_id 与 is_snapshot。工具身份来自 Timeline cause；snapshot 经 passive notification 发送，实时路径保留原有交互边界。
2. Pager tracker 保存当前展示中 ACP tool id 到 EntryId 的映射，跨 turn 保留，合并 Edit 时重定向到存活行；重载随 tracker 重置，查找核对 EntryId 仍存在。精确身份缺失时不猜最后一个工具。
3. 工具 Hook 归属成功时累加对应 phase，occurrence 去重阻止重放重复。实时发生时的正常展示保持；快照里的无锚点 Hook 复用 LifecycleEventBlock / ToolCallHookData，在一个默认折叠的 Restored hooks 条目中保留事件名、occurrence、工具身份、运行详情及说明，不新增持久事件实体。
4. 快照标记来自消息本身，不依赖客户端 loading_replay 的竞态窗口；重复快照不会改写当前 turn 的 stop stash。历史 stop 缺少 durable prompt id 时留在历史记录，绝不冒充当前 turn 的终态 Hook。
5. 正常和子 Agent 视图复用同一 Hook 处理函数，避免根/子会话两份行为。
6. 审计确认实时 ToolCall 经 event_tx 入队，而 Hook 直接走 gateway，发送顺序不代表客户端到达顺序。按 call id 暂存早到的实时 pre/post Hook，在 ACP ToolCall 登记时附着；已知隐藏工具或 turn fence 后仍无锚点的记录保留为显式生命周期行。快照仍按历史语义处理。

## Risks / Trade-offs

- 两个日志没有可依赖的统一 UI 顺序 → 无锚点事实明确放历史记录，不伪造其原始位置。
- 合并 Edit 包含多个工具调用 → 映射重定向并累加 Hook，验证两次完成及多次快照，不覆盖先前详情。
- 长历史仍需保留事实 → 不截断运行数据；默认折叠限制可见噪声，沿用已有按需渲染。
- 未取得截图对应的新会话 ID → 用真实生产处理入口与合成大历史回归，明确现场验证限制。
