# Design: ask_user_question 迟到答案结算 — 源码事实与竞争解释

调查记录。最初按源码提出的解释保留在下文，后续worker判别与UI/transport复核已纠正其证据强度：5项判别（含rg对照）和14项问答worker回归通过，用户报告的持续卡住尚未复现。实际命令见tasks；两层oneshot与Pager清理的最终核对见[UI/transport review](ui-transport-review.md)。下文的旧位置和候选不作为已确认根因或待实施方案。

## 1. 真实通知链与 owner/state/代际 ID

1. **Tool**（`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs`）
   - 发送 `UserQuestionRequest { tool_call_id, deadline, result_tx(oneshot) }` 到 session 内 unbounded mpsc；`run()` 阻塞在 oneshot。
   - deadline 默认 30 min（`RESPONSE_TIMEOUT`，`timeout_enabled` 默认 true）。`timeout_enabled=false` → `deadline=None` → 永久等待。
   - 超时路径：返回 `CANCEL_TEXT`，**drop oneshot receiver**，注释明确"stale UI is cancelled when a new ext_method arrives"——即**不向 pager 发任何取消消息**。
2. **Worker**（`crates/codegen/shell/src/session/actor/spawn.rs:617` `start_user_question_worker`）
   - 每 session **一个串行** LocalSet 任务，biased select：`shutdown.cancelled` > `request.result_tx.closed()` > `gateway.ext_method(ext_request)`。
   - `result_tx.closed()` → 放弃 ACP 等待（drop ext_method future），按是否过 deadline 记 `TimedOut`/`Cancelled`，随后仍走 `resolve_durably`（spawn.rs:759 附近）。
   - **更正（原结论有误）**：`resolve_durably`（`pending_interaction.rs:126` 起）成功移除 registry 条目后会通过 gateway 广播 `GrowSessionUpdate::InteractionResolved { tool_call_id }`（`grow/session_notification`，fire-and-forget）；`PendingInteractionGuard::drop`（`pending_interaction.rs:365` 起）在 first-answer-wins 下同样广播。因此 **worker 放弃等待时存在现成的取消/关闭通知链，无需新增 cancel 协议**。
   - Pager 侧：`session_notification.rs:1634/1906` 收到 `InteractionResolved` 调 `dismiss_resolved_interaction`（`agent_view/interactions.rs:1426`），按 `(session_id, tool_call_id)` 关闭 question/plan/permission 弹层并 `restore(stashed_prompt)`。原"pager overlay 残留、无取消链"的结论被证伪。
3. **Pending registry**（`crates/codegen/shell/src/session/pending_interaction.rs`）
   - 键 = `tool_call_id`（transcript 稳定）；代际 = `attempt_id`（uuid v7，`new_with_attempt`）。`resolve_durably` 校验 kind + attempt 双重所有权，first-answer-wins，失主返回 `Ok(false)`（静默）。
   - `PendingKind::Question` **不计入** keep-resident 判定（只有 PlanApproval 计入，`has_parked_plan_approval`）。
   - `resolution_gate`（async Mutex）跨 `record_timeline_event_durably` await 持有；`chat-state/src/handle.rs:184` 该调用 `deadline: None`（无界 durable 写）。
4. **Gateway/transport**（`acp-transport/src/gateway.rs:470`、`channel.rs:33 acp_send`）
   - `ext_method` → oneshot over unbounded channel，**transport 层无超时**。
   - **更正（待核 transport task）**：`enqueue`（gateway.rs:335 起）把 `response_tx` 移入 `AcpClientMessage` 本身；worker drop 它的 ext future 只 drop `response_rx`。因此"worker drop ext future ⇒ pager 侧 `response_tx` 已死"**不能凭两层 oneshot 等同**：`response_tx` 的存活由转发/接收消息的 transport task 决定，需按该 task 的实际生命周期核对，不得假设。
5. **Pager 接收**（`pager/src/app/acp_handler/interactions.rs:14`）
   - 按 `session_id` 路由到 agent view；**每个 view 仅一个 question slot**；新问题到达时对旧问题发 `Cancelled` 并替换。Pager 只知道 `(session_id, tool_call_id)`，**不知道 attempt_id/turn id**。
6. **Pager 提交**（`agent_view/interactions.rs:1518` → `question_view.rs:983 send_ext_response`）
   - `tx.send(Ok(...)).ok()` —— **最早硬丢失点**：shell 侧 receiver 已 drop 时答案被静默丢弃，无错误、无 timeline 事件、无用户提示。
7. **结算回程**：worker 收到 ext response → `pending_guard.resolve_durably`（durable timeline 写 + gate）→ 成功后才 `result_tx.send`。**答案被 UI 接受 ≠ tool 解除阻塞**：durable 写挂起则 tool 继续阻塞。

## 2. 三种场景可达性（源码更正后）

| 场景 | 可达性 | 结果 |
|---|---|---|
| blocking 当前请求、按时作答 | 可达 | `Answered`，tool 正常返回 |
| tool 超时/turn 被取消或 suspend（"任务已完成"）| 可达 | tool 已返回 CANCEL_TEXT 或 future 被 abort；worker 放弃 ACP 等待并走 `resolve_durably` → 广播 `InteractionResolved` → pager `dismiss_resolved_interaction` 关闭弹层并 restore stash。**无残留 overlay**（原结论证伪）；pager 随后的迟到提交送入已放弃的 oneshot（transport task 存活期内）静默失败 |
| 迟到答案送旧 Turn / 新问题已开 | 部分可达 | 新问题替换 slot 时旧 tx 收 `Cancelled`；worker 侧 first-answer-wins（attempt 所有权）使迟到答案无法再结算旧 attempt 或新问题；无 attempt/turn 级绑定供 pager 侧校验 |

ask 阻塞 tool loop，所以正常路径下 turn 不可能在 ask 挂起时结算——"任务已完成"必然经过：tool 超时、turn cancel/suspend abort tool future、或 ask 来自 background/child session 而 parent 主会话自行结束。

## 3. 竞争解释（更正后）

**E1 答案接受后 durable 结算挂起（通用 persistence 前提）**：`resolve_durably` 内 `record_timeline_event_durably` 无界（`deadline:None`）且 `resolution_gate` 串行化 → durable ACK 被扣留时 tool oneshot 永不收到。retained-ACK fixture 证明该**通用前提成立**（park 时 tool 停塞，释放后按 Answered 结算；unknown ACK 不会被当 Failed）。但这只是前提：**不能据此宣称用户场景的 root cause 是 persistence park**，需证明生产 persistence 在该场景真的 park。
- 反证缺口：`query()` 的 reply 通道 drop 时返回 `AcknowledgementLost`（handle.rs:230），即 chat-state actor 存活但忙/卡时确实无界。

**E2 迟到答案静默丢失（修正）**：tool 已超时/被取消后，worker 已广播 `InteractionResolved` 且 pager 已关弹层，迟到提交通常已无 UI 可达；若 transport task 仍持有 `response_tx`，送入已放弃的 receiver 静默失败。症状是"答案被忽略"而非主会话卡住；且弹层残留说法不成立。

**E3 串行 worker 饥饿**：`timeout_enabled=false` 或首个问题无人答且 ext_method 无 transport 超时 → worker 停在第一个 `gateway.ext_method`，同 session 后续 ask 永远排队不开 UI → "后来的 ask 长时间无人回应"。

**等待/取消链结论**：现有 `InteractionResolved` 广播（resolve_durably + GuardDrop 双发、first-answer-wins）已是足够的关闭信号，**无需新增 cancel 协议**；pager 已按 `(session_id, tool_call_id)` 消费它。

## 4. 历史修复候选与最终裁决

以下为早期候选，未实施；父最终裁决见[verification.md](verification.md)。缺少用户场景因果证据时，不新增cancel、durabledeadline或Pager代际协议；非阻塞推荐继续另属独立change。

1. ~~worker 放弃等待时向 pager 发显式取消通知~~（更正：`InteractionResolved` 链已存在并被 pager 消费，无需新协议；若需可观测性，可考虑给"答案到达但无主"记 timeline 事件，属可选增强）。
2. 给 worker 的 `resolve_durably` 等待加界（有界 durable deadline），失败按 `Failed` 结算并解除 tool 阻塞 —— 仅在 E1 被证实为用户场景 root 后实施。
3. 供 `feature-ask-blocking-choice` 使用的事实：terminal question identity 今日 = `(session_id, tool_call_id)` + shell 内部 `attempt_id`；pager 侧零代际绑定，仅 slot 替换；任何 blocking 设计需把代际带到 pager 或由 pager 回传校验。

## 5. 最小 call-boundary fixture（worker部分已执行）

- **F1（pager 边界）**：`QuestionViewState::with_response_tx` + drop 接收端 + `send_ext_response`（pager 侧另行处理，不在本 change 写 scope）。
- **F2（worker 边界）**：已写入 `reverse_request_session_id_tests.rs`：
  - `answered_question_after_long_idle_settles_exactly_once_and_worker_continues`（场景1：受控长等待后作答，result 与 canonical `InteractionResolved` 各一次，worker 续答下一问）。
  - `late_acp_response_after_receiver_close_is_inert_and_worker_survives`（场景2：tool receiver 关闭 → 观察到 worker 真发 `InteractionResolved` 广播 → 迟到 ACP response 惰性（无二次结算、无新事件）→ 问题 B 正常作答）。
  - 通过即反证：**不得宣称用户 bug 已复现或修复**。
- **F3（E1 retained-ACK）**：`retained_durable_ack_parks_open_and_resolution_until_released`（Open ACK 与 resolution ACK 分别扣留：扣 Open 时不得发 ACP 请求；扣 resolve 时 tool 停塞且不记 Failed，释放后按 Answered 结算）。只证明通用 persistence 前提。
