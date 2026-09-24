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

### Requirement: Coordination guidance preserves audience ownership

Grow SHALL 为 primary 和 subagent 提供英文协作指引，分别保留整体交付责任与有界任务责任。指引 SHALL 服从现有用户约束、工具资格、active Behavior 和权限边界，不因委派或询问创建新的授权或会话所有权。

本组要求规范模型可见的协作指引及其可评估目标，不宣称 prompt 是确定性的依赖调度器、文件锁或强制行为执行器。

#### Scenario: Primary retains overall responsibility
- **WHEN** 为 primary session 生成协作指引
- **THEN** 指引要求其掌握任务级理解、核对影响全局的证据、协调委派成果并负责最终验收，不把整体问题与交付责任全部转交子任务。

#### Scenario: Child retains bounded responsibility
- **WHEN** 为 subagent session 生成协作指引
- **THEN** 指引允许其在范围和能力内自主调查、实现和验证，同时要求报告父级决策边界，不接管父会话或扩大权限。

#### Scenario: Role composition varies
- **WHEN** 同一 audience 使用 Extend 或 Full role composition，或切换具体 Agent role
- **THEN** 协作责任仍来自对应 audience，父级专属调度指引不作为所有 child 都必须遵循的共享 role 正文，动态任务状态不进入稳定 System head。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；`crates/codegen/agent/src/prompt/context.rs` 的 `render_with_renderer` / `render_role_with_renderer` 与现有装配测试。

### Requirement: Delegation guidance names sufficient inputs and acceptance

Grow SHALL 指导委派方提供与任务规模相称的目标、必要输入、待确认假设、产物、写入范围和验收条件，并以真正需要的事实、接口、决策或产物表达依赖。指引 SHALL 允许简单任务保持最小闭环，不要求固定 JSON、完整 DAG 或新增持久化任务记录。

#### Scenario: Consumer needs only a confirmed interface
- **WHEN** 一个任务实现所需的接口已确认，但接口生产方的其他工作仍在进行
- **THEN** 指引允许依据该已确认接口推进独立工作，不默认依赖生产方整个任务结束。

#### Scenario: Task is too small to benefit from delegation
- **WHEN** 一个任务可直接完成，拆分会增加协调和整合成本
- **THEN** 指引允许直接完成，不为了并发数量强制创建 subagent。

#### Scenario: Task-specific context is required
- **WHEN** 委派任务依赖父 Agent 尚未写明的决定或证据
- **THEN** task 工具说明要求显式提供这些信息，不假定 child 自动知道父级计划，也不以统一的压缩规则描述所有 host 的项目规则传递。

证据与实施入口：`crates/codegen/agent/prompts/audience/primary.md`；`crates/common/tool-types/src/task.rs::build_task_description`；`crates/codegen/agent/src/prompt/context.rs::agents_md_user_reminder`。

### Requirement: Coordination guidance distinguishes exploration execution and acceptance

Grow SHALL 指导 Agent 区分已确认事实、工作假设、候选产物和已接受成果，并分别判断是否可以探索、实现或验收采用。未知条件 SHALL 只阻塞真正依赖它的工作；基于假设的准备 SHALL 有界且可放弃，不将该假设用于修改正式共享状态或产生相应外部副作用。

#### Scenario: Design input is unresolved
- **WHEN** 实现依赖的接口或决策尚未确定，但相关机制可以独立调查
- **THEN** 指引允许限定范围的调查或验收准备，并要求实现等待所需输入，不把预测当作已确认契约。

#### Scenario: Candidate result has returned
- **WHEN** 子任务返回代码或结论
- **THEN** 指引要求核对适用前提和必要验证后才采用，不将 runtime completed 自动等同于语义验收。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；验收采用是任务上下文判断，不新增 task status。

### Requirement: Primary guidance advances independently ready work

Grow SHALL 指导 primary 在任务启动、结果返回、出现阻塞或关键前提改变后的自然检查点重新选择工作，优先处理交付瓶颈和能解锁后续工作的成果。等待 SHALL 基于实际依赖和现有工具语义；指引不得要求忙轮询、重复已委派工作或通过扩大范围保持忙碌。

#### Scenario: One result arrives before an unrelated task
- **WHEN** A 的结果已返回且可以解锁下一项工作，B 仍运行但与该工作无关
- **THEN** 指引要求及时检查 A 并推进被它解锁的工作，不默认等待 B。

#### Scenario: Useful independent work remains
- **WHEN** child 正在后台执行，parent 还有不重复且不依赖该 child 结果的有价值工作
- **THEN** 指引要求 parent 继续推进该工作，并在自然检查点处理结果。

#### Scenario: No useful independent work remains
- **WHEN** 下一步所需结果尚未到达且没有值得执行的独立工作
- **THEN** 指引允许使用真实等待机制，不制造任务或反复请求无变化快照。

#### Scenario: Multiple task wait is selected
- **WHEN** 工具说明介绍多 task ID 配合正超时的等待
- **THEN** 保持 wait-all 说明，并指导仅在下一步需要全部结果时使用该方式，不把它描述成 wait-any。

证据与实施入口：`crates/codegen/agent/prompts/audience/primary.md`；`crates/common/tool-types/src/task.rs::build_task_output_description`；既有运行时 `crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs::wait_all_event_driven` 不修改。

### Requirement: Coordination guidance bounds concurrency and write ownership

Grow SHALL 指导主从双方尊重明确的写入范围，并依据独立就绪工作、资源约束和验收整合能力决定并发。工作区隔离 SHALL NOT 在指引中被描述为已经消除接口与共享假设的语义依赖。

#### Scenario: Results accumulate faster than review
- **WHEN** 待验收成果积压、共享修改冲突增加或资源竞争拖慢关键任务
- **THEN** 指引要求优先验收、整合或解除冲突，减少新任务派发。

#### Scenario: Parent would overlap an active writer
- **WHEN** primary 想接管仍由 child 负责的修改范围
- **THEN** 指引要求先确认安全交接，不将消息接收回执当作 writer 已停止或写入权已释放。

#### Scenario: Shared premise changes
- **WHEN** 接口或关键输入变化使部分在途工作或已有成果的前提失效
- **THEN** 指引要求识别并停止受影响工作、重新判断相关产物，允许不受影响的工作继续，不自动丢弃其他执行者的修改。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；现有隔离和干预语义来自 `crates/common/tool-types/src/task.rs::TaskToolInput` 与 `crates/codegen/tools/src/implementations/grow_build/task/interaction.rs`。

### Requirement: Child guidance respects inquiry and delivery boundaries

Grow SHALL 指导 child 按真实能力进行澄清和成果交付，不将 frozen-context inquiry 回答当作目标前台已采取行动、权限扩大或新验证完成。没有可用的主动中间成果发布通道时，指引 SHALL 允许通过有价值的阶段性任务结果交付，不假设不存在的通道。

#### Scenario: Clarification cannot perform required parent action
- **WHEN** child 的继续工作需要 parent 调查、重排计划或执行操作，而现有 Sideband 澄清无法提供该行动
- **THEN** 指引要求返回已完成部分、证据和具体阻塞，不无限等待 parent，同时不自行越过范围或权限。

#### Scenario: Intermediate evidence is queried
- **WHEN** parent 通过 inquiry 获取 child 冻结上下文中已存在的事实
- **THEN** 指引要求按快照范围使用该信息，不声称 inquiry 执行了新的工具验证或提交了整体计划。

证据与实施入口：`crates/codegen/agent/prompts/audience/subagent.md`；`crates/codegen/tools/src/implementations/grow_build/task/interaction.rs` 中已有 ask / send 工具描述保持。

### Requirement: Delegated results carry evidence and bounded conclusions

Grow SHALL 指导 subagent 返回自然语言的完成程度、可定位产物、关键输入前提、实际执行的验证及结果、未验证部分和待决问题。primary 指引 SHALL 要求按风险验收实际产物，并完成必要的跨模块验证，不无差别重做所有局部工作。

#### Scenario: Local tests pass but integration remains
- **WHEN** child 完成局部实现和相关测试
- **THEN** 指引要求区分局部验证与整体集成，返回剩余集成条件，不宣称整个父任务已完成。

#### Scenario: Exploration reaches sufficient evidence
- **WHEN** explore child 已取得足以回答委派问题的证据
- **THEN** role 指引要求停止扩展调查，返回事实、路径、不确定性和覆盖边界，负面结论限定在实际检查范围。

#### Scenario: Returned assumptions become stale
- **WHEN** parent 在验收时发现影响结论的输入已经变化
- **THEN** 指引要求重新检查受影响结论和产物，不以旧报告的成功措辞替代当前验证。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；`crates/codegen/agent/prompts/agents/{general-purpose,explore}.md`。模型实际遵循程度另按 change 的场景评估记录，不由字符串测试推导。
