# Delivery inventory

> 以下为实施前/实施中阶段的静态审计快照，保留当时的缺口和建议，不代表最终实现。最终接线、验收和限制以 [verification.md](verification.md) 为准；旧行号仅用于追溯。

本文件是有界只读核对记录。没有修改运行时代码，未运行 `cargo`；行号以当前工作树为准。

## 实际输出路径

1. `crates/codegen/sampler/src/actor/request_task.rs`
   - `run_request_task` 为一次逻辑 request 建立 `RecoveryBudget`，循环调用 `run_one_attempt`。
   - `run_one_attempt` 在 `stream/{chat_completions,responses,messages}.rs` 的 L2 parser 上驱动流；`drive_l2` 将 `FirstToken`、`ChannelToken`、`ToolCallDelta`、`ResponseStarted`、`ReasoningCompleted` 立即送入 `event_tx`。
   - 正常 delta 只带 `request_id`，没有 attempt 号；attempt 只出现在 `Retrying` 事件和 evidence metadata。当前工作树的 `output_observed` 会影响 retry policy，`OutputDelivery` 默认/SessionActor 接线仍是 `Irreversible`。

2. `crates/codegen/shell/src/session/actor/tool/result.rs:615` (`SessionActor::handle_sampling_event`)
   - Text `ChannelToken` → `send_update(AgentMessageChunk, chunk_index)`。
   - Reasoning `ChannelToken` → `send_thought_chunk` → `send_update(AgentThoughtChunk, chunk_index)`。
   - `ToolCallDelta`、`ResponseStarted`、`ReasoningCompleted` → `send_buffered_grow_update`。
   - `Retrying` → `send_grow_notification(RetryState::Retrying { attempt, ... })`，是直接通知；它没有清旧块。
   - `Completed` 仅记账/释放 `turn_stream_drained`；最终 response 的持久化/`ResponseCompleted` 在 `turn/mod.rs` 的 `push_response_durably` 之后发生。

3. `crates/codegen/shell/src/session/actor/updates.rs`
   - `send_update_full` 把 ACP 更新包装为 `SessionEvent::Notification`，写入 `event_tx`，并生成 `eventId`、`promptId`、`streamStartMs`、`chunkId` 等 metadata；没有 attempt metadata。
   - `send_buffered_grow_update` 同样只构造 `GrowSessionNotification { session_id, update, meta: None }`。
   - `send_grow_notification`（RetryState 等单次状态）绕过 ReplayBuffer，直接持久化/转发；高频 ACP/ToolDelta 则走 ReplayBuffer。

4. `crates/codegen/shell/src/session/actor/run_loop.rs:1018-1031`
   - `SessionEvent::Notification` → `ReplayBuffer::consume_chunk` → `emit_buffered`。
   - `FlushReplay` 在 `:1080` 刷出 pending 项。
   - `updates.rs:257` 的 `emit_buffered`：ACP chunk 持久化并 gateway 转发；Grow ToolCallDelta 只 gateway 转发，不持久化（最终 canonical `ToolCall` 才是 replay source）。

5. `crates/codegen/shell/src/agent/update_chunk_merge.rs`
   - `ReplayBuffer::consume_chunk` 只按 session、时间窗、协议种类及 chunk 类型合并。
   - `merge_acp_chunks` 会拼接连续 Text/Thought 文本；`merge_meta` 仅维护 `chunkIdRange`。
   - `merge_grow_chunks` 以 tool id 或 `tool_index` 判定同一调用并拼接 arguments；合并结果显式丢掉 `meta`。若 attempt 重用 index，跨 attempt 的 ToolDelta 目前可被错误合并。

6. Pager：`crates/codegen/pager/src/app/acp_handler/mod.rs` 将 ACP notification 路由到 `agent.session.handle_update`；`crates/codegen/pager/src/acp/tracker.rs:AcpUpdateTracker::handle_update` 把 AgentMessageChunk/AgentThoughtChunk/ToolCall 更新直接写入 scrollback 和 active state。`streamStartMs` 只切断旧 message/thinking，不是 attempt 身份。
   - `crates/codegen/pager/src/app/acp_handler/session_notification.rs:1901` 的 `apply_retry_state` 对 `Retrying` 只清 retry activity；不会移除已追加的 text/reasoning/tool preview。
   - 旧块若已进入 scrollback，需要一个按 attempt 的 discard/reopen 操作；仅清 `running message` 不足以覆盖已经提交的块。

7. Headless：`crates/codegen/pager/src/headless.rs`
   - `handle_headless_acp_message:1438` 收到 ACP AgentMessageChunk/AgentThoughtChunk 后立即调用 emitter；ToolCall/Plan 等立即送 reducer。
   - `HeadlessEmitter::on_text_chunk`：Plain 直接 stdout + flush（不可撤回）；Json 只 buffer 到最终对象；StreamingJson/StreamingMessagesJson 立即 reducer 输出。
   - `on_thought_chunk` 在 Plain 丢弃，在 Json buffer，在两种 streaming format 立即输出。
   - `build_headless_init_request:465` 只发送 fs/terminal 与 `clientType=headless`、startup hints；没有 output delivery/retraction capability。`--include-partial-messages` 是本地 headless option，未进入 ACP init metadata。
   - `MessagesReducer` 可按 `ResponseStarted/ResponseCompleted` 分帧，但没有 attempt discard；partial 模式一旦发出 NDJSON frame，协议没有撤回帧。

## 可撤销性边界

| 消费面 | 当前行为 | 重试后能否撤回已发内容 |
| --- | --- | --- |
| sampler event channel / shell event queue | 进程内可在通知入 ReplayBuffer 前丢弃，但当前没有 attempt barrier | 可以设计为可撤回，尚未接线 |
| ReplayBuffer pending | 尚未 flush 的 ACP/Grow chunk 可直接丢弃；当前 `consume_chunk` 不识别 attempt | 可以；需要 discard 事件先清 pending |
| Pager scrollback / active text/thought/tool preview | `handle_update` 已经追加，`Retrying` 只更新 spinner | 当前不可撤回；需 attempt keyed staging 或显式 discard update |
| Headless `json` | 结果仍在 `text_buffer`，可丢弃旧 attempt 后输出 accepted response | 可撤回（实现需隔离 buffer） |
| Headless `plain`、`streaming-json`、`streaming-messages-json` partial | 已写 stdout/NDJSON | ACP/NDJSON 没有通用撤回帧；一旦发布必须禁止后续重采样 |
| 外部 ACP 客户端（未知实现） | 收到 SessionNotification 后语义不可假设可撤回 | 只有明确协商 retract/discard 能力时才可重采样 |

`stream/{chat_completions,responses,messages}.rs` 的协议 terminal/EOF 错误与“客户端已看到的输出”是两层概念：parser 可以把 provider attempt 判为失败并继续逻辑 retry，但已发往 ACP/stdout 的 preview 不会自动消失。

## 能力来源与最小接线

- 已有初始化能力入口：`shell/src/agent/mvp_agent/acp_agent.rs:45` 的 `initialize` 已读取 `clientType`、`clientIdentifier`、`bufferingSettings`；并保存到 agent 级状态，随后 `agent_ops.rs:1635` 读取并传给 session spawn。`SessionSpawnOptions`（`shell/src/agent/mvp_agent/mod.rs:114`）和 `SessionActor` 已有 client identifier/buffering settings 字段。
- 已有 per-request 入口：`acp_agent.rs:1401` 的 `prompt` 读取 `_meta.promptId`、`clientIdentifier`、`screenMode`、`verbatim`、`outputSchema`，再发送 `SessionCommand::QueuePrompt`。因此若能力因请求/消费者不同，应从 Prompt `_meta`（或新增 typed request field）读取，而不是从 sampler 猜测。
- leader 的 `ClientCapabilities`（`shell/src/leader/protocol.rs:121`）目前只覆盖权限、model、code nav、terminal、fs；没有 output delivery。Pager 初始化 (`pager/src/acp/mod.rs::initialize`) 也没有 retract/attempt capability。
- 最小建议：在 initialize 的 client capabilities/meta 中协商一个 typed delivery mode，并按 session 保存；leader 为多 client 场景把它注入 `session/new`/`session/load` 或 Prompt `_meta`。在排队 prompt 时将该 mode snapshot 到 turn context，传给 sampler policy。未知/未声明一律 `Irreversible`；headless `json` 可声明 `Buffered`，只有真正支持按 attempt discard 的消费者声明 `Retractable`。
- 若启用 recovery：每个 attempt 的所有 ACP/Grow 事件带同一个 attempt identity；Retrying 先入一个 FIFO barrier（丢弃该 attempt 的 ReplayBuffer pending，并通知 surface discard），随后才允许新 attempt 的 `ResponseStarted`/chunks。Pager/headless 以 attempt identity 丢弃/关闭对应 active text、reasoning、tool preview；accepted `push_response_durably` 仍是唯一 durable admission。
- 当前工作树的 `sampler::OutputDelivery` 已能在 `request_task.rs:357-365` 依据 `output_observed` 禁止 irreversible delivery 后 retry，但 `shell/session/actor/spawn.rs:516-534` 仍固定 `OutputDelivery::Irreversible`，这只是安全默认，不能替代上述 attempt 传播与清理。

## 现有测试入口

- Sampler retry/delivery：`crates/codegen/sampler/src/recovery.rs` 单元测试；`sampler/src/actor/request_task.rs` 的 malformed tool/empty/transport retry 测试；`sampler/tests/test_actor.rs` 的 retry policy fixtures；各 protocol parser tests：`sampler/src/stream/messages_tests.rs`、`stream/responses.rs`、`stream/chat_completions.rs`。
- Merge/replay：`shell/src/agent/update_chunk_merge.rs` 中 `grow_same_id_deltas_merge_args_and_preserve_name`、`grow_different_tool_call_id_forces_flush`、`timestamp_mismatch_flushes_pending`、ACP text/thought merge tests。
- Shell event path：`shell/src/session/actor/tests/` 下 actor/update and sampling fixtures；`shell/src/session/actor/spawn.rs` 的 `sampler_retry_policy_tests`。
- Pager surface：`pager/src/acp/tracker.rs` 的 `streaming_agent_message`、`streaming_thinking`、`tool_call_lifecycle`、`stream_start_breaks_*`；`pager/src/app/acp_handler/tests/session_events.rs` 的 `apply_retry_state_retrying_clears_in_flight_prompt` 及 failure/exhaustion tests。现有 retry test 只验证 spinner/prompt state，不验证清理 preview。
- Headless reducer：`pager/src/headless/reducer/messages/tests/content.rs` 的 response boundary/late completion/partial framing tests；`pager/src/headless/ext_protocol_tests.rs` 的 ResponseStarted/ResponseCompleted parsing tests。现无 attempt discard/retraction test。

## Atlas 记录与不确定点

已先执行 `atlas project(open)`，数据库为 `.atlas/atlas.db`，再对 sampler actor/events、shell event/update、ReplayBuffer、Pager tracker/headless 做 scoped search/incoming queries；没有运行全仓库 index。部分 Focus incoming-call 查询受 closure scope/FK 限制，且 Atlas 返回了一个不存在的旧候选路径（`session/acp_session_impl/...`）；该候选未作为事实使用，实际路径以当前源文件为准。空的 incoming 结果不代表没有调用方。
