## ADDED Requirements

### Requirement: Inquiry context visibility is scoped to each participant

通过 agent 工具发起的 peer 或直接父子询问 SHALL 将问题保留为调用方的工具调用，将回答或失败保留为调用方的工具结果。目标 SHALL 仅在独立、无工具的冻结上下文询问中使用该问题，并 SHALL NOT 因接收、回答、审批、取消、重连或 UI 回放向目标 foreground 上下文追加问题、答案或通知。协调 API 的响应 SHALL 交付其调用者，不自动构造来源 Session 的模型输入。

#### Scenario: Answer reaches the asking agent
- **WHEN** agent 调用 `ask_session`、`ask_parent` 或 `ask_subagent` 并获得回答
- **THEN** 调用方后续采样可见对应工具结果，目标询问前后主 Surface 不因该交互改变，目标接收视图仍可展示该问答。

#### Scenario: Query and passive progress have different visibility
- **WHEN** 调用方收到被动 inquiry phase 更新，随后显式调用 `get_inquiry`
- **THEN** phase 更新本身不新增模型消息，显式查询结果按普通工具结果进入调用方上下文，并且不触发目标重新采样。

#### Scenario: Replay does not inject inquiry content
- **WHEN** 已完成询问在任一端重连或冷恢复时重建显示
- **THEN** 显示恢复不重新发起询问，不向目标 foreground 注入问答，也不向来源重复追加工具结果。

#### Scenario: External coordination API caller
- **WHEN** 外部调用方通过协调 API 发起询问并取得响应
- **THEN** 响应返回该 API 调用方，来源 Session 不因此自动获得合成工具结果或用户消息。

现有证据入口：`crates/codegen/shell/src/session/actor/coordination.rs` — `handle_coordination_inquiry`、`delegated_inquiry_during_foreground_bypasses_peer_approval_without_injecting_input`；`crates/codegen/shell/src/session/actor/tool/result.rs` — `handle_bridge_tool_success`；`crates/codegen/shell/src/extensions/coordination.rs`。本要求明确现有边界。

### Requirement: Message acknowledgements retain durable receipt identity

消息发送 SHALL 仅在目标 durable Received 事实成立后返回 received，并保留原操作 ID、目标身份和真实 receipt ID。接收确认、拒绝和无法确认 SHALL 使用结构化结果区分。ACK SHALL NOT 等待模型回答、代表消费或任务完成；回复意见 SHALL 是独立消息。

#### Scenario: Durable acceptance
- **WHEN** 目标持久接收消息
- **THEN** source 收到该事实的真实 receipt ID，普通工具结果交付一次，UI 不产生第二份模型通知。

#### Scenario: Lost acknowledgement
- **WHEN** 已派发的操作无法确认是否完成 durable commit
- **THEN** 结果是 unconfirmed，不自动重发，不以错误字符串推断未接收；有预算时仅核验原 identity 的已有 receipt。

#### Scenario: Existing receipt after target becomes inactive
- **WHEN** 已授权调用方核验自己已发送的消息而目标已不接受新消息
- **THEN** 只读核验可以返回已存在的同一 receipt，不打开 writer、不恢复目标；新的消息仍受原 admission 限制。

#### Scenario: Repeated operation or conflict
- **WHEN** 同 identity 请求再次到达
- **THEN** 同正文和投递参数返回原 receipt 且不重复消费或 interrupt，冲突正文/模式/目标拒绝；新的 tool call 不按正文相同自动去重。

### Requirement: Opinion replies use explicit message delivery

agent 的正式意见交换 SHALL 使用消息接纳和安全步骤消费。每一端 SHALL 保留自己的发送调用与回执，以及对方的入站意见。沿已收到消息的 reply SHALL 由 runtime 解析并校验原参与方、关系与权限；reply SHALL NOT 被视为人类授权。Sideband 咨询答案 SHALL NOT 自动写入目标主上下文或成为其正式承诺。

#### Scenario: Reply to received guidance
- **WHEN** child 根据实际持久收到的父消息发出 reply
- **THEN** runtime 将其作为意见交付原 parent，并返回独立接收回执；reply 不能请求上行 interrupt 或扩大父任务权限。

#### Scenario: Invalid reply route
- **WHEN** 调用方猜测别人的消息 ID、指定冲突目标或转发到未授权第三方
- **THEN** 路由拒绝，不向其他 Session 写入上下文。

#### Scenario: Concurrent exchange
- **WHEN** 两方同时发出意见
- **THEN** 各自收到独立接收回执，按各自 durable receive order 消费；发送不等待对方生成回复，也不触发自动 ACK 循环。

#### Scenario: Sideband consultation remains separate
- **WHEN** ask 返回意见而调用方尚未显式发送该内容
- **THEN** 答案只作为调用方工具结果，目标主对话不追加该问答；调用方以后显式 send 引用时保留引用来源。

## MODIFIED Requirements

### Requirement: Parent-child inquiries use isolated Sideband execution
A running child SHALL be able to ask its immediate parent a question, and a primary agent SHALL be able to ask a directly-owned running child. Runtime-derived identities SHALL authorize the relationship. Questions SHALL use asynchronous tool-free Sideband execution without entering or interrupting the target foreground context. Children SHALL NOT gain cross-session discovery/inquiry or upward intervention; an explicitly authorized reply to a durably received message SHALL be treated as an attributed opinion, not upward intervention or human permission.

#### Scenario: Child clarification during parent work
- **WHEN** a child asks its parent while the parent foreground is busy or waiting for that child
- **THEN** a Sideband answers from frozen parent context, returns the answer to the child, and the main view shows one correlated inquiry row through receipt and completion.

#### Scenario: Parent asks child
- **WHEN** the parent asks its running child a question
- **THEN** the child answers via Sideband without changing its foreground task or requiring cross-workspace approval for its delegated worktree.

#### Scenario: Invalid route or cancellation
- **WHEN** a caller targets an unrelated, sibling, terminated or Workflow-owned hidden child, or cancels an in-flight inquiry
- **THEN** a typed local failure/cancellation is returned without injecting user input, leaking another session's context or pausing the Goal.

#### Scenario: Reply does not grant intervention authority
- **WHEN** child replies along an authorized message relationship
- **THEN** the parent receives an attributed opinion without granting the child interrupt authority, changing the inquiry route, or treating the opinion as a new user instruction.
