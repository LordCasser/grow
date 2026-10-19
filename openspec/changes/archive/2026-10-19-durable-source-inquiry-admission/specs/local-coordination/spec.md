# local-coordination Specification Delta

## ADDED Requirements

### Requirement: Source inquiry startup admission is idempotent
Source SessionActor SHALL serialize outgoing inquiry startup admission against its canonical Timeline. For a `(source_session_id, inquiry_id)` already bound to a target and exact question, a retry SHALL return the existing admission without appending another `OutgoingStarted` fact or projecting another startup notice. A retry with a changed target or question SHALL return `Conflict` without mutation or dispatch. Question text SHALL only be compared as payload under the explicit inquiry identity and SHALL NOT serve as a deduplication key.

#### Scenario: Source retries an admitted startup
- **WHEN** 同一 source session 对已有 `OutgoingStarted` identity 以相同 target 和 exact question 重试，包括首次持久化 ACK 不明后的重试
- **THEN** actor 从 canonical Timeline 确认原绑定并返回已接纳，不追加 `OutgoingStarted` 或重复投影启动通知。

#### Scenario: Source retries with a changed target or question
- **WHEN** 同一 source session 对已有 identity 使用不同 target 或 exact question 重试
- **THEN** actor 返回 `Conflict`，不覆盖首次绑定、不追加事实或通知，也不 dispatch 冲突请求。
