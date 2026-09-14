## Why

父 agent 调用 `send_subagent_message` 时，当前 TUI 把已知的工具名、目标和正文丢掉，只剩 `Tool call` 与 `{ id, status: received }`。子 agent 的消息已持久接收并会进入模型上下文，但没有对应的 TUI 接收消息；同 session 问答与跨 session 问答另有接收展示，方向和信息层级也不一致。

2026-09-14 用户已授权补齐盲区并实施。以下 delta 在实现与验证完成前不合入主规范。

## What Changes

- 发送侧保留真实 tool call，从开始到完成显示具体工具、通信对象、消息/问题预览、投递模式与实际结果；展开保留全文和关联标识。
- 子 agent 持久接收父消息后，在自己的 TUI 显示一条可回放的系统通知，明确来源和正文，不增加模型输入或抢走当前视图焦点。
- 同 session 问答按实际方向区分父问子、子问父；跨 session 问答明确标注另一主会话。保留现有 Sideband 执行、权限与单行生命周期。
- 接收通知与发送工具共享信息层级，保留各自的状态语义：消息的 `received` 是持久接收，问答的 `answered` 是回答完成。
- 保留父消息的历史正文，使接收通知可以在消费、重连、冷恢复后从 Timeline 事实重建。`updates.jsonl` 继续是可丢失投影。
- **BREAKING**：调整父消息 artifact 的内容表示和保留范围，以及直接委派 inquiry 的展示方向字段；同步更新格式校验，不设计旧格式兼容分支。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：通信工具的可读展示、父消息接收通知、长文本与多终端模式的展示边界。
- `local-coordination`：询问展示识别实际对端和通信方向，保持 peer 与委派语义。
- `session-timeline`：父消息接收正文保留并可重建 UI，消费与回放不重复投递。

## Impact

涉及 Shell 工具开始/结果投影、SubagentCoordinator 的权威目标信息、inbox 接收与清理、询问 audit、Pager tracker/notice/重放合并，以及 normal/minimal 验证。开发者说明同步更新 `docs/architecture/local-coordination.md`。代码证据与展示稿见 [design.md](design.md)，实施验收见 [tasks.md](tasks.md)。

不增加任意跨 session 发指令、兄弟 agent 互发、已读协议、后台轮询、中心消息总线或独立聊天面板。通用工具穷举治理和被动问答行的整体架构重构独立处理。
