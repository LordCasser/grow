## ADDED Requirements

### Requirement: Durable timeline authority
会话 SHALL 将完整消息与生命周期边界追加到 Timeline，并从这些事实派生上下文；流式 delta 只用于传输。

#### Scenario: 持久化后发布消息
- **WHEN** 用户消息被提交
- **THEN** 写入等待 Timeline 的持久化确认后才完成。

证据：`crates/codegen/chat-state/src/actor/tests.rs` — `push_user_message_durably_waits_for_timeline_commit`。

### Requirement: Replayable model surface
恢复 SHALL 重放模型 Surface 并继续既有事件序号。

#### Scenario: 恢复已有会话
- **WHEN** 从已保存 Timeline 重建 ChatStateActor
- **THEN** 恢复的 Surface 与事实一致，后续事件延续序号。

证据：`crates/codegen/chat-state/src/actor/tests.rs` — `restored_actor_replays_surface_and_continues_event_sequence`。

### Requirement: Integrity repair preserves evidence
工具响应完整性修复 SHALL 保留原始事实，并将修复后的 Surface 用于后续模型请求。

#### Scenario: 响应配对损坏
- **WHEN** 接收包含无效工具协议的聚合响应
- **THEN** 保留原始响应与修复事实，下一次请求使用可接受的投影。

证据：`crates/codegen/chat-state/src/actor/tests.rs` — `response_repair_retains_raw_fact_and_allows_the_next_request`。
