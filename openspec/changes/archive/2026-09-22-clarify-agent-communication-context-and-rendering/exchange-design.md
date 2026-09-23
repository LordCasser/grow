# 意见交换与 Sideband

用户已明确：回执只确认接收；agent 需要交换意见，来往内容进入双方上下文，并呈现为特殊工具结果。建议保留两种语义：**ask 用于咨询，send 用于正式通信**。意见回复仍然是 send，不新增 discuss、聊天室、协商轮次或自动共识状态机。

## 两种行为

| 操作 | 目标如何处理 | 双端上下文 |
| --- | --- | --- |
| `ask_question` | 使用目标冻结上下文做一次无工具 Sideband 回答 | 调用方保留问题与工具答案；目标主 Surface 不追加本次问答 |
| `send_message` | durable 收件，返回 Received；在目标安全步骤交付 | 发送方保留自己的发送调用与回执；接收方保留一份有来源的 agent 消息 |
| 回复一条消息 | 目标正常前台决定内容，执行带 `reply_to` 的 send | 双方各保留自己的发言和对方的回复；不修改原回执 |

两个名字在本文表示目标语义。现有 `ask_parent/ask_subagent/ask_session` 已实现第一类；变更前 `send_subagent_message` 只实现 parent→child。新增双向回复是真实的能力变化，不能仅通过 TUI 改名假装已有。本轮不连带统一工具命名或删除旧工具；实际工具 schema 在第二阶段按以下路由约束收敛，避免同时保留多个含义重叠的发送入口。

## 一次意见交换

```text
A: send_message(B, "I suggest keeping the parser local. Thoughts?")
   ← Received(m1)
B: [receive_agent_message result: from A, message m1, body ...]
B: 正常前台阅读、推理，必要时用自己的工具核对
B: send_message(reply_to=m1, "Agreed; keep width calculation shared.")
   ← Received(m2)
A: [receive_agent_message result: from B, message m2, reply_to m1, body ...]
A: 正常前台继续任务；需要时再发一条消息
```

这里的 Received 是运行时 ACK，不是对方 agent 的意见。B 可以不同意、暂不回复或者回复时补证据；ACK 不等待这些动作。避免将双方阻塞在“等对方完成回复”的发送调用上。进度通知、工具次数和 ACK 的 UI 副本不进入模型上下文。

消息消费是后续请求中的可见性保证，无法保证已经启动的 provider request 看到新意见。正在执行时沿用现有安全步骤/显式 interrupt；不能原地改正在采样的输入。

## “特殊工具结果”如何成立

当前 `ConversationItem::ToolResult` 必须引用配对的调用；`project_portable_history` 会丢弃孤立结果。接收方没有执行发送方的 send，不能把 A 的 tool_call_id 拿到 B 的历史中直接追加 ToolResult，也不能把消息拼进 B 无关工具的结果。

建议只增加一个必要的语义入口：**有明确来源的 agent-message context item**，例如 `ConversationItem::AgentMessage`。它从现有 inbox receipt 消费生成，携带 receipt/source/target/reply_to/body，不是新账本。落地流程：

1. Received 仍在目标既有 Timeline / inbox，保存原正文和身份。
2. 安全步骤提交 `Notification::Consumed { input: Some(AgentMessage(...)) }`，消费与精确输入保持同一原子事实。批量消费可由一个 item 承载有序消息；item 的每条消息仍保留独立 receipt。
3. provider-neutral request projection 将这个 item 展开成专用的 `receive_agent_message` 调用/结果对。call ID 由目标 receipt identity 确定，与真实模型 tool-call ID 命名空间隔离；调用参数只引用收件身份，结果携带来源、正文和 reply_to。
4. 该调用是 runtime delivery 的协议表示，不是模型主动决定执行的动作。它不进入可执行工具 dispatch，不重复触发工具副作用，不计入模型发起的工具次数；Timeline 始终保留 runtime 来源。
5. UI 将这一个逻辑事实显示为 `Message from <agent>`，不把合成调用和结果拆成两行。

Source 的出站正文已经在自己真实 send 参数中，不再把同一正文作为额外接收消息追加一次。收到回复时才出现自己的 `receive_agent_message` 结果。**双方保留完整来往意见，不要求同一意见在一端重复存两份。**

专用 item 是为了保留身份、权限来源和消费原子性。直接持久化“伪装成模型发起的 Assistant + ToolResult”反而需要事后辨认；直接沿用无结构的 user-role 文本又无法满足特殊结果的语义。此次不顺带把所有 monitor/Goal 通知改成该类型。

### 必须验证的协议边界

实施必须检查实际 adapter 产出的请求；本地 wire 验证不代替真实远端 provider 的兼容性验收。

- 三类现有 endpoint adapter 必须得到相邻、完整、唯一 ID 的 call/result 对；当前真实工具批次未闭合时先完成该批次，再插入消息。
- runtime-only `receive_agent_message` 是历史交付表示，不新增给模型调用的轮询工具；需验证各 endpoint 对这种历史调用的支持，失败时不得静默丢消息或退化成孤立结果。
- stable Surface identity 属于 canonical AgentMessage；一对 provider item 只是一个事实的展开，不能使 compaction/input_ref 坐标整体偏移。
- portable history、Sideband 冻结、compaction、token 统计、replay/export 和 provider 切换都必须理解新 item。已消费意见可以被后续 ask 的冻结上下文读取；尚未消费的 inbox 不能冒充已进入 Surface。
- runtime item 不携带 `PermissionEvidence::DirectUser/Interjection`；发言内容不能通过自称“parent/user/system”取得权限。

## 是否结合 Sideband

| 方案 | 代价与结果 | 取舍 |
| --- | --- | --- |
| 正常前台收到消息后决定是否回复 | 使用真实任务上下文，可查证、调整工作；回复要等安全步骤 | **推荐作为正式意见交换** |
| Sideband 生成答案，同时自动写入双方主上下文 | 冻结答案可能与前台刚作出的决定冲突；多出跨端提交、revision 冲突和回复去重语义 | 不作为默认 send 行为 |
| 先 ask，再由调用方明确 send | 快速咨询，调用方决定哪些内容值得通知对方 | 支持组合，不增加第三种工具 |

只复用底层执行设施不代表应该合并语义。Sideband 的优势是独立、无工具、不修改主任务；把它自动当成代表目标前台的正式意见，会让目标后来看到一段自己主任务从未认可的发言。

如果 A 想把 Sideband 答案纳入共同讨论，应显式 send，例如引用“the earlier snapshot suggested X”，保留引用来源。它成为 A 发出的讨论材料，而不是伪造 B 已正式承诺 X。无需为此做双端事务或共享讨论数据库。

## 回复路由与授权

当前契约禁止 child upward intervention，也没有 peer Send IPC，必须区分“回复意见”和“获得新指挥权”。

最终发送 schema 保留实际字段 `subagent_id`，与 `reply_to` 二选一：

- 初始 target 发送先保留现有 parent→direct child 授权和显式 interrupt 能力。
- `reply_to` 必须指向当前 Session 实际持久收到的消息，其消息身份为原发送者 Session 与原操作 ID 的组合；receipt ID 用于本地消费去重，两者不能混用。Runtime 从 receipt 解析对端，核对原双方身份与会话归属；不信任模型另给的 source/target，不仅凭猜中的 ID 放行。
- 允许沿已授权交流关系反向回复；child→parent 的 reply 是意见和证据，不是更高权限指令，不能请求 interrupt、绕过审批或更改父 Goal。
- 沿 reply 链保留原两方和原授权关系；没有 receiver receipt 证据的转发、第三方回复、sibling 发送均拒绝。`reply_to` 只做关联和回复授权，不建立新 Conversation 实体。
- parent 恰好等待该 child 时，收件与 ACK 通过独立 inbox/actor 路由完成，不等待父 wait 工具。上下文仍要在 wait 正常返回或既有可中断等待安全让出后交付，不能拆断活跃工具批次；不能把 ACK 的及时性误称为前台已经看见回复，也不新增阻塞双方的同步等回复 RPC。
- idle 会话可按既有 notification admission 接纳一次消息驱动的 turn，但不能恢复已归档/关闭的任务、越过暂停/停止状态或自动重启目标。需要为该类来源明确 autostart 策略，不能默认继承所有 notification 的唤醒行为。

任意 primary↔primary 的初始 send 是独立的写授权扩展，建议后续再做：须增加明确的目标接纳策略与 Send/Receipt IPC，不复用 ask 的读取许可。现有 peer ask 仍可用于跨会话咨询；本次回复闭环不顺带开放任意消息注入。

## 防止意见交换失控

Received 不产生自动回复，也不触发 ACK 的 ACK。收到内容是否需要回复由前台模型依据任务决定；已有 token/turn/cancellation 限制继续生效。不新增定时轮询、自动“达成一致”循环或每条消息唤醒一组 Sideband。消息中断不是默认选项。

双方并发发言按各自 inbox 的 durable receive order 消费，reply_to 保留引用关系，不声称存在跨 Session 全局总序。每个 receipt 只消费一次；重连不补跑讨论，不把已完成 send 再执行一次。

## 分两步实施

1. 先完成简洁英文 TUI、Markdown 和明确 receipt ID / typed outcome；保持当前父消息输入机制，不能提前声称已支持特殊工具结果或双向回复。
2. 再实现 agent-message item、request projection、反向 reply 的授权及调度；同步 `session-timeline`、`model-sampling`、`local-coordination` 的 delta 与跨 provider/恢复测试。

两步均已获用户批准，分别实现和验证；第二步作为明确输入协议变化记录。实现与验证结果见 verification.md。
