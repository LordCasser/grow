# local-coordination Specification

## Purpose
定义本机会话协调协议中可以识别和追踪的交互。覆盖 peer 握手字段、询问与取消的稳定标识，以及进度、结果、取消回执和错误的区分，不把协调通道等同于任意会话输入。

## Requirements

### Requirement: Peer handshake identity
本地协调连接 SHALL 使用包含 protocol version、peer identity、incarnation、bearer token 和 source session 的握手。

#### Scenario: 建立协调连接
- **WHEN** 客户端发送 ClientHello
- **THEN** 服务端通过 ServerHello 返回是否 accepted 及可能的错误。

证据：`crates/codegen/shell/src/coordination/protocol.rs` — `ClientHello`。

### Requirement: Inquiry scoped operations
协调协议 SHALL 以 inquiry_id 与 target_session_id 标识询问和取消，并区分进度、结果、取消回执与错误。

#### Scenario: 取消询问
- **WHEN** 客户端对已知 inquiry_id 发送 Cancel
- **THEN** 响应保留 inquiry_id 和 accepted 状态，不将取消回执伪装成询问答案。

证据：`crates/codegen/shell/src/coordination/protocol.rs` — `Request`。

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

### Requirement: Parent intervention has explicit delivery timing
A primary agent SHALL be able to send a message to a directly-owned running child with an explicit immediate-interrupt or queued-next-step option. The message SHALL be attributed to the parent and durably received before acknowledgement. Retries and restoration SHALL preserve exactly-once inbox consumption.

#### Scenario: Queued intervention
- **WHEN** the parent sends a non-interrupting message
- **THEN** the child completes its current work boundary and includes the message at the next step's sampling without cancelling the active request.

#### Scenario: Immediate intervention
- **WHEN** the parent requests immediate interruption
- **THEN** the child safely preempts the current model request or interruptible wait and resamples with the message, preserving non-interruptible tool outcomes, Timeline integrity and the current Goal lifecycle.

#### Scenario: Restore or retry
- **WHEN** an acknowledged intervention is retried or its receiving session restores
- **THEN** the same receipt is not duplicated and unconsumed context remains available without impersonating human input.

### Requirement: Windows peer publication coexists with manifest readers
Windows peer heartbeat publication SHALL atomically replace the private manifest while an already-open discovery reader retains the previous file. Owner-only access, no-reparse validation and long-path support SHALL remain enforced; publication failure SHALL preserve the prior manifest.

#### Scenario: Reader spans heartbeat replacement
- **WHEN** a discovery reader holds a peer manifest open while a new heartbeat is published
- **THEN** that reader can finish reading the previous complete manifest, new readers see the replacement, and publication does not first delete the destination or relax its ACL.

### Requirement: Inquiry presentation identifies the actual coordination participant

Receiving-side inquiry audits SHALL retain whether the inquiry came through peer coordination or direct parent-child delegation and SHALL identify the actual direction and counterpart. A parent receiving a child's inquiry SHALL identify the coordinator-owned subagent task name; a child receiving its parent's inquiry SHALL identify the parent and retain the participating child task in details. Peer coordination SHALL continue to identify the source session. Presentation SHALL derive identity from structured audit facts, never UUID shape, visible rows, or title prose.

#### Scenario: Subagent asks its parent
- **WHEN** a running subagent with task name `TS registry workload presentation` asks its immediate parent and the inquiry is answered
- **THEN** the receiving row identifies that subagent task through receipt and completion, rather than displaying only the child session id.

#### Scenario: Parent asks its subagent
- **WHEN** a primary agent asks a directly-owned subagent
- **THEN** the child-side receiving row identifies the parent as the source and keeps its own participating task identity in details, while the source tool remains the primary-side interaction surface.

#### Scenario: Another primary session asks
- **WHEN** an inquiry arrives through authenticated peer coordination
- **THEN** the receiving row uses `session <source session id>` as the counterpart and does not label the peer as a parent or subagent; a shortened display retains the full id in details.

#### Scenario: Reconnect or replay
- **WHEN** a delegation inquiry is replayed or its terminal update arrives after reconnect
- **THEN** persisted direction and participant identity update the same correlated row without reversing source and target, reverting to ambiguous session identity, or duplicating the inquiry.

#### Scenario: Foreground completes during sideband work
- **WHEN** the receiving foreground turn ends while an inquiry is active
- **THEN** the inquiry row continues until its own terminal outcome and is not completed by foreground tool cleanup.

### Requirement: Inquiry snapshots retain completed tool evidence

协调询问 SHALL 在冻结上下文中保留已经提交且可正确配对的工具调用身份、参数、结果、附件和 Assistant 正文，包括连续工具交换的尾部。未返回的调用 SHALL NOT 形成悬空工具协议或被伪造成已完成；同批已完成的交换 SHALL 保留。询问 SHALL 继续无工具执行，不修改目标主 Surface，也不将询问回答视为权限授予。

#### Scenario: Parent asks about a completed child inquiry
- **WHEN** 子 Agent 的冻结上下文包含已完成的 ask_parent，后面紧接其他完成的工具交换，parent 发起 ask_subagent
- **THEN** 回答请求仍包含 ask_parent 的问题、真实结果和后续已完成的工具证据，不因消息位于尾部而删除。

#### Scenario: Inquiry arrives during a partial tool batch
- **WHEN** 父子或 peer 询问冻结时，同一批工具仅部分返回或全部尚未返回
- **THEN** 回答请求保留已完成交换及 Assistant 正文，省略未完成的结构化调用，不产生虚构结果，主 Surface 保持原样。

证据入口：`crates/codegen/shell/src/session/actor/coordination.rs` — `handle_coordination_inquiry` 与真实 provider 请求回归。

### Requirement: Valid prior Sidebands do not disable later inquiries

A session whose prior Sideband selected valid canonical Surface coordinates SHALL remain able to durably receive and answer peer and direct parent-child inquiries. Foreground activity and active background subagents SHALL NOT be used as an inquiry admission gate.

#### Scenario: Inquiry after compaction while a child remains active

- **WHEN** a target completed compaction over consumed user or notification input, its foreground is idle or busy, and a background subagent remains active
- **THEN** a peer or directly related agent can durably record the receipt, obtain the tool-free Sideband answer and record the terminal audit without a Surface-selection `audit_failure`.

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
