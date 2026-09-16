# Hook、resume 与 Timeline 核对

## 已确认链路

- Timeline 是 Hook 执行事实源：Triggered → RunStarted/RunFinished/RunSkipped → Completed；`HookCause::Tool.call_id` 已持久保存归属。恢复通过 `Timeline::from_events` 验证折叠，不执行历史 handler。
- `session/load` 先重放 `updates.jsonl` 的 ACP/UI 展示，再通过 actor 的 `PublishControlState` 发布完成 Hook 投影。冷恢复会产生新的 `SessionStart(source=load)`，它与历史重放不同。
- 原快照传入 `tool_name=None`、`prompt_id=None`，DTO 没有 tool id，也没有 snapshot 标志。Pager 通过 loading 状态猜恢复语义，每条历史 pre/post 都变成独立 lifecycle 行；其他历史生命周期亦逐条追加。去重集合只能消除同 occurrence 的第二次到达，无法避免首次恢复时成千条不同 occurrence。
- 原 `publish_completed_hook_projections` 复用实时 `send_hook_execution`，经 `send_transient_hook_notification` 关闭 rewind。发布已存在事实不应引入新的交互边界。
- 实时 ACP 工具事件经 `event_tx` 入队，HookExecution 直接经 gateway 发出；源代码调用顺序无法保证客户端到达顺序。按“最后一个工具”附着在并行/乱序场景不成立。
- Pager 完成工具后清除 pending_tools；相邻同文件 Edit 可能合并删除行。恢复必须保留完成工具身份，并在合并时转移归属，不能仅查询 pending_tools。
- 子视图 Grow 路由原来没有处理 HookExecution。本次复用根视图处理以保持会话隔离。

## 边界

本次没有重写 Timeline schema、Hook 执行策略或两个日志的顺序协议。无可靠工具展示位置的快照事实明确放入历史条目，保留 occurrence 与事件标签；不猜测它们在 ACP 对话中的原始位置。恢复仍会查询全部完成 Hook，传输体积与历史长度相关，当前不引入第二个持久游标或新的 Hook 历史协议。

截图对应的新会话 ID 未提供，未修改任何用户会话文件；现场 session_start/session_end 的具体失败原因无法仅由截图确认。此前提供的会话 ID 属于上一项问题，不据此冒认新截图的会话。
