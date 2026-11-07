# Design: finalize-child-hooks-before-result

## Context

已核对的当前事实（base `8ab486a6`，隔离 worktree）：

| 位置 | 事实 |
| --- | --- |
| `agent/subagent/handle_request.rs` 收尾 | `execution_returned` 后建 30s finalization deadline；先写 output artifact，再 `record_child_result_before` 经共享 ChatStateHandle 直接写 child `SubagentResult`，再父 usage fold、reparent，最后 `child_handle.cmd_tx.send(SessionCommand::Shutdown)`，然后父 `Ended`（引用 child result ref）与 completion receipt。 |
| `session/actor/run_loop.rs` | `Shutdown` 臂 latch Graceful → `begin_graceful_shutdown`（关准入、退役 foreground/控制/辅助、cancel 子任务）→ `pending_shutdown` → loop 退出 → `finish_graceful_shutdown`：permission audit、workflow drain、SessionEnd Hook 派发、memory on_session_end（非 subagent）、dream、辅助 drain、memory summary 发送、Goal 用量结算与 checkpoint、`final_session_persistence_flush`。 |
| `session/actor/hook_dispatch.rs` | `dispatch_hook_occurrence` 在执行任何 handler 前先 `record_hook_event(Hook::Triggered)`，handlers 为空也写。 |
| `chat-state/src/timeline.rs` | `validate_header` 在 `subagent_result_recorded` 后拒绝所有 append（`SubagentTimelineEnded`）；该标志由 `SubagentResult` 事件设置。 |
| `session/actor/teardown.rs` | `final_session_persistence_flush` 先 `fail_stop_sideband_admission` + 取消 4 个 sideband/durable-UI token + 5s `session_activities.wait_idle()`，再 `FlushAndAck`；drain 失败时扣留最终 frontier。 |
| 取消/panic 路径 | `ChildControl::cancel` 发 `Cancel`（child actor 存活）；`runner_panicked` 发 `Cancel`+`Shutdown` 后走 `finalization.rs::reconcile_shell_child` 的冷恢复（`ensure_recovered_child_result_with_opened`），不经 runner 正常收尾。 |

由此：runner 先写 Result、后发 Shutdown 的顺序使 SessionEnd 的第一笔 Hook 写必然被封闭保护拒绝（已由隔离 child 日志证实）。

## Goals / Non-Goals

- 正常与取消路径下，child 的 SessionEnd 生命周期完整执行，`SubagentResult` 是 child Timeline 最后一条正常事实；父 `Ended`/receipt 与现在一样严格引用同一结果。
- 失败路径不伪造成功：typed 失败、unknown ACK、deadline 均保持 pending 原 identity，交由现有冷恢复；不重放 Hook、不二次 Result。
- 不改公共工具/API；不放宽 Timeline 封闭保护；不改 usage fold、rewind、file_state、sampler 与 turn settlement。

## Decisions

### 1. 内部终态提交命令（唯一新增命令）

`SessionCommand` 新增变体（`session/commands.rs`，非公共 API）：

```rust
CommitSubagentResult {
    result: chat_state::SubagentResultEvent,
    deadline: tokio::time::Instant,
    respond_to: oneshot::Sender<
        Result<chat_state::TimelineRangeRef, chat_state::TimelineWriteError>,
    >,
}
```

选择“命令自带 Graceful latch”而非“先 Shutdown 再发提交”：run_loop 的 cmd 分支带 `if pending_shutdown.is_none()` 门，Shutdown latch 后普通命令不再分发；由该命令自身 latch 并携带待提交终态是唯一能保证顺序的通道。处理臂镜像 `Shutdown` 臂：`runtime_suspend` 活动时回 `Err(TimelineWriteError::Cancelled)` 并跳过（物理交接拥有自己的资源 drain）；latch/`begin_graceful_shutdown` 失败走既有 `terminate_failed_timeline_writer` 并回 `Err(Cancelled)`（“未达 durable 即被取消”，不悬垂 runner 到 deadline）；成功则 `pending_shutdown = Some(GracefulShutdown { reason: "shutdown", terminal_commit })`。`reason` 保持既有 `shutdown` 值：Hook payload 的 `reason` 是已发布脚本可见字段，不新增枚举值。

### 2. `finish_graceful_shutdown` 的提交点与 append owner 排空

`GracefulShutdown` 增加 `terminal_commit: Option<SubagentTerminalCommit>`（event + deadline + respond_to；既有三处构造填 `None`）。提交点在 permission audit fail-stop 检查通过之后、`final_session_persistence_flush` 之前（父方 review 修正：audit 未 durable 时沿用既有 fail-stop，不得提交 canonical Result 或 ACK 成功；此时 responder 被 drop，runner 得到 unknown 并交恢复）：

- 之后不再有正常 producer：memory summary 发送（仅 diagnostics）、Goal 结算/checkpoint（可能写 Control 事件）、permission audit 都在提交点之前；`final_session_persistence_flush` 不写 Timeline。
- Result 不耦合于 audit 成功之外的收尾；audit 失败路径与旧顺序一致地扣留全部最终事实。

父方补充约束：Result append 前必须确认所有可 append owner 已退役，不能依赖“最终 flush 在 Result 之后”兜底。`teardown.rs` 把 `final_session_persistence_flush` 的准备段最小拆出为 `drain_final_persistence_owners(session) -> bool`（`fail_stop_sideband_admission` + 取消 4 token + 5s `wait_idle`；各操作幂等），`final_session_persistence_flush` 照旧先调用它再 flush，普通 shutdown 语义不变。终态提交段先调用它：drain 失败则不提交、回 `Err(Cancelled)`，后续最终 flush 照旧被扣留（与今日 owner 未排空的行为一致）。

提交本身：`chat_state_handle.record_timeline_event_durably_before(SubagentResult(event), deadline)`；成功 ACK `TimelineRangeRef { timeline_id: session_id_string(), first_seq, last_seq }`；失败 ACK typed 错误并记录 error 日志。提交后 session 继续既有收尾（audit 检查、final flush、signals、cleanup）。

### 3. runner 侧交接与 ACK 映射（`agent/subagent`）

- `record_child_result_before` 重构为“准备 + 交接”：先按原逻辑读 child planning_state 组装 `planning_summary`、拼 `subagent_result_fact(result, output_ref)`（不丢 planning_summary、不复制结果文本），然后向 `child_handle.cmd_tx` 发 `CommitSubagentResult` 并 await ACK（受外层 30s `timeout_at` 约束，不做第二个内层期限）。
- ACK 映射：成功 → `child_result_ref`；typed 失败（含准备失败 `Invalid(InvalidSubagent)`）→ 既有 `FinalizationFailure::timeline(ChildResult, ...)` 记录路径；发送失败（通道关闭/actor 已退役）或 ACK 通道被 actor drop → `TimelineWriteError::AcknowledgementLost`（certainty=Unknown）：恢复路径 `ensure_recovered_child_result` 先核对既有事实再决定是否补写，Unknown 是安全方向。
- 交接成功（命令已送达）即由该命令拥有 child shutdown：2671 处不再无条件补发普通 `Shutdown`；仅在未完成交接（output artifact 失败、准备失败或发送失败）时发普通 `Shutdown` 排空 child。deadline fallback 分支同规则（用交接标志判断，不再无条件二次 Shutdown）。permission release、`end_local_session`、worktree snapshot/移除、父 `Ended`、receipt 均不变。
- 早期失败路径（spawn 前/`RestoreBlocked` 等在 1837/1908 已发 Shutdown 或未建 actor 的）不经过终态交接，行为不变。

### 4. 取消、panic 与冷恢复边界

- 取消：`ChildControl::cancel` 只发 `Cancel`，child actor 存活并结算 turn 后仍能接收终态提交 → 取消结果沿用同一顺序（Hook 在前、取消 Result 最后）。
- actor 已退役（早期 Shutdown 已 latch、通道关闭）：交接发送失败 → Unknown → pending 原 identity，deadline 不续期；冷恢复 `ensure_recovered_child_result` 沿 exact 身份/owner lease 读取既有 Hook 事实补 Result，不执行外部 Hook 或 provider。
- panic：`runner_panicked` 已发 Cancel+Shutdown，`reconcile_shell_child` 冷恢复路径不变。
- 写已成但 ACK 丢失：恢复核对原 identity 命中即复用，绝不二次 Result 或父 receipt。

## Risks / Trade-offs

- 30s 收尾预算现需覆盖 child SessionEnd 生命周期（Hook 超时受既有配置约束）：超时 → pending + 恢复，不放宽期限、不取消在途 I/O 所有权。
- child Timeline 在 Result 前保持开放更久：期间该 child 已封闭准入且 foreground 退役，无新增并发写风险；封闭 validator 不变。
- 交接标志误判会导致 child 不关闭：仅在“命令已成功 send”时跳过补发 Shutdown；send 成功而 actor 之后才崩溃属于既有 actor 自愈/恢复范围。

## Migration Plan

本 change 一次性切换 runner→child 的终态写入通道；无兼容期（不考虑后向兼容）。同一 change 内更新 child 终态相关测试与开发者说明引用；归档由父统一执行。
