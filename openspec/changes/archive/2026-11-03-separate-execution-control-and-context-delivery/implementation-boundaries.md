# 首个实现增量的实际边界

状态：实现前源码核对与交接，不是已完成实现。限定消息投递入口内的流程控制/上下文消费；不改造其他上下文操作。下列依据来自当前工作区，未运行 Rust 或 provider 集成测试。

## 1. 一个完整特性

实现“成功 ask → 回答方 canonical 成功证明 → 被动 Received → 下一获准边界的 Consumed/真实请求 → 恢复去重”。同时支持 parent/child/peer 共用的目标处理路径。不要先发布只有 enum、只有 UI 或只有内存 reminder 的半闭环。

提问方继续使用原工具结果。新增来源绝不进入 `pending_interjections` 或设置 `parent_message_interrupt`：`turn/sampling.rs:2152` 当前把这两个信号作为取消 sampler request 的触发条件。

## 2. 复用真正的 Sideband 结果引用

`actor/sideband.rs::SidebandRun::complete` 已返回 `Result<TimelineRangeRef, SidebandRunError>`：先 durable Result，再 durable Completed End，成功返回该 Result 的精确范围。`actor/coordination.rs::handle_coordination_inquiry` 现在把这个返回值丢弃。

直接保留该返回值，给目标侧内部处理结果携带最小结构化来源证明，例如：

```text
InquiryAnswerContextProof {
    target_session_id: 原始回答 Session,
    result_ref: complete() 返回的精确 TimelineRangeRef
}
```

成功 Incoming audit 保留此证明；收到/审批/失败等事实没有它。证明本身是新契约的资格，不加独立布尔开关。历史 Answered 缺少证明时只作历史观察。无需给 SidebandRun 增加 id getter，或从最后一条 Timeline 事件猜 Sideband。

冷加载/恢复核对可复用 `storage/mod.rs::validate_sideband_ledgers` 及 `completed_sideband_result`：验证本地 target、parent spawn、InfoRequest purpose、冻结源及 Completed Result，且原 answer 与 Result 的公开最终文本一致。继承的他属历史不能形成当前 writer 的同步义务。只读取最终答案，不复制 hidden reasoning、整个 request 或凭据。

正常成功路径的证明直接来自刚确认的 `complete()` 返回值；不为每次 Step 再打开或全量解析 Sideband 文件。writer 冷恢复复用已有 loader 已读取/验证的 ledger 与来源结果，补上咨询证明的关联校验，不另写一套 Sideband I/O 引擎。suffix 协调只处理新增成功证明，已核对的旧范围由非权威游标跳过。

source 建议使用已定的 `InquiryAnswered{source_session_id,inquiry_id}`，owner 为证明中的原 target，source version 绑定成功 terminal event identity；使用 `record_timeline_event_durably` 的真实返回事件，不制造 eventId。可以保留当前 `persist_coordination_inquiry -> Result<()>` 外层入口，在私有共用写入逻辑中返回事件，避免不必要地改变 mailbox 的 public response。

## 3. 补齐 terminal→Received：suffix 查询与一个非权威游标

采用现有 ChatState `GetTimelineEvents` 的增量查询，避免每次 Step 克隆全部 Timeline，也不引入 pending map 或第二个账本：

- `timeline_events_from(start)` 返回单次 actor 查询捕获的不可变 suffix；既有 `timeline_events()` 继续相当于 start=0。
- SessionActor 仅持有一个进程内、非权威的已协调 `next_seq` 游标。writer 启动时从 0 扫描一次，按 canonical 新成功证明重建缺失收据，然后才开放执行。
- 真实采样/最终候选边界先捕获 suffix 的固定结束位置 F。处理游标到 F 之间的成功证明：校验来源，使用现有 `received_notification_id(source,version)` 精确查询，包括已消费收据；只为缺失项调用既有 immutable payload/Received 接纳。
- **仅当 F 之前全部义务完成核对或补齐，才把游标推进到 F。** 不能推进到协调过程中重新查询到的 Timeline 当前末尾；那会跳过并发刚提交的成功终态。
- 任一协调失败返回错误，游标不越过该未完成义务，不派发依赖该边界的 provider 请求。重试仍是原 identity，Received 的 artifact gate 和现有幂等折叠负责串行/冲突检查。
- 处理过程中新增的 Received 及其他事件留在下一段 suffix；没有新的成功证明时只是跳过这些事实。F 之后的成功证明属于下一个边界，不能回插到已冻结的请求。

这样 canonical ACK 后、Shell 内存注册前不存在丢失窗口：下一边界始终读 canonical suffix，而不是依赖某个任务及时更新内存待办。崩溃丢失游标只导致合法 writer 重建，不导致重复消费。相同 inquiry 的既有 outcome 重试也可核对该精确证明，无须重跑 Sideband。

成功 terminal 已提交后的补齐失败不得走现有“把 outcome 改为 Failed”分支。Answered 仍表示真实咨询成功；后续上下文同步错误由采样边界显式报告。只有原成功 audit 本身没有确认时才沿现有审计提交失败语义处理。

## 4. 不能继续沿用吞错的 bool 边界

`notification_drain.rs::drain_active_notifications(_excluding)` 现在遇到 payload 读取或 Consumed 提交错误会记录日志并返回 false；`turn/admission.rs` 两处和 `turn/mod.rs` 主循环忽略该 bool。这不足以满足新契约。

应让必要的消费/协调路径返回 `Result<bool, ...>`，并在真实请求路径传播；false 只表示没有可消费项，不再同时表示失败。`interjection.rs::close_steering_and_drain` 和最终回复调用方也必须区分这两者。已有错误分类可复用 `fatal_turn_boundary_error`，避免 Agent completion-requirement recovery 把存储错误当作可重新采样的普通回答失败。

不能仅在 helper 测试里检查错误。需要证明首个 Turn 请求、普通后继 Step 和最终候选续接三条实际路径都不会越过失败。对共享通知错误语义的实际变化在 delta/验证中明确记录，不顺带改写通知调度器。

## 5. 暂定候选快照不等于最终关闭

当前 `close_steering_and_drain` 会先关闭 `steering_open`，释放 gate 后再查 parent/reply 通知；收到消息后可能重新开启同一 Turn。因此不能把这个暂定快照理解成所有后来消息永远只能进入下一个 Turn。

- 在已有 actor/控制边界上捕获固定候选集合；它只决定是否有信息可以请求同 Turn 的获准后继 Step。
- 先遵守现有预算、取消、Behavior 和 owner 检查。若后继 Step 不获准，不为“同步完成”而先消费候选，保持其 pending。
- 如果候选促成合法的同 Turn 后继，后来接纳的消息仍可进入该后继或以后 Step 的正常安全边界；不能回插已关闭的旧请求。
- 如果没有获准后继，最终关闭才胜出；之后的收据保持 pending，等待下一次原本允许的 Turn，不主动唤醒。
- 不增加每条 passive 消息的 `min_turn`、新路由或 wake latch。

`notification_autostarts` 当前仅排除 TaskStillRunning。新来源必须明确排除；相应优先级、正文投影和 exhaustive source matches 需机械补齐。不能直接把新来源伪装成 ParentMessage/AgentReply。

## 6. 实际修改面与实现顺序

根据当前源码，最小完整能力预计触及约 **14 个既有生产文件**，而非六个以内：

| 责任 | 当前必要文件 |
| --- | --- |
| 成功证明与 inquiry 完成/既有 outcome | `shell/src/coordination/inquiry.rs`、`shell/src/session/actor/coordination.rs` |
| 被动来源、消费、错误、final 候选 | `shell/src/session/actor/notification_drain.rs`、`interjection.rs`、`turn/mod.rs`、`turn/admission.rs` |
| 非权威游标及 writer 初始化 | `shell/src/session/actor/mod.rs`、`spawn.rs` |
| 来源折叠/验证与 suffix 查询 | `chat-state/src/timeline.rs`、`handle.rs`、`commands.rs`、`actor/mod.rs` |
| 必要 exhaustive 展示与冷加载证明 | `chat-state/src/trajectory.rs`、`shell/src/session/storage/mod.rs` |

路径以 `crates/codegen/` 为根；表中短文件名沿同一前缀。最终编译可能揭示其他机械 match/测试调用方，允许同步，不因此扩张产品行为。Sideband `complete()` 已足够，预计不需要修改其生产实现；sampler 抢占行为保持原样。

上述文件是同一个持久化/跨模块特性的必要接缝，可以超出任务体量参考，但不应拆成互不运行的三层工程。先固定证明/查询/来源契约，再贯通真实 actor 请求和恢复，最后补机械调用方、独立对抗测试与文档。任务书给同一集成 owner；不要平行派两个 coder 修改相同通知/Timeline 所有者。

## 7. demo 与运行时验收必须区分

抽象状态机必须在“成功终态已进入本次冻结 canonical frontier、Received 尚缺失”时禁止 dispatch，直到协调和消费确认；不能只检查已存在 Received 的集合。暂定候选与最终关闭也须分别建模。独立审查曾给出此前缺失 guard 的可达三步反例：terminal_valid → ordinary admission → dispatch，最后 Received/Consumed 均为零而旧 invariant 仍通过。该反例已修正：冻结 frontier 同时覆盖成功终态义务与 Received，新增 live reconciliation 场景及 bypass-terminal-obligation 行为变异。

独立审查者阅读最终 transition/invariant，并实际运行 `PYTHONDONTWRITEBYTECODE=1 python3 openspec/changes/separate-execution-control-and-context-delivery/demo/model.py`：退出 0，144 初始配置、24,658 状态、98,953 转移、40 类动作、16 个命名场景与 10 个行为变异反例。最终 dispatch 同时要求冻结义务已完成消费、Consumed ACK 已确认；候选快照与真正关闭分开，新增 budget/Behavior 失效和工具结算后的 cancel 路径。行为变异在同一可达 transition relation 中执行，不靠直接构造非法终态。此模型可作为进入实现的有限设计证据。

即使 demo 通过，仍需真实 actor/provider gate 测试确认不取消 stream/tool、三条采样路径阻止存储失败、游标不跳过并发成功事实、ACK 不明、已有 outcome 去重、writer 恢复和来源/权限边界。模型不证明这些 Rust 集成行为。
