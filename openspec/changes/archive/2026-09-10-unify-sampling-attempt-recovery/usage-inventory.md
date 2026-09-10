# Attempt usage inventory

> 以下为实施前/实施中阶段的静态审计快照，保留当时的缺口和建议，不代表最终实现。最终接线、验收和限制以 [verification.md](verification.md) 为准；旧行号仅用于追溯。

2026-09-10。本文是对当前工作树的有界静态核对；不代表实现已完成。行号以本次核对的工作树为准。父子消息相关文件有并行未提交改动，未纳入本报告的行为判断。

## 结论先行

`sampler::AttemptUsageSink` 已经在 `run_request_task` 中对每个真实 provider attempt 调用，并在重试/终止前等待其 future 完成。但当前 shell sink 只有 `scope=Some` 时才做 Goal settlement；`scope=None` 的 `Known` 与 `Incomplete` 分支是空操作。因此没有 Goal 的普通 prompt、普通 session/model ledger 和 `TaskOutputTokenBudget` 尚未在被 sampler 内部吞掉的失败 attempt 上结算。

普通 ledger 的现有写入只发生在最终成功接纳路径和最终 `SamplingEvent::Failed` 路径。把它直接复制进 sink 会使最终失败 attempt 被结算两次；应先把最终失败路径改成只投影诊断，再由 attempt settlement 负责消费。

## 现有路径（源码证据）

### sampler attempt 边界

- `sampler/src/handle.rs:218-227` 的 `submit_and_collect_accounted` 接受 `AttemptScopeCapture`、`AttemptUsageSink` 和 `EvidenceSink`。
- `sampler/src/actor/request_task.rs:180-207` 每轮建立 attempt evidence，并以 `request.clone()` 调用 `run_one_attempt`；`attempt_number = retry_count + doom_retry_count + 1`。
- `sampler/src/actor/request_task.rs:224-267` 从 Completed、Empty、Failed、truncation/pause 等结果提取 usage；无 usage 时保留 `None`，仅未 dispatch 的 evidence 或严格图片能力拒绝可以得到精确零。
- `sampler/src/actor/request_task.rs:282-303` 在 evidence ACK 之后仍会等待 `AttemptUsageSink`；`Known` 或 `Incomplete` 的 sink 错误会终止请求。随后才检查 evidence 结果、取消和 retry decision。
- `sampler/src/actor/request_task.rs:920-1027` 的 `drive_l2` 将流的 Completed/Failed/取消归并成 attempt outcome。取消发生在首次 provider poll 之后时，scope slot 变为 `provider_started=true`，最终也会发 Incomplete usage。

因此，sampler 侧已有“每次真实 provider poll → usage callback → 决定下一步”的时序。现有 AttemptUsage 没有显式 request/attempt key，只有可选 Goal scope 和 TokenUsage：`sampler/src/handle.rs:24-36`。

### regular shell → Goal 与普通账本

- `shell/src/session/actor/turn/sampling.rs:1937-1951` 为每个 regular sampler request 捕获 owner epoch 和当前 Goal；`begin_model_attempt` 在首次 provider poll 处返回 `Some(attempt_id)` 或无 Goal 时返回 `None`。
- `shell/src/session/actor/turn/sampling.rs:1953-2001` 的 sink：
  - `Known + Some(attempt_id)` 转为 `GoalTokenUsage`，调用 `GoalUsageWindow::settle_attempt_via_root` 并等待 root ACK。
  - `Incomplete + Some(attempt_id)` 先调用 `ChatStateHandle::mark_usage_incomplete(true, true)`，再调用 Goal root settlement；两者都必须成功。
  - `Known + None` 和 `Incomplete + None` 是 `{}`，没有普通账本、model 统计或 task grant 更新。
- `shell/src/session/actor/turn/sampling.rs:2192-2232` 的 `record_response_token_usage` 在响应被 durable `push_response_durably` 后执行：有 usage 时更新 TaskOutput grant、context anchor、last-turn slot、prompt/session/model ledger、Goal（仅传入的 `admitted_goal_id`）和 signals；无 usage 时仅对有 Task budget 或 `sampler_retry_only_before_output` 的 child 标记不完整。
- regular sampler 调用在 `shell/src/session/actor/turn/mod.rs:1291-1299`；成功响应随后在 `turn/mod.rs:1526-1548` durable admission 后调用 `record_response_token_usage`。
- 最终失败事件在 `shell/src/session/actor/tool/result.rs:766-780` 再次将 `error.usage` 写入 TaskOutput grant、last-turn slot、普通 model ledger 和 signals。这里没有 Goal 写入，因为注释和 sampler sink 假定 Goal attempt 已结算。

普通 ledger 的底层实现是 `chat-state/src/actor/mutations.rs:593-619`：`ChatStateHandle::record_model_call_usage` 把一次 call 同时 fold 到 open prompt `UsageLedger` 与 lifetime session `UsageLedger`，并按传入 model id（缺失则当前 sampling config model）维护 `by_model`。handle API 在 `chat-state/src/handle.rs:116-127` 是 fire-and-forget，没有 ACK、attempt key 或去重。

### Goal root settlement

`shell/src/session/actor/goal_support.rs` 已有最接近目标的 ACK/幂等协议：

- `begin_model_attempt_with_background`（约 `191-248`）在 owner epoch、Goal provider window 和旧 attempt settlement fence 通过后创建 attempt id；无 Goal 返回 `None`。
- `claim_attempt_settlement`（约 `296-312`）只接受第一次 Known/Incomplete 结果；后续 payload 不会覆盖已 claim 的结果。
- `settle_attempt_via_root`（约 `362-386`）按 attempt id 发送 `SessionCommand::SettleGoalUsageAttempt`，等待 oneshot ACK；root 在 `settle_claimed_goal_usage_attempt_outcome`（约 `670-706`）读取 shared attempt result，持久化 Goal usage，成功后才 `finish_attempt`。未收到 ACK 时 attempt 保留，可用同一 id 核对/重试。
- `account_captured_goal_usage`（约 `534-571`）在 Goal transaction gate 下持久化 usage；未知 usage 走 `apply_captured_goal_usage_incomplete_outcome`（约 `603-667`），精确预算关闭 provider admission，未预算 Goal 保留下界并继续。

这套接口只覆盖 Goal。其 `bool` 返回值代表“是否仍属于可应用的 Goal”，不是普通账本写入的 attempt 去重结果。

### child task 与 output grant

- `shell/src/agent/subagent/handle_request.rs:1046-1052` 从 runtime override 创建 `TaskOutputTokenBudget`，并设置 `sampler_retry_only_before_output=true`。
- `shell/src/tools/tool_context.rs:20-78` 的 grant 以 `spent`/`incomplete` 维护剩余量；`clamp_request` 可将请求上限夹到剩余值，`record_reported_output` 增加 completion tokens，`mark_incomplete_and_exhaust` 对未知用量 fail-closed。
- 当前唯一生产调用链是 `ToolContext::clamp_task_model_request`（`tool_context.rs:195-215`），由 `turn/mod.rs:1266-1269` 在进入 regular sampler 前调用；因此它只夹 outer loop 的 request。
- sampler 的内部 retry 在同一个 `run_turn_via_sampler` 调用内执行，使用 `request_task.rs:192-202` 的 `request.clone()`。即使 sink 现在更新 grant，当前请求对象的 `max_output_tokens` 也不会在下一 internal attempt 重新夹值。
- child 结束时 `handle_request.rs:1764-1793` 从 child session ledger 与 TaskOutput budget 汇总 `output_tokens_used`/`output_usage_incomplete`，之后通过 `record_subagent_usage`（约 `1-2` 行入口及 `session/actor/updates.rs:36-58`）等待 parent ChatState ACK；该 fold 是 child 完成后的 parent 汇总，不是 provider attempt settlement，也没有 attempt id。

## 计费缺口与重复风险

### 明确缺失的入口

1. **普通 regular sampler 的内部重试**：第一次 attempt 的 Known usage 被 Goal sink 记录（若有 Goal），但 scope=None 时被丢弃；无 Goal 的 prompt/session/model ledger 与 TaskOutput grant 都不增加。最终 retry 成功只记录最后一笔，之前的真实 provider 消费消失。
2. **普通 regular sampler 的内部 Incomplete**：scope=None 分支跳过 `mark_usage_incomplete`。因此无 Goal 的流截断、连接阶段取消、timeout 或未知 usage 不会给普通 prompt/session ledger 留下 incomplete 标记；有 Task budget 的 child 只有在整个 sampler 返回后，outer error/success 路径才可能处理，且中间 attempt 仍不计。
3. **每次 internal retry 的 output grant**：outer `clamp_task_model_request` 只在 sampler 调用前运行；sampler 内部 attempts 复用同一 `ConversationRequest`，所以第二次 attempt 不会拿到扣除第一笔 output 后的新上限。
4. **sideband/auxiliary provider calls**：`SidebandRun::run_provider`（`shell/src/session/actor/sideband.rs:428-506`）与多个 `conversation_collect` 调用只通过 GoalUsageWindow settlement；`SidebandUsage` 被写入 sideband Timeline，未调用 `ChatStateHandle::record_model_call_usage` 或 `TaskOutputTokenBudget`。这可能是现有“side calls 不进入 main-loop ledger”的有意边界，但若本 change 的“每个真实 provider attempt”包含 sideband，则 scope=None sideband 也是缺失入口，必须在实现范围中明确。

### 当前可观察的重复风险

1. 若把普通 ledger 写进 AttemptUsageSink 而保留 `tool/result.rs:766-780`，最终 Failed attempt 会先由 sink 写入，再由 Failed 事件写入一次；同一个 `error.usage` 会被双计。
2. 若把 TaskOutput grant 写进 sink 而保留 `record_response_token_usage` 的 `record_task_model_output`（成功）或 Failed 分支（最终失败），最终 attempt 会双计。移除终端事件路径的消费累计，只保留诊断/response usage projection，才能让 settlement 成为唯一消费入口。
3. 若把 Goal charge 同时传入 sink 和 `record_response_token_usage(..., admitted_goal_id=Some(_))`，Goal 会重复累计。当前 regular sampler 传 `None`（`sampling.rs:1541`），测试直接调用 `record_response_token_usage` 传 Some 是另一条旧式路径；实施时必须统一 ownership，不能两边都 charge。
4. child 完成时的 `record_subagent_usage` 是 parent fold；若 child internal attempts 已经进入 parent ledger，又把 child aggregate 再 fold，则会重复。parent fold 应只保留 sideband/child session 的跨 actor 汇总边界，并改为使用已结算且去重的 child totals。

## 最小接口改法（供主 agent 做架构决定）

1. **扩展 attempt 归属，而不是新建 UUID 服务**：让 `AttemptUsage` 携带稳定的 `(RequestId, attempt_number)`（Goal 仍复用已有 scope attempt id）。`run_request_task` 已有这两个事实；同一 key 是普通 prompt/session/model 与 TaskOutput 的去重依据。API 可以保持一个 sink，但不应继续只传 `Option<Goal scope>`。
2. **把普通账本写入收敛为一个 await-ACK 的 ChatState mutation**：在 `ChatStateCommand::RecordModelCallUsage` 增加 attempt key、incomplete/known 结果和 oneshot reply；actor 以 key first-wins 后同时更新 prompt/session `UsageLedger` 与 `by_model`。当前 `record_model_call_usage` fire-and-forget 不能证明 settlement 已落地，也不能安全支撑 ACK 丢失重试。`mark_usage_incomplete` 已有 await ACK（`handle.rs:275-282`），但没有 attempt key，适合作为单调的账本状态更新，不足以独立去重 Known。
3. **复用 Goal 的 ACK 顺序**：一个 shell attempt settlement callback 先以稳定 key 应用普通 ledger/Task budget，再等待 Goal `settle_attempt_via_root`；任何一项 ACK 丢失都关闭下一次准入，重试只重放同一 key，不能重新 poll provider。若要保留“两独立 durable ledgers”语义，至少要把普通账本 ACK 结果显式返回，避免 Goal 成功而普通账本未确认时继续恢复。
4. **每次 provider admission 重新计算 output grant**：给 accounted sampler 增加一个每-attempt request preparation/clamp callback，或让 attempt admission 返回当次 request snapshot；`run_request_task` 在每轮 provider poll 前从 `TaskOutputTokenBudget::remaining()` 重新生成 `max_output_tokens`。只在 outer loop 夹值不能满足 internal retry。未知 usage 必须先 `mark_incomplete_and_exhaust`，再阻止下一次 admission。
5. **普通 ledger 的 model id 与 duration/cost 要在 attempt 结算时冻结**：regular path 已在 `turn/mod.rs:1292-1295` 捕获 `usage_model_id`，应把该快照和 `api_duration_ms`/`cost_usd_ticks` 一起传给 settlement；不能在终端事件阶段再从 mutable current config 推导。Sideband 若纳入本 change，也需显式提供 route model、duration/cost 和 prompt attribution，而不是复用 main-loop `record_main_loop_call` 的语义。
6. **scope=None 必须真正生效**：Known 仍写普通 prompt/session/model + Task budget；Incomplete 至少给 prompt/session ledger 标记 incomplete，并在有限 grant 上 fail-closed。Goal 的 scope 只决定额外的 Goal charge，不决定普通结算是否发生。

## 已有 ACK/幂等测试资产

- `sampler/src/actor/request_task.rs` 的 `malformed_completed_tool_arguments_recover_with_bounded_accounted_attempts`（约 `1207` 起）：mock Responses 流，断言 invalid attempt 后的 retry、原 request input 不变、每 attempt 都调用 usage sink；`unknown_usage` 断言 Incomplete；`usage_failure`/`persistence`/`cancel` 覆盖 ACK 失败和取消先后。
- `request_evidence_ack_gates_wire_for_every_backend`（约 `1460` 起）：Chat/Responses/Messages 三协议，request evidence 未 ACK 时不应发 wire；取消后已 first-poll scope 要收到 Known zero 或 Incomplete。
- `cancel_during_stream_open_settles_the_first_poll_scope_for_every_backend`（约 `1717` 起）：验证 provider poll 后取消会发带 scope 的 Incomplete。
- `usage_settlement_failure_emits_terminal_before_completion`（约 `1786` 起）：结算失败先产生 terminal Failed，再关闭 completion。
- `shell/src/session/actor/tests/record_response_token_usage_tests.rs`：`anchors_projected_context_from_response_usage`、`quarantined_response_is_billed_without_restoring_its_context_anchor`、`goal_usage_accumulates_model_consumption_when_context_pressure_falls`、`descendant_model_usage_is_submitted_to_the_root_goal_window`、无 usage projection 与 per-turn stash 测试。它们覆盖最终响应/Goal，但没有 sampler internal failure→success 的普通 ledger 两笔断言。
- `shell/src/session/actor/goal_support.rs`：`failed_goal_settlement_retains_attempt_and_retries_exactly_once`、`late_unknown_usage_preserves_stopped_goal_status`、`unbudgeted_incomplete_usage_preserves_admission_and_lower_bound`、`incomplete_usage_closes_admission_then_pauses_after_step_end`、root shutdown claim 测试，覆盖 Goal ACK/重复/未知边界。
- `shell/src/tools/tool_context.rs` 的 `clamps_every_request_to_remaining_and_stops_at_zero`、`unknown_usage_exhausts_grant_pessimistically` 只测纯 budget 对象；没有 internal retry 的动态 re-clamp 集成测试。
- `shell/src/session/actor/tests/subagent_usage_fold_tests.rs` 与 `agent/subagent/tests/mod.rs::usage_ack_precedes_terminal_presentation` 覆盖 child aggregate fold ACK、prompt attribution 与 apply miss；它们不证明 provider attempt 去重。
- `chat-state/src/actor/tests.rs::prompt_usage_ledger_via_handle_resets_and_clears` 验证普通 prompt/session ledger 的现有 fire-and-forget API 结果，但没有 ACK 丢失或 attempt idempotence。

实施后最小新增验证应是：无 Goal 的 Responses mock 第一 attempt Known/第二 attempt success，prompt/session/model 与 Task budget 各计两笔；第一 attempt Incomplete 使 ledger incomplete；有 Goal 时四个 ledger 各一笔且无 Failed 终端重复；第二次 wire 的 `max_output_tokens` 等于扣除第一笔后的剩余 grant；scope=None 与 scope=Some 都覆盖。

## Atlas 记录与限制

已先执行 `atlas project(action="open", project_path="/Users/lordcasser/workspace/projects/grow")`，随后做了 `search`/`symbol`/`calls` 的有界查询。Atlas 能确认 `run_request_task`、`ChatStateHandle::record_model_call_usage`、`TaskOutputTokenBudget` 的局部符号；部分 Focus call-graph 查询因 `.atlas/atlas.db` 被并行工作锁定而失败，且当前数据库包含已删除的旧 `session/acp_session_impl/sampler_turn.rs` 候选。Atlas 的空 callers 不能作为全仓库“无调用”证明；本报告的调用链以当前工作树 `rg`/源码读取为准。
