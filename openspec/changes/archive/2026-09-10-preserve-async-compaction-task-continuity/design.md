## Context

事故证据及范围见 [investigation.md](investigation.md)。压缩事务完整，下一请求也完整返回。能证明的是输入语义在压缩点发生变化，不能证明模型内部为何选择 `stop`，更不能将合法 stop 猜成代理丢帧。

当前数据流为 `Timeline → Surface → request projection → sampler → durable response → turn outcome`。后台 `PreparedCompaction` 冻结片段，前台在 `StepEnded` 后验证 authority/model/range 并发布。`replace_compaction_range` 保留未选中 SurfaceId，但所有 Surface replacement 都重置内存 continuation epoch；portable projector 会将成对工具往返变成不可信历史文本。Sampler 只验证协议完整性，Shell 在无工具且无特殊 stop reason 时返回 Completed。

主规范约束范围 identity、边界发布和迟到取消，未覆盖历史摘要与当前任务的语义衔接。本次新增契约，不将缺失场景冒充已存在规范。

## Goals / Non-Goals

目标是在压缩后的既有下一 Step 中明确当前任务的时间顺序，补齐可审计、可恢复的续接事实。非目标是推断自然语言是否属于“承诺未兑现”、保障模型必然完成任务、改造三个后端的 portable 工具协议，或用重试隐藏合法终止。

## Decisions

1. **历史范围说明写在摘要载体中。** 复用 `CompactionMeta` replacement，在原始摘要格式之后追加说明，再追加 Recall/实时资源提示。保留原始 Sideband 摘要及摘要前缀的证据校验，范围说明不改写生成结果。
2. **续接由下一 Step 的 owner 持有。** `background_compaction_boundary()` 的 committed 返回值只在 `process_conversation_turn` 的 between-step 路径使用。等 stationarity、控制/预算和 `can_continue_regular_turn` 检查通过后，在现有 `step_control_gate` 与 admission 锁内持久化 `ConversationItem::auto_continue`，再发 StepStarted。已完成结果外层也会发布压缩，但该路径没有下一 Step，不追加提示。
3. **一次提交，一次提示。** 使用当前边界的局部 committed 值，并以现有 Step index 排除新 turn 的首 Step（新输入自身承担任务接纳），不新增 session 状态、计数器、消息队列或二次恢复循环。请求重建只读取已提交提示。纯 prune、手动 compact、后台失败和未就绪不产生 committed 值，因此不受此续接路径影响。
4. **不恢复旧 native。** 摘要改变了 provider 前缀，保留旧签名/加密 reasoning 无合法性保证。结构化 portable tail 可在独立 change 设计三协议投影及完整性校验；不能仅调小 `portable_prefix_len` 绕过现有安全策略。
5. **保留停止权限。** 提示要求根据更新用户指令及结果继续未完成任务，也允许任务已完成或确需用户输入时正常结束。它没有真实用户 PermissionEvidence，不重启 Goal，不增加调用次数或改写 refusal/stop/预算语义。

## Risks / Trade-offs

- 这是对已证实的输入衔接缺口的修正，不是对概率模型行为的必然性保证。回归验证请求、持久化和生命周期，不能用 scripted provider 的成功输出宣称线上模型已被治愈。
- 在 gate 内等待一次既有 ChatState durable append，沿用其他准入事实的同步边界；ACK 失败停止下一次 provider admission。
- 仅在 between-step 成功发布异步结果时追加尾部提示。同步/promoted compaction 的续接策略、跨 route 的 portable 结构化工具历史另行评估；摘要范围说明对所有摘要适用。
- 工作区存在并行采样恢复实现，改动保持在两个既有代码插入点，不改它的 attempt preview/usage/recovery budget。

## Validation

使用真实 SessionActor、MockInferenceServer 及可释放的后台响应屏障，构造“旧任务已完成 → 新任务已执行工具 → 异步发布 → 下一请求”。核对模型可见提示顺序、同 turn Step 和唯一 terminal、无重复 reminder/模型调用；覆盖后台失败、仅完成边界发布和取消。沿既有持久化故障注入验证续接 ACK 失败不请求 provider。OpenSpec 全量及 archive 校验在完成实现后执行。
