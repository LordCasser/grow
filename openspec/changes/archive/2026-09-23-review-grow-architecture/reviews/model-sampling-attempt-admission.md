# Model sampling / attempt admission 架构审查

- 基线：`bd1f5085`（`v2.1.9`）
- 日期：2026-09-16
- 范围：provider attempt 身份、统一恢复额度、取消、证据和用量结算、候选 durable admission、工具执行 authority、provider/host terminal、portable replay、leader/Pager 投影和冷恢复
- 契约：`openspec/specs/model-sampling/spec.md`、`session-timeline/spec.md`、`client-surfaces/spec.md`

## 1. 当前职责与依赖方向

### 1.1 所有者

- **Shell `SessionActor`** 持有一个未接纳模型步骤的编排权：构造请求、创建共享 `RecoveryBudget`、申请 Goal/任务用量范围、提交 sampler、持久化接纳响应、决定工具是否可执行，并发布 Turn terminal。
- **sampler** 持有实际 provider attempt：wire admission、流协议归一、attempt number、错误分类、取消、证据闭合、每 attempt 用量结算和 provisional candidate。它不修改会话历史。
- **ChatState Timeline/Surface** 持有 canonical 会话事实：响应 durable admission、用量 settlement 去重、native continuation 安装、工具隔离修复和冷恢复状态。
- **session persistence** 持有 `updates.jsonl` 展示回放缓存。该缓存不是模型状态权威；`timeline.jsonl` 才是 canonical Surface 来源。
- **leader/Pager** 持有 transient attempt 投影：支持 lifecycle 的客户端可看到可撤销预览；不支持的客户端在 Accepted 前缓冲；Pager 在未确认预览存在时放弃 cursor-only reconnect。

### 1.2 正常路径

```text
Session 构造 ConversationRequest + 共享 RecoveryBudget
  -> sampler admit attempt
  -> provider stream / provisional attempt-tagged output
  -> attempt evidence finish
  -> Timeline + Goal/任务 usage settlement ACK
  -> sampler 返回 provisional candidate
  -> ChatState push_response_durably
  -> Shell 发布 SamplingAttempt::Accepted
  -> updates.jsonl 写入已接纳展示投影
  -> Shell 才允许本地工具执行或结束 Turn
```

必须保持的不变量：

1. 每个真实 provider attempt 有唯一身份，并在下一次 provider admission 前闭合证据和所有适用账本。
2. 取消、期限和 shutdown 可以停止 provider 工作，但不能把独立 settlement 当作可直接 abort 的附属 future。
3. response 只有在 ChatState durable admission 确认后才能发布 Accepted、安装可用 continuation 或执行工具。
4. admission 确认不明时不能盲目重新采样；必须按原提交身份核对。
5. 历史 provider-neutral 事实不能被当成当前 attempt 可能发生副作用的证据；当前不明 provider side effect 又必须关闭 replay。
6. `updates.jsonl` 可以是可重建缓存，但已接纳 Timeline 响应在冷恢复后必须仍能投影给用户。

## 2. Actionable findings

### HIGH-1：正常 sampler shutdown 会 abort 尚未闭合的 attempt settlement

**处理状态（2026-09-17）**

当前工作树已通过归档 change `2026-09-17-settle-sampler-attempts-on-shutdown` 修复正常关闭路径：actor 取消 provider token 后协作 `join_next()`，不再调用 aborting `JoinSet::shutdown()`；阻塞 response evidence ACK 与 usage ACK 的红绿回归、owner forced-timeout 测试及 Shell drainer 顺序测试均通过。`model-sampling` 主规范已补入正常关闭和 forced-timeout 失败语义。进程被强杀时未确认 attempt 的冷恢复证据仍受本报告“验证缺口”限制，不由本次 graceful-shutdown 修复冒充已证明。

**触发条件**

Session teardown 在一个已准入 attempt 正在 provider polling、`AttemptEvidence::finish`、`AttemptUsageSink` 或 retry evidence ACK 中发生。

**代码证据**

- `crates/codegen/sampler/src/actor/mod.rs::SamplerActor::run` 收到 `Shutdown` 后取消 active token，随后立即调用 `JoinSet::shutdown()`。
- Tokio `JoinSet::shutdown()` abort 剩余任务并等待取消完成，不是 cooperative join。
- `crates/codegen/sampler/src/actor/request_task.rs::run_request_task` 的 evidence finish 和 known/incomplete usage sink 都位于被 abort 的 request task 内，并包含独立 `.await`。
- `crates/codegen/shell/src/session/actor/teardown.rs::shutdown_sampler` 把该路径作为 production graceful shutdown；外层十秒 timeout 无法改变 actor 内部立即 abort 的事实。

**违反的不变量**

- `model-sampling` 的每 attempt 结算、取消期间 settlement 独立完成、deadline 不硬中断 settlement。
- `session-timeline` 的进程在 attempt closure 前停止时应保留未确认状态和证据。

**影响**

provider 已实际开始后，已知消费、未知消费标记或原始 attempt evidence 可能没有进入 durable ledger；shutdown 仍可能返回成功。该问题直接可由普通 session unload/shutdown 触发，不要求公共 API 误用。

**修正任务**

1. 关闭新提交 admission。
2. 取消每个 provider token，使 provider poll/backoff cooperative 结束。
3. 在 actor 中逐项 join request task，让任务进入 evidence/usage closure；不要在 graceful 分支调用 aborting shutdown。
4. 只有外层显式 forced timeout 才 abort；forced abort 前必须已有可恢复的 pending-attempt/settlement 身份，不能把缺失事实当作结算完成。
5. Shell teardown 只有在 request tasks 和 sampler event drainer 都闭合后才能声明该阶段成功。

**回归验证**

- evidence sink ACK 阻塞时发起 shutdown，确认 shutdown 等待 ACK 且 evidence 恰好一次。
- Timeline usage 已提交、Goal ACK 阻塞时 shutdown，确认同 attempt 补交而不重推理。
- forced timeout 返回明确失败并留下可核对状态，不发布假 durable frontier。

---

### HIGH-2：response admission 确认不明被压成普通 turn error，可进入 completion recovery

**触发条件**

ChatState 已持久化 assistant response，但 `PushResponseDurably` 的 oneshot reply 在调用方收到前丢失；当前 Agent 配置 completion requirement recovery。

**代码证据**

- `crates/codegen/chat-state/src/handle.rs::push_response_durably` 把 query reply 丢失表示为 `TimelineWriteError::AcknowledgementLost`。
- `crates/codegen/chat-state/src/actor/mutations.rs::push_response_durably` 先提交 Timeline response，再由 actor command handler 发送 reply。
- `crates/codegen/shell/src/session/actor/turn/mod.rs` 将所有 `TimelineWriteError` 格式化为无类别的 `acp::Error::internal_error`。
- 同文件 completion recovery 只停止带 `turn_boundary_persistence_failed` marker 的错误；普通错误会发布 `Retrying`、追加 `AutoRecovery` 并启动下一模型 Step。

**违反的不变量**

`session-timeline` 明确要求：response 可能已写入但确认丢失时，停止新 sampling，以原提交身份核对；不得视为 generation failure、重复提交或执行工具。

**影响**

若该窗口发生，durable Surface 可能已有第一份 response，而 Shell 将其解释为失败并尝试新的模型步骤，造成重复语义工作、额外 provider 消费和不一致 host terminal。第一份 response 的本地工具不会在未收到 ACK 时立即执行，这是现有边界的有效保护，但不能消除盲目 resampling。

**可达性限制**

当前 ChatState actor 串行处理命令，持久化成功到 reply 发送之间没有正常异步等待，因此该窗口比普通网络 ACK-loss 窄，主要依赖 actor/task 在 commit 后、reply 前终止或 panic。当前没有故障注入证明该窗口的实际频率；这会限制频率判断，但不消除契约缺少原提交 identity 和 reconciliation 的事实。因此定级为 High，而不是以未证明的频率扩大为 Critical。

**修正任务**

1. 为 response admission 建立与 request/attempt 关联的不可变 identity 和 payload digest。
2. ChatState 提供幂等核对：不存在、同 identity 同 payload 已提交、identity 冲突、当前不可用。
3. Shell 保留 typed uncertain-admission 错误；在核对完成前禁止 sampler retry、completion recovery、Stop-hook continue 和工具 dispatch。
4. 核对为已提交时，从原 admitted response 继续一次而不是重新 append；关闭中的 unresolved 状态以 Recovery/Host interruption 结束，不冒充 provider failure。

**回归验证**

- commit 成功后丢 reply：Timeline 恰好一份 response、无第二 provider request、无 `Retrying`、无工具执行。
- completion requirement 开启时仍不追加 `AutoRecovery`。
- admission identity 冲突 fail closed。
- cancellation 与 ACK 竞争后冷恢复按原 identity 对账。

---

### HIGH-3：Timeline 已接纳 response 与 `updates.jsonl` 已接纳展示之间存在不可恢复裂缝

**触发条件**

以下任一情况发生：

1. `push_response_durably` 成功后、`SamplingAttempt::Accepted` 到达 persistence actor 前进程停止；
2. Accepted 已到达，但 `flush_sampling_candidate` 写 `updates.jsonl` 失败。

**代码证据**

- `turn/mod.rs` 先 await `push_response_durably`，随后才调用 `finish_sampling_preview(true)`。
- `updates.rs` 把 Accepted 作为 transient 事件经 event pipeline 异步送往独立 persistence actor。
- `persistence.rs` 只在 Accepted 分支把内存中的 candidate chunk 写入 `updates.jsonl`；channel close 时未接纳 candidate 被有意丢弃。
- candidate 写入失败只记录 `failed to write accepted sampling preview`，没有 retry 或与 Timeline reconciliation。
- session load 的普通 UI replay 从 `updates.jsonl` streaming；审查未找到从 canonical Timeline 重建缺失 assistant scrollback 的生产路径。

**违反的不变量**

严格的“先 durable admission、后 Accepted”顺序本身正确；缺陷在于 `updates.jsonl` 作为非权威缓存不可由 admission fact 重建。`client-surfaces` 的 reconnect 场景要求只展示已接纳成功历史，而不是在 Timeline 已接纳时永久缺少该历史。

**影响**

冷恢复后模型 Surface 包含 assistant response、native continuation 或后续工具交换，但用户 scrollback 可能没有产生这些状态的 response。工具结果或后续回复会失去可见因果前件，形成 model/user split-brain。

**修正任务**

1. 复用 response admission identity 关联 Timeline response 与展示 projection。
2. load/reconciliation 以 Timeline 为权威：匹配 cache 投影缺失时重建一次；存在时去重；冲突时拒绝把错误 candidate 标为 accepted。
3. `SamplingAttempt` lifecycle 继续是可重建投影，不把 `updates.jsonl` 提升为第二状态权威。
4. accepted cache 写失败进入有界 retry/reconciliation，而不是只告警。

**回归验证**

- kill point：Timeline ACK 后、Accepted 消费前停止；冷 load 的 Surface 和 UI 都恰好一份 response。
- Accepted 后 cache flush ACK 丢失或写失败，恢复结果不为零也不重复。
- tool-bearing response 的 assistant call presentation 在 tool result 前恢复，replay 不重新执行工具。
- earlier discarded attempt 不能被绑定到 later admitted response。

---

### MEDIUM-1：重复 live `RequestId` 会破坏 active ownership 并碰撞 attempt identity

**触发条件**

两个仍存活的 sampler submission 使用同一个 caller-supplied `RequestId`。

**代码证据**

- `RequestId` 接受任意 caller string；公共 `SamplerHandle` submission API 不要求 UUID。
- actor `register` 覆盖旧 map entry 并取消旧 token，但两个 task 都留在 `JoinSet`。
- task 正常返回时只返回 `RequestId`；旧 task 退出可按相同 ID 删除新 task 的 active entry。
- cancel/is_active/active_count/shutdown 之后无法可靠定位新 task。
- usage/evidence key 是 `<request_id>:<attempt_number>`；两个 execution 都从 attempt 1 开始时发生 identity collision。

**影响与定级**

触发后会产生取消所有权丢失、错误 active 状态、live task 漏管和 settlement duplicate/conflict。当前 Shell 主 turn 使用随机 UUID，普通产品路径触发概率很低；问题主要存在于公共 sampler API 和未来调用方，因此整体定级 Medium，触发后的完整性影响仍高。

**修正任务**

优先在 actor admission 原子拒绝 duplicate live caller ID，并通过 completion/event 返回 typed lifecycle error；如果确需替换语义，则必须引入 actor-owned execution generation，cleanup 使用 compare-and-remove，attempt settlement identity 包含 execution generation。

**回归验证**

提交同 ID 的两个阻塞请求，证明第二个被拒绝或两者使用不同 execution identity；旧 task 结束不能删除新 owner，settlement 不被误判 exact duplicate。

---

### MEDIUM-2：历史 `BackendToolCall` display fact 会永久关闭后续无关步骤 replay

**触发条件**

请求历史含任意旧 `ConversationItem::BackendToolCall`，后续无关模型步骤首次 attempt 发生本应可恢复的 EOF、idle 或 transport failure。

**代码证据**

- `request_task.rs` 在第一次 attempt 前扫描整个 `request.items`；发现任意 `BackendToolCall` 就 `recovery.prohibit_replay()`。
- `sampling-types/conversation.rs` 明确定义 `BackendToolCall` 是 provider-neutral display fact，native ID/status/output 只存在 continuation lane。
- portable history 将该事实转换为普通 assistant summary text，不会把历史调用变成新执行请求。
- 当前 Responses stream 已有 `ReplayUnsafe` 事件，用于当前 attempt 观察到 hosted/server tool activity 时关闭 shared recovery。

**违反的不变量**

`model-sampling` 只在当前请求可能产生无法证明安全的 provider side effect 时关闭 replay；portable 历史不得把已完成历史调用解释为新的执行请求。

**影响与定级**

包含过往 server-side tool display 的会话在之后所有相关请求中失去正常 transient recovery，形成持续可用性退化；它不会直接导致不安全重放，因此定级 Medium。

**修正任务**

不能只删除现有检查。replay authority 应来自当前 request/attempt 的 side-effect capability 和 evidence：

- portable historical display fact 不关闭 replay；
- 当前 native/request capability 若可能执行 hosted side effect，应在 dispatch/observed evidence 的正确边界关闭；
- 已观察 `ReplayUnsafe` 保持 sticky；
- transport 在 side-effect evidence 到达客户端前中断时，也必须有保守且可证明的策略。

**回归验证**

- seeded historical `BackendToolCall` + replay-safe transient first failure：第二 attempt 发生。
- 当前 hosted operation：replay 关闭。
- provider 可能执行 hosted tool、但 stream 在 tool evidence 前中断：按明确的安全策略停止或以可证明 identity 核对。

## 3. 被反证的候选问题

### leader full-load 4096 溢出不会按已提出路径泄漏 provisional candidate

原假设认为：lifecycle-incapable client 在 `session/load` 期间的 live buffer 超过 4096 后会进入 fallback，直接收到未接纳 preview。

实际控制流在 `leader/server.rs` 中先计算 `buffer_unknown`；对于不支持 `sampling_attempt_lifecycle` 的 client 立即 `continue`，之后才查找 load buffer 和执行 overflow fallback。因此 pending candidate payload 不能到达该分支。Accepted 时 payload 才被 fanout，此时已不是 provisional。

结论：不列 actionable defect。仍缺一个“incapable client + active candidate + in-flight full load + 超过 cap”的针对性测试；Accepted burst 绕过该 cap 的资源上限问题应作为独立债务调查，不能冒充本次预览泄漏。

## 4. 已验证强项

1. `RecoveryBudget` 在 sampler 重试与 Shell repair/resubmit 之间共享 attempt 数、绝对期限和 sticky replay safety；分类上限只会收紧。
2. provider wire activity 前先完成 attempt scope admission；pre-cancelled request 不获得账本范围。
3. 每 attempt evidence 和 usage sink 完成后才判断 retry 或把 candidate 返回 Shell；unknown usage 不冒充零。
4. malformed completed tool arguments 会拒绝整个 candidate，不执行健康 sibling，且恢复受统一 attempt allowance 限制。
5. ChatState 对 model attempt usage 以 attempt key durable 去重，exact duplicate 幂等、conflict 拒绝、冷恢复重建索引。
6. response durable admission 成功前不会发布 Accepted 或执行本地工具；quarantine response 不安装 native continuation，也不执行 sibling 工具。
7. provider-native terminal 与 host Turn terminal 分属不同事实；拒绝、content filter、截断和 recovery 不会被本地工具存在性覆盖。
8. 普通 leader/Pager 路径对 capable/incapable client 正确隔离 provisional output；Pager 的 unconfirmed-preview reconnect 会强制 full replay 并保留失败回滚所有权。

## 5. 验证缺口

- 没有 shutdown + blocked evidence/usage ACK 的集成测试。
- 没有 duplicate live ID + old task cleanup + settlement collision 测试。
- 没有 historical `BackendToolCall` 与 current hosted operation 的对照恢复测试。
- 没有 Shell 级 response-admission reply-loss 故障注入。
- 没有 Timeline admission → Accepted projection → leader replay → Pager scrollback 的 kill-point 端到端测试。
- 没有主 attempt、Goal、child usage 跨部分提交与冷恢复的完整矩阵。

这些缺口不改变已由控制流和契约直接确认的 finding，但限制对故障发生频率和修复完整性的证明。

## 6. 独立复核与定级裁决

只读独立复核再次确认 HIGH-1、HIGH-2 与 MEDIUM-2 的机制、生产路径和契约冲突；HIGH-1 的正常 teardown 可达性还由 `cancel_running_task` 先 abort foreground、随后 `shutdown_sampler` 进入 sampler actor 的顺序支持。Tokio 1.52.x 本地源码明确说明 `JoinSet::shutdown()` 等价于 `abort_all()` 后持续 join。

复核建议因主 turn 使用随机 UUID 而下调 MEDIUM-1；本报告仍维持 Medium，因为公共 sampler admission 接受 caller-supplied ID，触发后会同时破坏 live owner、取消与 settlement identity，已在影响中计入低生产概率。复核也建议因规范未明写“从 Timeline 重建 scrollback”而下调 HIGH-3；本报告维持 High，因为 `client-surfaces` 已要求 reconnect “只以已接纳内容展示成功历史”，而当前裂缝会让已接纳 response 永久缺失并使后续工具结果失去用户可见前件。该实现手段可以变化，但成功历史不能缺失的用户契约已经存在。
