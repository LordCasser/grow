## ADDED Requirements

### Requirement: Attempt evidence and accepted response have distinct authority

被废弃 attempt 的原始证据、用量及恢复决定 SHALL 保留在既有 Timeline 证据链或其不可变 artifact 引用中，但 SHALL NOT 投影成模型有效上下文、native continuation 或可执行工具。候选只有在会话 durable admission 确认后才能发布已接纳状态。接纳失败或确认不明 SHALL NOT 触发盲目重新采样。

#### Scenario: Failed generation is followed by a valid attempt
- **WHEN** 第一次候选被拒收而第二次被持久化接纳
- **THEN** 证据可以追溯两次调用及废弃原因，模型 Surface 和可执行工具只包含第二次被接纳结果。

#### Scenario: Admission write acknowledgment is lost
- **WHEN** 响应可能已经写入 Timeline 但提交确认丢失
- **THEN** 停止新采样并通过原提交身份核对，不能把确认丢失当成模型生成失败而重复提交或执行工具。

#### Scenario: Process stops before attempt closure
- **WHEN** 进程在 provider 调用后、证据/结算/接纳闭合前终止
- **THEN** 恢复保留未确认状态和可用证据，不把未确认 attempt 自动重放为成功消息，也不据此自动重发 provider 请求。
