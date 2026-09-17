## MODIFIED Requirements

### Requirement: Attempt evidence and accepted response have distinct authority

被废弃 attempt 的原始证据、用量及恢复决定 SHALL 保留在既有 Timeline 证据链或其不可变 artifact 引用中，但 SHALL NOT 投影成模型有效上下文、native continuation 或可执行工具。候选只有在会话 durable admission 确认后才能发布已接纳状态。每个产生 canonical assistant response 事件的新 admission SHALL 在该事件上携带原 sampler request 与最终 attempt 组成的不可变 identity 及确定性 admission result；同 identity、同 response payload 的本地 admission 重放 SHALL 幂等返回原结果，同 identity、不同 payload SHALL fail closed。接纳失败或确认不明 SHALL NOT 触发盲目重新采样。

#### Scenario: Failed generation is followed by a valid attempt

- **WHEN** 第一次候选被拒收而第二次被持久化接纳
- **THEN** 证据可以追溯两次调用及废弃原因，模型 Surface 和可执行工具只包含第二次被接纳结果。

#### Scenario: Admission write acknowledgment is lost

- **WHEN** 响应可能已经写入 Timeline 但提交确认丢失
- **THEN** 系统只按原 admission identity 和原 payload 核对或重放本地 Timeline admission，不启动新 provider 请求；确认仍不明时停止当前 Step/Turn，不发布 Accepted、不执行工具，也不进入 completion recovery。

#### Scenario: Exact admission submission is repeated

- **WHEN** 同一 response admission identity 以完全相同的 canonical response payload 再次提交
- **THEN** 返回原 admission 结果且 Timeline 只保留一份 response；不得重复安装或用历史重建 provider-native continuation。

#### Scenario: Admission identity is reused with a different payload

- **WHEN** 已存在的 response admission identity 被用于不同 canonical response payload
- **THEN** admission 以身份冲突失败，既有 Timeline 与 Surface 保持不变，不发布 Accepted 或执行任一 payload 的新工具。

#### Scenario: Process stops before attempt closure

- **WHEN** 进程在 provider 调用后、证据/结算/接纳闭合前终止
- **THEN** 恢复保留未确认状态、原 response admission identity 和可用证据，不把未确认 attempt 自动重放为成功消息，也不据此自动重发 provider 请求；历史无 identity 的 response 不能被猜测为该提交。

证据入口：`crates/codegen/chat-state/src/timeline.rs` 的 response admission fold，`crates/codegen/chat-state/src/actor/mutations.rs::push_response_durably`，`crates/codegen/shell/src/session/actor/turn/mod.rs` 的 response admission gate。
