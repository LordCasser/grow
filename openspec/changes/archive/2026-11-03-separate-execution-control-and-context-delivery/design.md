# 执行控制、投递与上下文边界

状态：询问回写增量、联合旅程与独立审查已完成，最终门禁和归档/release 结果见 [收口记录](release-verification.md)。源码与当前契约见 [evidence.md](evidence.md)。本轮设计从 ask 完成后同步这一具体需求出发；不建立通用控制总线或复制 DSH 的全部 API。

## 1. 消息投递路径的分层

本方案只分离消息投递内部的流程控制动作与上下文边界操作：前者决定是否抢占/取消/唤醒，后者决定消息何时以何种身份被消费进入请求。来源校验和持久接纳是共同前提。其他层面的上下文变更由其原机制管理，不进入本消息投递方式。

正式心智模型、消息入口映射与反例见 [消息投递分层](mental-model.md)。下面具体限定 ask 回写闭环；非抢占/no-wake 仅指新的 context-only 来源，不泛化为所有消息入口。

## 2. 明确区分产品组合

下表的 next-step 是“下一个获准采样边界”，不是修改在途请求。

| 操作 | 抢占或取消 | 投递 | 空闲时主动开启 Turn |
| --- | --- | --- | --- |
| ask 完成信息（本次新增） | 无 | 被动 next-step | 否 |
| 用户 steer（现有） | 软抢占 sampler request，续同 Turn | exact TurnId 的 next-step | 现有显式入口不接受 idle/stale 目标 |
| 用户普通后续输入（现有） | 无；配置自动提升后的行为按原 steer 路径 | 先 FIFO，允许时按现有配置提升 | 按既有 FIFO 仲裁 |
| parent message, interrupt=false（现有） | 无 | 既有安全通知消费 | 按现有 admission/notification 规则，不改为被动 |
| parent message, interrupt=true（现有） | 收据提交后请求现有软抢占/可中断等待 | 同一收据的安全消费 | 按既有规则 |
| Stop / Turn cancel（现有） | 终结其精确执行 owner | 不创建消息；产品命令可另有显式队列处理策略 | 不因取消本身请求新工作 |

DSH 的 steer=next-step+wake 不等于 Grow 当前用户 steer。首版保留 Grow 用户体验，把底层投递和抢占的职责表达清楚；被动同步绝不借用 pending_interjections，因为该 buffer 在 sampling select 中就是抢占信号。

不向外部开放任意 `(target,wake,cancel)` 组合，也不为尚无需求的全部组合新增 enum/开关。首版只增加受 runtime 授权的 inquiry 完成来源；其 delivery policy 固定为 context-only。未来生产者需要相同语义时复用同一通知接纳/消费边界。

## 3. 首个闭环：成功咨询写回回答方

1. 维持冻结上下文、无工具 Sideband。提问方继续得到原工具结果，不追加第二份自己的问答。
2. Sideband 成功结果与回答方 Incoming terminal audit 按原 durable 规则记录。新成功事实的结构化来源证明必须绑定原始 `target_session_id`（回答方）及本次实际 Sideband；冻结源坐标从其 manifest 取，不从 UI 文案或当前 Timeline 尾部猜测。同步资格来自这份新契约的证明，不增加布尔开关，也不把旧 Answered observation 默认解释成待同步任务。
3. 由回答方 runtime 把成功 audit 转成 Notification 的 context-only 来源。建议最小来源形状为 `InquiryAnswered{source_session_id,inquiry_id}`，owner 必须等于成功证明中的原始 target，且 Sideband 的实际 owner 必须一致；source version 绑定其成功 terminal event identity。提问者/回答者不得互换，不从当前 actor 或缺失的收据反推回答方。
4. 复用不可变 payload 与 Received，内容包含准确参与方、问题、完整答案、Sideband/快照来源。runtime 事实与问答正文分开，正文作为引用数据，不提升为系统约束。
5. 复用 Consumed：通知 identities 与精确合成输入同事件 durable 发布，然后才能构造含它的下一次 provider 请求。可使用现有 notification input 表示，不制造孤立 ToolResult、假的模型工具调用或新的 ConversationItem 家族。

建议模型表述：某子任务/会话向本会话询问了 Q；本会话的 Sideband 基于快照 S 回答 A；这是已发生咨询的记录，前台尚未必审阅或采纳，也不是人类授权。保留完整条件、限定和代码原文，不能只提取“选 A”。仅引用实际公开的最终答案，不复制隐藏推理、系统提示或整份冻结上下文。

成功信息覆盖 `ask_parent`、`ask_subagent`、`ask_session` 的回答方。外部 coordination API 仍不向来源 Session 合成工具调用/结果；已实际完成咨询的目标仍按相同规则同步。目标 canonical 非成功终态（失败、拒绝、超时、取消）只保留原审计，不注入成功问答；来源停止等待或接纳本地取消 intent 本身不证明目标取消已获胜。不将权限分类、标题、OCR、compaction 等所有 Sideband 自动写回。

## 4. 边界、唤醒与终止竞争

- **活跃 Step：** 收据接纳不取消在途 stream/tool。下一个获准采样边界消费，不拆开结构化工具调用/结果批次。
- **最终回复竞争：** 区分最终回复前的候选消息快照与不可逆的 Turn 最终关闭。在同一 actor 控制边界内捕获精确 candidate IDs，只有这些候选可作为续同 Turn 的理由。原 owner、预算与 Behavior 仍允许后继执行且候选成功消费时，Turn 并未最终关闭；后到消息可在该后继 Step 的正常安全边界消费。若最终关闭胜出或后继执行不获准，未消费消息保留到下一真实 Turn。不能向已结束 Turn 追加输入。`no wake` 禁止新建空闲 Turn，并不保证活跃 Turn 永远不增加后续 Step。
- **排序依据：** 快照按 Received 实际接纳顺序固定 IDs，不能释放栅栏后再读全部 pending 冒充旧快照，也不按 Sideband 开始时间回插。快照后的消息不能成为此次最终关闭决策的续跑理由；无需新增 durable min_turn 路由。
- **空闲：** 单独存在此来源只留下 pending，不开启 Turn，不改变 Goal/Plan 状态。新用户 Turn、原本允许的 Goal/Workflow/通知 Turn 的首次采样前可以携带它；不得绕过 FIFO edit hold 或别的 owner 准入。
- **取消收敛：** 已关闭目标不能接纳新的显式 steer；已接纳输入仍按其既有 reroute 规则。被动通知没有旧 Turn 预留，保持 Session 级 pending，到后来获准的 Turn 再消费，不自行保留/制造 wake latch。
- **挂起/关闭：** 停止新接纳并按当前活动 owner 栅栏收束。已持久化未消费信息留在原 Session，恢复不主动执行。关闭与永久删除仍按现有事实及关联约束处理；不能因为恢复待办而复活已删除目标。
- **后续 Sideband：** 仍只读其实际冻结 Surface；尚 pending、尚未物化的回写不承诺已出现在另一次咨询快照中，不另造一套 Sideband 专属记忆。

## 5. 成功终态与收据之间的故障

不假设两个单事件写入是事务，也不在两个 Session 之间建立事务。

属于当前 Session 且具有上述新结构化来源证明的成功 terminal audit 本身构成回写义务。Received 使用由该事实派生的稳定身份；同身份同 payload 复用，冲突拒绝。若成功 terminal 已提交而 Received 尚未提交就崩溃，replacement writer 在公布/准入该 Session 前，以 canonical 成功事实补齐缺失收据；live 已有 outcome 重试也沿相同核对，不重新运行 Sideband。重建须校验原始 target 与当前 writer Session 相同、Sideband owner/成功结果对应；fork 继承的他属 observation 不具备本地同步资格。正常后续采样前亦须协调未完成的回写义务，不能只依赖进程启动修复。

不增加 outbox 文件、独立 applied 标志或第二份问答终态。正文 artifact 没有 Timeline 收据时按现有 orphan 规则处理；需要补齐时由成功 audit/Sideband 事实重新得到准确正文。回写失败不能改写已经持久化的真实 Answered 为 Failed。Answered 表示咨询成功；它不声称提问方已收到工具结果或回答方前台已读取。

补齐只由持有合法 writer epoch 的 runtime 执行。普通列表/full/light observation、trajectory 检查和 UI replay 都是只读路径，不能为了“修复同步”取得 writer 或追加收据。

消费 ACK 失败时不能派发声称已含该输入的请求；ACK 不明沿现有 writer/Timeline 核对，不自动重跑 provider。Consumed 已提交但请求尚未发出的恢复只重建已有 Surface，不重复消费，也不能声称模型已经读取；后续请求继续按正常压缩/投影规则构造。UI 回放只投影成功终态和正文，不触发投递、消费或第二次 ask。

## 6. 权限、分支与历史

- 不调用 HumanIntent admission，不生成 PermissionEvidence，不把 Sideband 的“允许/批准”识别成人类许可。
- 不借用 ParentMessage/AgentReply 来源，避免上行 interrupt 或实际发送者归属错误；仅新增咨询完成 producer。
- fork 可按既有规则继承已消费的文本历史和可验证来源，不能把原 Session 的 pending 收据、操作权或消费身份转成新 Session 的待执行任务。
- compaction 保留既有 canonical 引用和权限边界；历史已归档不代表永不被摘要。需要精确引用时回到原 audit/Sideband，而非用摘要重建授权。
- 只对新成功事实应用新契约；不从历史 UI 的 Answered 文案批量回填。若持久类型需要 schema 变更，遵循本仓库严格版本规则，不编造迁移或兼容 fallback。

## 7. 分步实施与评审

首个增量是“ask 成功→被动收据→真实下一请求/恢复”端到端能力，同时完成所需投递/抢占边界整理；不先搭完抽象框架。生产实现只允许回改 inquiry、notification、Timeline 来源/消费验证和必要请求投影；其他控制动作本轮仅作语义对照，不改行为。任务与对抗性验收见 [tasks.md](tasks.md)。

不新增 UI 常驻状态或技术标签。现有 Answered 继续只表示答案生成；若未来 UI 要显示上下文同步，必须从真实 Consumed 事实派生，并且“已纳入请求”不等于模型认可。本提案不以 UI 出现问答作为模型已知的验证证据。


## Inquiry 取消与成功接纳

复用 InquiryCancellation 原子状态，在现有成功 terminal 栅栏内、完成全部准备和 canonical 查重后、实际 append 前决定唯一赢家。取消先赢则写对应非成功 terminal，无成功证明；success admission 先赢则后到取消不能反转该候选，但 durable ACK 之前仍不能派生上下文。持久化失败如实返回 AuditFailure，不伪装为成功或取消。目标关闭的 token 独立管理 Sideband 生命周期；已持有 Queue activity 的 Completed End 仍允许在合法 drain 内结算。

父子 Ask 的 host-response 等待尊重此赢家。Peer 不共享目标原子状态：来源只接纳 cancellation intent，首个匹配 Progress 确认目标注册后，向 pinned 目标请求一次取消并继续原 Ask 读取；实际 typed outcome 仍来自目标流或真实传输/期限错误。局部 cancel_session 的 bool 不是远端取消证明。保持已读取一部分的同一个 read_json frame future，不丢帧；断线重连保留原 inquiry identity 与可能已经派发的事实，不创建新问题或新 ledger。

## R11: Peer wire eligibility
The existing 1 MiB notification limit measures rendered UTF-8, while local IPC measures actual JSON payload bytes. Peer success must satisfy both before its success seal/proof. Extract the wire writer's pure encoder and invoke it with the real Response::Inquiry; use the same check for cold Peer proof acceptance. This is an eligibility check at the existing owner, not a new cap or a transport retry. Completed Sideband evidence/usage is retained on typed failure; direct delegation keeps its own limit.
