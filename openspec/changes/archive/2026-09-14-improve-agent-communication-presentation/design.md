## Context

这是 2026-09-14 对当前工作区的源码排查与实施设计。用户已授权补齐盲区并改进。工作区已有大量未提交改动，结论包含这些实际代码；没有把归档文件的验证结果当成本轮测试结果。截图确认了展示症状，尚未取得截图对应进程的版本和 ACP 原始事件，因此不能证明截图来自当前构建。

这里的“同 session”是产品中的父子委派关系。实现上 child 仍有自己的 `child_session_id` 和 SessionActor，必须按 child session 路由通知。“不同 session”指本机 peer runtime 之间的 primary session 协调；当前没有任意跨会话单向消息接口。

| 改动前交互 | 改动前发送侧 | 改动前接收侧 | 排查结论 |
| --- | --- | --- | --- |
| 父 → 子 `send_subagent_message` | `Tool call`，结束后输出 `id/status` JSON | inbox 持久接收，安全边界消费，无专门接收通知 | 用户截图对应的直接缺口 |
| 父 → 子 `ask_subagent` | 同样落入 `Tool call`，结果含 answer/error | 已有 inquiry 行，按子任务名显示 Answering/Answered | 子视图会把自己标成回答对象，方向不明确 |
| 子 → 父 `ask_parent` | 同样落入 `Tool call` | 父视图已有 Answering/Answered subagent task 行 | 接收身份基本可用，发送信息缺失 |
| 主会话 A → B `ask_session` | 有工具名和目标 session ID；问题未得到专门预览 | 已有按来源 session 标识的 inquiry 行 | 功能较完整，展示仍偏协议字段 |
| `list_active_sessions` / `get_inquiry` | 普通工具结果，可展开 | 不产生新的接收请求 | 查询自身成功与查询到的 inquiry 失败要继续区分 |

### 改动前证据与验证缺口

- [preparation.rs](../../../../crates/codegen/shell/src/session/actor/tool/preparation.rs) 的 `send_tool_call_start` 保留了 typed `raw_input` 和 `grow/tool` 元数据，但没有匹配 `AskParent / AskSubagent / SendSubagentMessage`，全部使用兜底标题 `Tool call`。
- [tracker.rs](../../../../crates/codegen/pager/src/acp/tracker.rs) 的通用分支对非空标题、`Other` kind 给空 summary，正文只取 `content`；输入中的目标、message、question 不进入可读展示。
- [interaction.rs](../../../../crates/codegen/shell/src/agent/mvp_agent/subagent_coordinator/interaction.rs) 在父消息 durable ACK 后返回 `AgentInteractionOutput { id, status: received }`；[acp_conversion.rs](../../../../crates/codegen/shell/src/session/acp_conversion.rs) 已覆盖此结果类型。当前问题不是结果完全没有发给 TUI。
- [notification_drain.rs](../../../../crates/codegen/shell/src/session/actor/notification_drain.rs) 的 `receive_parent_message → receive_notification` 只写 inbox，`drain_active_notifications_excluding` 把消息消费成模型上下文；这些路径没有对应的 UI 接收投影。`cleanup_notification_payloads_under_gate` 和恢复 sweep 只保护 pending 引用，已消费正文可被回收。
- [inquiry.rs](../../../../crates/codegen/shell/src/coordination/inquiry.rs) 的 `IncomingInquiryAudit` 已持久保存子任务名，但没有区分 parent-to-child 和 child-to-parent，标题总是选择这个子任务作为参与方。[coordinator.rs](../../../../crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs) 实际知道双方关系与权威任务描述。
- [session_notification.rs](../../../../crates/codegen/pager/src/app/acp_handler/session_notification.rs) 已支持将 `UiNotice` 路由进 child view；接收 inquiry 复用 `OtherToolCallBlock`，按 `(sourcePeerId, inquiryId)` 原位更新，发送端 audit 则隐藏以免重复工具行。
- [coordination.rs 测试](../../../../crates/codegen/pager/src/app/acp_handler/tests/coordination.rs) 中 `coordination_source_tools_keep_normal_running_rows_and_full_return_values` 手工用 `ask_parent` 创建开始事件，只经过真实的结果转换。因此它无法抓到 Shell 开始事件仍叫 `Tool call`。现有父消息测试证明持久、归属、消费与时机，没有检查子 TUI 接收消息。

主规范已经要求父消息 durable receipt、exactly-once 消费和 inquiry 接收展示，但没有要求父消息的可见接收通知。前者当前不是已证明的投递失败；后者需要新增 UI 契约。2026-09-11 的修复明确只做问答任务名展示，本轮需要修改它的方向语义，完整保留其他场景。

## Goals / Non-Goals

用户不用展开 JSON，就能知道调用了什么工具、发给谁、发了什么、现在是什么状态；切到接收 agent，可以看到同一条通信的接收记录。

本轮完成这个闭环。保留直接父子权限、active-child 限制、工具安全中断边界和 Sideband 的无工具/前台隔离。通用工具展示注册机制、独立通信面板、任意 agent 聊天、已读回执都不进入本 change。

## Decisions

### 1. 同一套信息顺序，保留两种执行语义

字段顺序为：动作/工具 → 对端 → 状态与投递模式 → 正文预览。工具名保留真实标识，说明使用自然语言。以下中文是语义稿；最终英文措辞跟随仓库现有 TUI，不做全局本地化。

发送侧默认展示标题和最多两行正文预览，例如：

```text
◆ send_subagent_message → subagent「排查工具展示」 · 已接收
  下个安全步骤加入上下文
  请补查接收侧 TUI，保留消息来源和正文。
```

接收侧是一条由系统产生的通知，来源明确标成父 agent，正文是父 agent 原文：

```text
COORDINATION  收到父 agent 消息 · 下个安全步骤加入上下文
  请补查接收侧 TUI，保留消息来源和正文。
```

它复用 `NoticeBlock` 的系统样式，不作为 child 自己的工具调用，也不伪装成人类输入。标题和短消息直接可见；长消息按两行预览截断，展开可读/选择完整原文。单向消息的接收行是不可变收件事实，第一期不追加消费状态行，不等待模型执行才显示。

父问子与跨 session 的示例：

```text
◆ ask_subagent → subagent「排查工具展示」 · 等待回答
  哪个环节丢掉了消息正文？

COORDINATION  收到父 agent 提问 · 正在回答
  哪个环节丢掉了消息正文？

◆ ask_session → session a81f… · 已回答
  你的会话正在修改哪些文件？

COORDINATION  收到 session b42c… 的提问 · 已回答
  你的会话正在修改哪些文件？
```

问答接收行继续原位完成，展开显示问题与回答/失败原因。`COORDINATION` 表达系统接收来源；它仍可复用现有问答工具外观的生命周期实现，不能进入前台 tool tracker。发送者不再多一条“已发送”系统通知。

### 2. 状态只说明已有证据

| 事实 | 用户文案 | 不可推断的事情 |
| --- | --- | --- |
| 工具调用已开始，尚无回执 | 发送中 | 目标已接收 |
| `received`，`interrupt=false` | 已接收；下个安全步骤加入上下文 | 已读、已处理、任务完成 |
| `received`，`interrupt=true` | 已接收；请求安全中断 | 当前非中断工具已被杀掉、已完成重采样 |
| 发送 ACK 超时/来源取消，可能已落盘 | 投递状态未知；可能已接收 | 明确未送达；自动重发新消息 |
| inquiry 开始/queued/approval/running | 已收到提问/排队中/等待批准/正在回答，按实际可用事实显示 | 没有阶段事实时猜测对端正在调用模型 |
| inquiry 终态 | 已回答/已拒绝/已取消/不可用/超时/失败 | 用工具 transport 成功代替回答成功 |

源端没有 inquiry 细阶段推送时只显示“等待回答”，不增加轮询。接收 audit 暂未覆盖的阶段也不补造状态。`get_inquiry` 查询成功的标题是“查询完成”，展开中单独展示 inquiry 的终态。

### 3. 使用已经存在的身份，补齐展示方向

Shell 为三个父子工具补全开始标题；Pager 从 typed `raw_input.variant` 与已验证 `grow/tool` 身份选择格式化分支，合并 `raw_output` 的结果。运行时开始、执行、完成和回放均使用相同格式化函数，不能靠字符串标题或收到的消息内容推断工具种类。

子任务名来自 coordinator 的 `request.description`，目标 ID 来自它验证过的路由。来源工具记录持久携带显示快照，目标解析完成后可补充名称；开始阶段只有 ID 时先显示 ID，不为获取名字再次发起 discovery。名称只用于展示，不参与授权、去重或寻址。重名任务在标题追加短 ID，全文细节保留完整 ID；短 ID 碰撞时增加长度。

`InboundInquiry / IncomingInquiryAudit` 增加明确的路由方向 `parent_to_child / child_to_parent / peer`。父收到问题时对端是子任务，子收到问题时对端是父 agent；子任务信息仍留在 details。直接路径由 coordinator 填入，peer 路径由已认证运行时填入。

跨 session 第一阶段继续显示 `session <短 ID>`，详情保留 source/target 完整 ID 和 cwd。当前 `ActiveSession` 没有会话标题字段，不为了装饰标题扩展 peer 发现协议或引入标题缓存。

### 4. 接收事实只写一次，UI 可以重建

```text
send_subagent_message
  → coordinator 验证直接父子关系
  → child ReceiveParentMessage
  → 原始 message blob + NotificationEvent::Received durable commit
      ├→ 返回 received
      ├→ 从该事件派生 UiNotice → child session → NoticeBlock
      └→ 原有 inbox 在安全边界消费 → 模型上下文
```

沿用 `NotificationEvent::Received`、`ParentMessage` source 和 content-addressed artifact，不增加消息账本。最终实现如下：

1. 父消息 artifact 保存原始 message，`notification_blocks` 再根据结构化 source 生成现有模型提示包装，保留 agent guidance 的来源约束。UI 不解析这段英文包装来获取正文。
2. 由已提交的 Received 事实生成 child 的 `UiNotice`。只有原有 durable ACK 成功才显示收到；UI 投影失败不改判投递失败。NoticeBlock 稳定身份为 `parent-message:<receipt ID>`；receipt ID 已由 owner session、父 session、message ID 与 source version 派生。原始投递去重由 Timeline 负责。
3. 所有 Timeline 中仍存在的 ParentMessage receipt 引用都保留 artifact，包括已消费的 receipt。即时 cleanup 与启动 sweep 使用同一保留集合，按 hash 与普通 pending 取并集；避免共享 blob 被另一通知的清理路径删除。会话删除时随会话数据清理，不无限扩展到其他通知类型。
4. load 从已验证 Timeline 的父消息 Received 事实与保留 artifact 派生接收通知，与 live 共用 projector；父消息通知不另写 UI 缓存，不依赖 `updates.jsonl`。完整 load、cursor reload、live/replay 交错按同一个稳定身份合并，已经消费的消息仍恢复收件行，pending 状态和模型消费完全由原 inbox 管理。
5. 历史正文缺失/损坏时显示“收到父消息；正文不可恢复”的有限诊断，保留源身份，不构造新正文；pending 的模型输入继续遵循现有严格 payload 校验，不能为了 UI 可用跳过它。

这是所需的持久化变化。单加 `send_grow_notification` 虽然能让 live 看到消息，却无法覆盖投影丢失与消费后恢复，因此不作为最终方案。

### 5. 复用现有组件，变化限于通信展示

发送工具继续用 `ToolCallBlock::Other`，增加可选的正文预览能力并只在通信格式化分支设置；已有 name/summary/output 分别承担工具、对端与状态、完整详情。接收父消息复用可折叠的 `NoticeBlock`，问答复用已有 `CoordinationRow` 的合并/终态规则。完整详情固定为工具、方向、双方身份、投递模式、Message/Question、Result/Answer/Error 和关联 ID，JSON 留给 raw/detail 调试视图。

长正文按终端 cell 宽度换行，不用字节数裁切中文；换行与列表保留，预览截断处明确省略。工具名、对端和状态优先保留，窄屏允许换行。复用终端字体与 theme 的 system/running/warning/error token，状态始终带文字，正文采用可读的主文本样式，辅助 ID 才降亮度。

消息全文复用既有展开、选择、复制路径；鼠标双击与键盘详情入口都要验收，不增加仅鼠标可访问的控件。包含本地图片路径的消息仍先显示完整文本，不能被通用图片识别替换。通知进所属 child scrollback，不自动跳转视图、弹窗或把正文再复制到父视图；minimal 接收消息可作为不可变收件事实直接追加一次，问答仍等自己的终态后提交到原生历史。

UX skill 的搜索给出了状态反馈与可见性原则；其网页 hero、字体和大间距建议不适用于现有 Ratatui。方案只采用及时反馈、渐进展开、键盘可达和非颜色状态编码。

## Risks / Trade-offs

- 历史正文保留增加存储：单条仍受既有 16 KiB 输入限制，正文只存一份 artifact；保留索引从 Timeline 派生，恢复按需/有界读取，避免 UI 启动一次拼接所有历史正文。
- 接收与消费不同：第一期的接收行始终描述曾经收到，不宣称仍在队列中；模式文案用“下个安全步骤加入上下文”，不写会随时间失效的“等待处理”。
- ACK 不确定：沿用现有错误信息明确显示可能已接收；不设计自动重试按钮。是否新增结构化投递错误码可独立讨论，不是本期展示前提。
- 子视图未打开或在重连：通知要走已有 session owner 路由和恢复机制，验收覆盖关闭后重开、嵌套 child 和重载中到达；不得因当前可见 agent 不匹配而改投父视图。
- 既有 `OtherToolCallBlock.coordination` 有职责耦合，backlog 已记录；本期不整体更换问答行模型。通用工具的兜底标题/结果覆盖治理另行登记。

## Migration Plan

建议按发送展示 → 接收持久投影 → 问答方向与统一验收的依赖顺序实施，三个步骤共同完成后交付。父消息 artifact 表示变化和 inquiry audit 方向字段属于内部持久格式变化，实施前核对现有 session 格式入口并更新版本/校验；不猜测旧正文，不静默混用新旧表示。旧数据保留，遇到不支持的格式明确诊断。

实现与行为验收完成后更新开发者说明，完成完整与增量恢复及 normal/minimal 验证，再归档。格式校验不能代替行为验证。

### 实施前补查

- 新父消息以 `NotificationSourceVersion::Ordinal { value: 2 }` 显式表示原始正文 artifact；ChatState 对父消息校验仅接受此版本，避免把旧模型包装误作新原文。producer identity 仍含 source version，重复同一消息时必须保留原版本与投递模式。
- 父消息同 ID、同正文但更改 interrupt 的请求目前可能复用旧 receipt；实施时父消息 source 元数据也必须一致，否则拒绝，防止 UI 展示与实际投递模式不一致。其他通知的可变化 owner 语义不改。
- 清理所需保留 hash 在 ChatState 的已有 receipt 索引上派生，不能每次复制整个 Timeline 或在 UI 线程读全部正文。
- `eventId` 也是 updates 重连 cursor，不能把任意 receipt ID 冒充有持久 updates 行的 eventId。收件行去重使用 domain receipt identity，与运输 cursor 分开。

### 恢复与终端验证补查

- child 视图已在 `SubagentSpawned` 时建立，不新增另一份 child 容器。既有 child replay 只读取 ACP 更新；本次在相同入口只读已验证 Timeline，逐条重建父消息和 incoming inquiry。先收到 Notice 的子视图仍需完成首次历史读取，缓存读取失败也不阻止独立的收件恢复。
- cursor reload 的 staging tail 原先直接拼接 Notice，导致同一 receipt 重复。合并时按已有 immutable event ID 保留原行及 native commit，不按文本去重。
- cache-free 重建会在 ACP 历史之后发布收件事实。Minimal 对同一已打印父消息按稳定身份继承提交状态，并从 ACP 前缀对齐中分离这类已确认存在的收件记录；其他 durable notice 及模型分支仍保留原有严格前缀规则。由包含交错 ACP/收件记录的回归测试约束，不用简单“已打印行数”跳过历史。
- 实际窄屏渲染发现一行同时容纳长目标和长状态会丢掉工具名。宽度足够保持一行，不足时工具/目标与状态各最多两行，正文预览最多两行，展开不截断原文。
- 核验中保留真实 raw JSON（包括 null），仅不把空 answer/error 生成人类可读的假回答。通信识别只接受 typed variant 或准确工具名，不从标题文字猜测。
