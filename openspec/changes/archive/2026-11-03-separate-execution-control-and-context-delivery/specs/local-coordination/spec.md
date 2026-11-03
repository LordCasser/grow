## MODIFIED Requirements

### Requirement: Parent-child inquiries use isolated Sideband execution
A running child SHALL be able to ask its immediate parent a question, and a primary agent SHALL be able to ask a directly-owned running child. Runtime-derived identities SHALL authorize the relationship. Questions SHALL execute asynchronously in a tool-free Sideband from frozen context without interrupting target foreground execution. After a successful answer, the target SHALL retain attributed consultation evidence for passive delivery at an authorized context boundary. Children SHALL NOT gain cross-session discovery/inquiry or upward intervention; neither consultation evidence nor an explicitly authorized message reply SHALL become human permission.

#### Scenario: Child clarification during parent work
- **WHEN** a child asks its parent while the parent foreground is busy or waiting for that child
- **THEN** a Sideband answers from frozen parent context, returns the answer to the child and updates one correlated inquiry row, while its completed question and answer become passive evidence for the parent's next authorized context boundary.

#### Scenario: Parent asks child
- **WHEN** a parent asks its directly-owned running child a question
- **THEN** the child answers via Sideband without interrupting its foreground task or requiring cross-workspace approval for its delegated worktree; successful consultation evidence is passively synchronized to the child.

#### Scenario: Invalid route or cancellation
- **WHEN** a caller targets an unrelated, sibling, terminated or Workflow-owned hidden child, or inquiry-specific cancellation wins before successful-terminal admission
- **THEN** a typed local failure/cancellation is returned without injecting a successful consultation, leaking another Session's context or pausing the Goal.

#### Scenario: Cancellation wins while completed answer preparation is awaiting
- **WHEN** the provider has returned but inquiry-specific cancellation wins before the target admits its successful canonical terminal, including while usage or Sideband persistence is awaiting
- **THEN** the target records a typed cancellation without a success proof or consultation receipt; already committed Sideband evidence and usage are retained.

#### Scenario: Successful-terminal admission wins before its durable acknowledgement
- **WHEN** the target admits a successful canonical terminal under its existing serialized boundary before inquiry-specific cancellation, and persistence acknowledgement is still pending
- **THEN** cancellation cannot reverse that admitted outcome; the original response reports its actual persistence-confirmed outcome or failure, and only durable success creates the passive synchronization obligation.

#### Scenario: Peer cancellation preserves the original dispatched inquiry exchange
- **WHEN** a peer inquiry is already dispatched and the source accepts a cancellation intent
- **THEN** after matching target progress confirms registration the source requests cancellation once on the pinned target while preserving the original inquiry read exchange, and derives the final inquiry outcome from that exchange or its actual transport/deadline failure; local intent acceptance is not proof that target cancellation won.

#### Scenario: Target shutdown settles already owned completed Sideband work
- **WHEN** target shutdown starts after the Sideband Completed End is durable and the inquiry still owns its existing queue activity
- **THEN** that owner may settle its canonical terminal before the shutdown writer barrier; this does not reopen execution admission or allow a new receipt while execution is closed.

#### Scenario: Reply does not grant intervention authority
- **WHEN** a child replies along an authorized message relationship or a parent consultation answer is synchronized
- **THEN** the target receives attributed opinion/evidence without granting the child interrupt authority, changing the inquiry route or creating a new user instruction.

### Requirement: Inquiry snapshots retain completed tool evidence
协调询问 SHALL 在冻结上下文中保留已经提交且可正确配对的工具调用身份、参数、结果、附件和 Assistant 正文，包括连续工具交换的尾部。未返回的调用 SHALL NOT 形成悬空工具协议或被伪造成已完成；同批已完成的交换 SHALL 保留。询问 SHALL 继续无工具执行，生成期间不修改目标主 Surface；成功问答随后按被动通知消费进入目标上下文，不构成权限授予。

#### Scenario: Parent asks about a completed child inquiry
- **WHEN** 子 Agent 的冻结上下文包含已完成的 ask_parent，后面紧接其他完成的工具交换，parent 发起 ask_subagent
- **THEN** 回答请求仍包含 ask_parent 的问题、真实结果和后续已完成的工具证据，不因消息位于尾部而删除。

#### Scenario: Inquiry arrives during a partial tool batch
- **WHEN** 父子或 peer 询问冻结时，同一批工具仅部分返回或全部尚未返回
- **THEN** 回答请求保留已完成交换及 Assistant 正文，省略未完成的结构化调用，不产生虚构结果；冻结和生成不修改目标主 Surface，后续被动同步不拆开主任务的工具批次。

### Requirement: Inquiry context visibility is scoped to each participant
通过 agent 工具发起的 peer 或直接父子询问 SHALL 将问题保留为调用方的工具调用，将回答或失败保留为调用方的工具结果。目标 SHALL 在独立、无工具的冻结上下文询问中生成回答，并在成功 terminal 持久化后通过 context-only 通知将完整问答与真实来源纳入自己的后续获准采样。此同步 SHALL NOT 抢占执行、主动开启空闲 Turn、伪装成前台 Assistant 发言或人类授权。协调 API 的响应 SHALL 交付其调用者，不自动构造来源 Session 的模型输入。

#### Scenario: Answer reaches the asking agent
- **WHEN** agent 调用 ask_session、ask_parent 或 ask_subagent 并成功获得回答
- **THEN** 调用方后续采样通过原工具结果看到回答，目标通过恰好一次的被动消费看到完整问答、Sideband 来源和快照依据；两端不重复追加来源工具结果。

#### Scenario: Query and passive progress have different visibility
- **WHEN** 调用方收到被动 inquiry phase 更新，随后显式调用 get_inquiry
- **THEN** phase 更新本身不新增模型消息，显式查询结果按普通工具结果进入调用方上下文，不触发目标重新执行询问或重复接纳问答。

#### Scenario: Replay does not inject inquiry content
- **WHEN** 已完成询问在任一端重连或 UI 回放时重建显示
- **THEN** 显示恢复不重新发起询问或派发模型输入；runtime 冷恢复仅按 canonical 成功事实与消费状态恢复尚未完成的被动同步，不重复已消费问答或来源工具结果。

#### Scenario: External coordination API caller
- **WHEN** 外部调用方通过协调 API 发起询问并取得成功响应
- **THEN** 响应返回 API 调用方，来源 Session 不自动获得合成工具结果或用户消息；回答方仍按相同成功事实生成被动咨询证据。

#### Scenario: Inquiry does not succeed
- **WHEN** 询问被拒绝、失败、超时或取消且没有成功 terminal
- **THEN** 保留原审计和工具错误，但不生成包含成功问答的上下文通知。

#### Scenario: Complete consultation cannot fit its existing delivery limit
- **WHEN** a valid Sideband produces an answer whose fully rendered attributed question/answer exceeds the existing notification payload byte limit
- **THEN** the inquiry returns a typed failure before publishing its successful incoming terminal/proof, retains the completed Sideband evidence and usage, and creates no impossible synchronization obligation; it does not truncate the answer or automatically retry.

### Requirement: Opinion replies use explicit message delivery
agent 的正式意见交换 SHALL 使用消息接纳和安全步骤消费。每一端 SHALL 保留自己的发送调用与回执，以及对方的入站意见。沿已收到消息的 reply SHALL 由 runtime 解析并校验原参与方、关系与权限；reply SHALL NOT 被视为人类授权。成功 Sideband 咨询可自动作为有归属的事实进入回答方上下文，但 SHALL NOT 因此成为前台正式承诺、显式发送的回复或新的执行授权。

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
- **WHEN** ask 返回意见且调用方尚未显式发送该内容
- **THEN** 原回答保留为调用方工具结果和回答方被动咨询事实，不冒充一次显式 send/reply；以后显式 send 仍是带有自身身份和来源引用的独立操作。

## ADDED Requirements

### Requirement: Peer success fits its existing result transport
Peer inquiry success SHALL be admitted only when its actual serialized Inquiry response fits the existing local IPC frame limit, in addition to the rendered consultation payload limit. This check SHALL occur before successful terminal admission/proof and use the same encoder as the wire writer. An oversized response SHALL produce a typed failure retaining completed Sideband evidence and usage, without truncation, a new cap or automatic retry. Cold proof validation SHALL reject an impossible Peer success even if a receipt is already present. Direct delegation does not acquire the Peer transport constraint.

#### Scenario: JSON escaping exceeds the Peer result frame
- **WHEN** a completed answer fits the notification byte limit but its actual JSON Inquiry response exceeds the IPC frame limit due to escaping or response overhead
- **THEN** the Peer inquiry fails before success admission and creates no success proof or context receipt; the same answer may succeed through direct delegation when its own delivery limit fits.
