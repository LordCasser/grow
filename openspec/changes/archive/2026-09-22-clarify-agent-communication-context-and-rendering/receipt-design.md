# send_message 回执设计

`send_message` 改变接收方后续上下文；`ask_question` 用 InfoRequest Sideband 回答，不把问答追加到接收方主上下文。这是运行时与工具说明的契约，不需要在每条 TUI 记录中重复解释。调用方的工具调用与结果仍按普通工具进入调用方上下文。

本文用这两个名字指两类行为。当前实现对应 `send_subagent_message` 与 `ask_parent` / `ask_subagent` / `ask_session`，不在 UI 阶段重命名工具，也不把 parent→child 的写权限扩成任意 peer 投递。第二阶段仅沿已有收件关系增加意见回复，使用相同回执规则。

## 回执确认什么

用户已确认回执只承诺 **Received：接收方已持久接收**。这是已有发送工具的完成条件，不等待接收方跑完当前工作，更不等待模型生成一句“收到”。意见回复使用另一条正式消息，见 [exchange-design.md](exchange-design.md)；不能把 Received 偷换成 Applied。

需要区分三个事实：

| 事实 | 权威证据 | 含义 |
| --- | --- | --- |
| Received | 目标 Timeline 的 `Notification::Received` 越过 durable barrier | 消息已保存，可恢复；发送工具可以返回 |
| Consumed | 原 receipt 被 `Notification::Consumed` 引用且携带对应模型输入 | 消息已加入目标 Surface，不等于 provider 已成功看到或按指令执行 |
| Task completed | 目标任务自己的终态结果 | 属于任务生命周期，不是这条消息的回执 |

界面默认只显示 `Received`。`Read`、`Applied`、`Done` 不能从 ACK、空闲或后来出现的回答推断。模型主动回复的正文也是新内容，不能充当可靠传输 ACK。

## 实施前基线与缺口

1. `tools/.../task/interaction.rs::interact` 用 `ctx.call_id` 作为发送 ID；Coordinator 把 source session 由 runtime 注入。
2. `shell/.../subagent_coordinator/interaction.rs::run` 将发送交给目标 `ReceiveParentMessage`，等待最多 30 秒。
3. 目标 `receive_notification` 先写正文 blob，再 durable 提交 `Notification::Received`。ChatState 按 source identity / version 去重，并核对 owner、payload 和 interrupt 是否一致。
4. 接收端返回真实 notification receipt ID；Coordinator 当前在 `Ok(Ok(Ok(_)))` 中丢弃它，只返回原调用 ID 与 `status: received`。
5. ACK 超时、取消和部分失败被压成 String；Pager 的 `is_unknown_delivery_error` 通过英文字符串匹配判断未知状态。该分类不应由文案承担。
6. Coordinator 的 active route 检查和 Session 的 active-turn 检查早于 receipt 去重。底层去重成立，并不证明 child 结束后重新调用发送入口仍可拿到旧回执。

第 4、5 项属于本次回执闭环。第 6 项通过将“核验已有回执”和“接纳新消息”分开解决，不放宽向 inactive child 写入的权限，不混入取消/resume 生命周期重构。

## 最小数据设计

继续用现有调用 ID、receipt ID 和目标 Session，不新增 message ledger、ACK 通知 inbox 或 receipt 状态机。

- `id`：原发送操作 ID，仍来自 tool call ID，不由模型生成；`source_session_id + id` 标识原发送操作，target_session_id 必须与该操作的原参数一致，不能在重试时换目标。
- `receipt_id`：目标已存在的 `Notification::Received.id`，收到 ACK 后原样向调用方返回。不能用调用 ID、UI event ID 或 hash 猜一个替代。
- `status`：发送结果只允许 `received` / `rejected` / `unconfirmed`；`Sending` 仅是工具尚在运行时的 UI 状态。
- `error.code/message`：结构化原因，由 Runtime 分类后交给展示层；不再通过 message 文案确定投递状态。

保留 `AgentInteractionOutput` 的既有外部字段 `id/status/target_session_id/subagent_task_name`。发送分支补充真实 `receipt_id` 与结构化错误；内部用封闭的发送 outcome 构造输出，保证 received 必有 receipt、rejected/unconfirmed 不伪造 receipt。询问分支仍使用 inquiry 自己的状态，不将两个状态集合混成通用生命周期。

发送工具的输出约束如下：

```json
{
  "id": "call_17",
  "status": "received",
  "receipt_id": "<target Notification::Received.id>",
  "target_session_id": "<child session>",
  "subagent_task_name": "Review parser"
}
```

这不是为了保留旧协议增加双栈：内部 Rust 调用方、tool schema、ACP raw output 和 Pager 在同一 change 一起更新。旧历史不重写，历史数据的读取策略见后文。

## 正常时序

```text
source tool call (id)
  → authorize existing direct parent-child route
  → target validates body and delivery arguments
  → persist immutable body
  → commit Notification::Received(receipt_id)
  → return received + receipt_id
  → source commits its ordinary tool result
  → TUI updates the original source tool row to Received

at the target's existing safe boundary
  → consume the same receipt once
  → append attributed agent guidance to target Surface
```

目标 UI notice 和 ACK 都派生自同一个 Received 事实。UI 发布失败不回滚 receipt；恢复可以补显示。回执回到 source 后通过一次普通工具结果进入 source 上下文，不再额外塞一条“message received”通知，否则是重复交付。

不为 ACK 新开 Sideband、不让对方模型生成 ACK、不在父消息路径复用 inquiry audit。`interrupt` 仍控制当前执行何时安全让出，既不改变 durable ACK 条件，也不写成正文里的“下步投递”。

## 失败、取消与幂等

| 边界 | 对外结果 / TUI | 处理 |
| --- | --- | --- |
| 校验/授权拒绝、命令尚未入目标队列 | `rejected` / `Rejected` | 可以确认本次未接纳，保留原因 |
| 正文持久化失败，确认尚未提交 Received | `rejected` / `Failed` | 返回真实错误；不发成功回执 |
| 已派发后超时、通道关闭或 source 取消等待 | `unconfirmed` / `Unconfirmed` | 目标可能已经保存，不能标记未送达或撤回 |
| 已确认 Received，随后源工具结果保存失败 | 目标仍 Received；source 结果未确认 | 不重新投递；恢复沿用原 tool-call identity 核验 |
| 目标 UI 投影失败 | `received` / `Received` | 修复显示，不重复接纳 |
| 同 identity、同原文与同模式的重复请求 | 同一个 Received / receipt ID | 不新增 receipt，不再次消费，不再次请求 interrupt |
| 同 identity 但正文/模式/目标冲突 | 拒绝冲突 | 不改旧事实，不将冲突当作“更新消息” |

持久写入错误不能仅因返回 Err 就归为 rejected。只有能证明尚未提交时才能声明未接收；可能已经过提交边界的取消、ACK 丢失或读状态不可用全部保守归为 unconfirmed。

ACK 与取消同时就绪时先取已经可用的确定回执。若仍未知，可在同次操作剩余预算内做一次只读 receipt 核验；核验不能阻塞等待另一轮目标采样。读不到或超时就返回 unconfirmed，不能无限重试。

只读核验复用既有 ChatState / 验证后的 Timeline 事实：先校验原 source 对目标的直接 ownership，再按原操作 identity 查 receipt，核对正文引用和 interrupt。新消息仍必须通过 active route/turn gate；查已存在的 receipt 不要求目标仍在运行，也不能创建 writer、启动 child 或投递新消息。没有足够授权/完整历史时返回无法确认，不通过扫描任意 Session 绕过权限。

**没有自动重发。** 模型再次调用工具会得到新 call ID，是新消息；不能按正文相同去重，因为用户可能有意重复发送。内部同操作重试必须携带原 identity；不新增由模型填写的幂等参数，不把 `get_inquiry` 用作消息回执查询。

同一次等待尚未完成时，确定 Received 可以取代 Unconfirmed 候选；工具结果一经提交不原地修改。历史核验可补充只读详情，但不能伪造先前模型已经收到另一份工具结果。

## 与现有能力如何衔接

| 现有能力 | 本次处理 |
| --- | --- |
| parent→direct child 权限 | 保留；不新增 peer send、child→parent 指令注入 |
| queued / interrupt | 保留执行语义，移出常规 UI；原始参数可在 Data 检查 |
| Timeline Received / Consumed | 原事实权威，使用已有 source version 与 receipt ID；不新增事件或迁移账本 |
| source tool row | 原 tool-call ID 原位更新；不加发送通知或独立回执行 |
| target ParentMessageNotice | 仍由 Received 重建，`parent-message:<receipt ID>` 去重；默认显示 `Message from parent` 和正文 |
| 已保存的 `status: received` 旧工具结果 | 可以显示 Received；缺 `receipt_id` 就在数据中缺省，不伪造，也不因展示升级重发 |
| 旧记录的 String 错误 | 只保留历史错误正文，不能用英文关键词升级成精确的 received/rejected；缺提交边界证据时显示 Unconfirmed |
| 缺正文或不支持的 payload version | 沿用严格恢复规则和 `Message unavailable`；不从包裹 prose 猜原文 |
| normal / minimal / replay | 接收记录是不可变 receipt，只显示一次；Minimal 不打印第二条 ACK；UI 恢复不重复模型输入 |
| 询问 Sideband | 不变；回答进入调用方工具结果，不进入目标主 Surface |

旧历史支持是对已有持久事实的解释，不是维护两套发送协议。UI 仅消费当前 typed outcome；有限的旧记录缺字段处理放在读取/投影边界。

## 如果将来要求消费回执

消费回执只能由目标 `Consumed` durable commit 后产生，引用同一个 receipt ID；只有确认包含该消息的输入才可标记 `Consumed`。它不等于 provider 成功、agent 理解或任务完成，命名不使用 Applied/Read。

它不能延迟现有 received ACK：child 可能正等父任务的工具结果，两边互等会造成协作停顿。需要时应是独立、可恢复的状态观察；不能在已经完成的发送工具里偷偷再追加结果。是否向 source 模型投递消费通知也需要显式契约，否则只作为 UI 观察。本变更不实现消费回执，避免为了收件确认引入额外异步状态流。

## 后续验证

先跑现有父消息与 Timeline 回归，再补：真实 receipt ID 端到端保留；unknown 不靠字符串；取消/ACK 竞争；commit 前后故障；source 保存失败；同 ID 同 payload 去重及冲突；已结束 child 的只读回执核验与新投递拒绝；冷恢复/Minimal 不重复；Received 前后和 Consumed 前后两端 provider request 对照。

证据：`crates/codegen/tools/src/implementations/grow_build/task/{interaction.rs,coordinator.rs}`、`crates/codegen/shell/src/agent/mvp_agent/subagent_coordinator/interaction.rs`、`crates/codegen/shell/src/session/actor/notification_drain.rs`、`crates/codegen/chat-state/src/actor/mod.rs`、`crates/codegen/chat-state/src/timeline.rs`、`crates/codegen/pager/src/acp/tracker.rs`。故障注入与运行验证结果见 verification.md。
