## MODIFIED Requirements

### Requirement: Inquiry context visibility is scoped to each participant

通过 agent 工具发起的 peer 或直接父子询问 SHALL 将问题保留为调用方的原始工具调用，将回答或失败保留为调用方的原始工具结果/receipt。目标 SHALL 在独立、无工具的冻结上下文询问中生成回答，并在成功 `InquiryAnswered` durable receipt 后，将完整问答作为 runtime typed `AgentMessage` canonical item 纳入当前回答方后续获准采样。该项 SHALL 保留 requester/source session、inquiry ID、answering session、frozen Sideband proof 与完整问题和回答。Chat Completions 与 Messages SHALL 使用既有 paired host `receive_agent_message` call/result；Responses SHALL 保留既有 attributed-context fallback，不伪造模型 authored call、reasoning 或 executable foreground tool execution。此同步 SHALL NOT 抢占执行、主动开启空闲 Turn、唤醒或授权 Session，亦不创建回复权限。协调 API 的响应 SHALL 交付其调用者，不自动构造来源 Session 的模型输入；询问方原有工具调用及结果/receipt SHALL 保持不变且不重复注入。

#### Scenario: Answer reaches the asking agent

- **WHEN** agent 调用 `ask_session`、`ask_parent` 或 `ask_subagent` 并成功获得 Sideband 回答
- **THEN** 询问方仅通过原工具调用和结果/receipt 看到该回答；回答方的下一获准采样通过既有 `InquiryAnswered` receipt 对应的一份 typed `AgentMessage` 看到完整问答、双方会话身份、inquiry ID 与 Sideband 快照依据。

#### Scenario: Provider context uses established adapters

- **WHEN** 成功 Q&A 被投影给回答方的 Chat Completions、Messages 或 Responses 请求
- **THEN** Chat Completions 与 Messages 使用现有 paired host `receive_agent_message` call/result，Responses 使用现有 attributed-context fallback；任何适配都不制造模型 authored call、reasoning 或 executable tool action。

#### Scenario: Consultation does not trigger execution or authority

- **WHEN** 成功 Q&A 被回答方持久接收或在边界消费
- **THEN** 它仅提供 attributed context，不启动新 run、不唤醒或 steering Session、不扩大工具权限，也不授权向询问方回复。

#### Scenario: Query and passive progress have different visibility

- **WHEN** 调用方收到被动 inquiry phase 更新，随后显式调用 `get_inquiry`
- **THEN** phase 更新本身不新增模型消息，显式查询结果按普通工具结果进入调用方上下文，不触发目标重新执行询问或重复接纳问答。

#### Scenario: Replay does not inject inquiry content

- **WHEN** 已完成询问在任一端重连或 UI 回放时重建显示
- **THEN** 显示恢复不重新发起询问或派发模型输入；runtime 冷恢复仅按 canonical 成功事实与消费状态恢复尚未完成的回答方被动同步，不重复已消费 Q&A、询问方原工具结果或 Sideband 执行。

#### Scenario: External coordination API caller

- **WHEN** 外部调用方通过协调 API 发起询问并取得成功响应
- **THEN** 响应返回 API 调用方，来源 Session 不自动获得合成工具结果或用户消息；回答方仍按相同成功 receipt 生成被动咨询证据。

#### Scenario: Inquiry does not succeed

- **WHEN** 询问被拒绝、失败、超时或取消且没有成功 terminal
- **THEN** 保留原审计和工具错误，但不生成包含成功问答的回答方上下文通知。

#### Scenario: Complete consultation cannot fit its existing delivery limit

- **WHEN** a valid Sideband produces an answer whose fully rendered attributed question/answer exceeds the existing notification payload byte limit
- **THEN** the inquiry returns a typed failure before publishing its successful incoming terminal/proof, retains the completed Sideband evidence and usage, and creates no impossible synchronization obligation; it does not truncate the answer or automatically retry.
