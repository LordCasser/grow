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
协调询问 SHALL 以 inquiry_id 与 target_session_id 标识询问和取消，并区分进度、结果、取消回执与错误。模型 `ask_session` 工具入口将缺失、null、空或仅含空白字符的 inquiry_id 视为未提供，由 backend 为新询问生成 UUIDv7；非空值原样按 UUIDv7 校验。外部协调协议/ACP 的显式 inquiry_id 和 `get_inquiry` SHALL 保持严格校验。

#### Scenario: Model tool creates an inquiry without a usable supplied ID
- **WHEN** 模型调用 `ask_session` 时 inquiry_id 被省略、为 null、空字符串或仅含空白字符
- **THEN** 工具将该值作为未提供 ID 处理，由 backend 为新询问生成 UUIDv7。

#### Scenario: Retry reuses a valid inquiry ID
- **WHEN** 模型以有效 UUIDv7 inquiry_id 重试相同询问
- **THEN** 工具原样传递该 ID，使 runtime 按同一询问身份处理重试。

#### Scenario: Invalid nonblank model ID is rejected
- **WHEN** 模型提供非空白但非 UUIDv7 的 inquiry_id，包括周围带空格的 UUID 字符串
- **THEN** 工具原样传递该值并由严格校验拒绝，不静默修剪或生成替代 ID。

#### Scenario: External protocol rejects an explicit blank ID
- **WHEN** 外部协调协议/ACP 调用方显式提交空 inquiry_id
- **THEN** 请求继续按严格 ID 校验拒绝，不触发新 ID 生成。

#### Scenario: 取消询问
- **WHEN** 客户端对已知 inquiry_id 发送 Cancel，并指定 target_session_id
- **THEN** 响应保留 inquiry_id 和 accepted 状态，不将取消回执伪装成询问答案。

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
协调询问 SHALL 在冻结上下文中保留已经提交且可正确配对的工具调用身份、参数、结果、附件和 Assistant 正文，包括连续工具交换的尾部。未返回的调用 SHALL NOT 形成悬空工具协议或被伪造成已完成；同批已完成的交换 SHALL 保留。询问 SHALL 继续无工具执行，生成期间不修改目标主 Surface；成功问答随后按被动通知消费进入目标上下文，不构成权限授予。

#### Scenario: Parent asks about a completed child inquiry
- **WHEN** 子 Agent 的冻结上下文包含已完成的 ask_parent，后面紧接其他完成的工具交换，parent 发起 ask_subagent
- **THEN** 回答请求仍包含 ask_parent 的问题、真实结果和后续已完成的工具证据，不因消息位于尾部而删除。

#### Scenario: Inquiry arrives during a partial tool batch
- **WHEN** 父子或 peer 询问冻结时，同一批工具仅部分返回或全部尚未返回
- **THEN** 回答请求保留已完成交换及 Assistant 正文，省略未完成的结构化调用，不产生虚构结果；冻结和生成不修改目标主 Surface，后续被动同步不拆开主任务的工具批次。

### Requirement: Valid prior Sidebands do not disable later inquiries

A session whose prior Sideband selected valid canonical Surface coordinates SHALL remain able to durably receive and answer peer and direct parent-child inquiries. Foreground activity and active background subagents SHALL NOT be used as an inquiry admission gate.

#### Scenario: Inquiry after compaction while a child remains active

- **WHEN** a target completed compaction over consumed user or notification input, its foreground is idle or busy, and a background subagent remains active
- **THEN** a peer or directly related agent can durably record the receipt, obtain the tool-free Sideband answer and record the terminal audit without a Surface-selection `audit_failure`.

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

Grow SHALL 指导 primary 在任务启动、结果返回、出现阻塞或关键前提改变后的自然检查点重新选择工作，优先处理交付瓶颈和能解锁后续工作的成果。等待 SHALL 基于实际依赖和现有工具语义；指引不得要求忙轮询、重复已委派工作或通过扩大范围保持忙碌。独立工作结束且下一步依赖已派发子任务时，指引 SHALL 明确使用专用 wait_subagent 对原 subagent ID 发起有界阻塞等待，不通过另起 sleep 命令、轮询循环或额外任务制造等待。等待返回 SHALL NOT 被指引视为子任务完成或成果验收。

具名工具的枚举、详情和等待指引 SHALL 在既有动态 Agent 层按 finalized 工具名渲染；稳定 system head SHALL 与工具集合无关。缺失能力 SHALL 不生成不存在的工具指令；Full 与 Extend SHALL 保留适用的 session extensions。

#### Scenario: One result arrives before an unrelated task
- **WHEN** A 的结果已返回且可以解锁下一项工作，B 仍运行但与该工作无关
- **THEN** 指引要求及时检查 A 并推进被它解锁的工作，不默认等待 B。

#### Scenario: Useful independent work remains
- **WHEN** child 正在后台执行，parent 还有不重复且不依赖该 child 结果的有价值工作
- **THEN** 指引要求 parent 继续推进该工作，并在自然检查点处理结果。

#### Scenario: No useful independent work remains
- **WHEN** 下一步所需结果尚未到达且没有值得执行的独立工作
- **THEN** 指引要求通过专用 wait_subagent 对所需的原 subagent ID 发起有界阻塞等待，不另起 sleep 命令、轮询循环或额外任务，也不反复请求无变化快照。

#### Scenario: Multiple task wait is selected
- **WHEN** 工具说明介绍多 subagent ID 配合等待预算
- **THEN** 明确任一目标进入终态即返回，并要求按 subagent ID 分析该目标的状态与结果，及时推进其解锁的工作；仅在后续步骤仍依赖剩余目标时对剩余 ID 继续等待，不因同批尚未完成而推迟分析，也不把失败或取消当作成功验收。

#### Scenario: Wait returns without a successful child result
- **WHEN** 等待到期、被用户消息打断，或返回失败、取消、仍运行或观察未知
- **THEN** 指引要求按真实状态决定下一步，不将本次等待结束等同于 child 完成或结果已验收，不另起占位任务替代等待。

#### Scenario: Production prompt assembly uses resolved observation capabilities
- **WHEN** AgentBuilder 生成 primary 或可委派 child 的实际 prompt，工具名称被重命名或部分观察能力不可用
- **THEN** 动态 Agent 层只引用实际可用名称，稳定 head 保持不变，默认具备 list/status/wait 时三类指引均到达模型上下文。

#### Scenario: Full role composition retains observation guidance
- **WHEN** Agent 使用 Full 而非 Extend 组合
- **THEN** 适用的观察与等待指引继续在 session extensions 中渲染，不被 authored role 替代，audience 与 child 能力边界保持。

证据与实施入口：`crates/codegen/agent/prompts/audience/primary.md`、`crates/codegen/agent/prompts/extensions/session.md`；`crates/codegen/agent/src/prompt/context.rs::{render,render_role_with_renderer}`；`crates/common/tool-types/src/task.rs::build_task_description`；既有等待入口为 `crates/codegen/tools/src/implementations/grow_build/subagent/wait.rs`。模型遵循效果需实际模型场景验证，静态文案和渲染测试不构成遵循保证。

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

### Requirement: Inquiry identity and replay are durable across session recovery
协调询问 SHALL 以 `(source_session_id, inquiry_id)` 作为逻辑身份；target session 由接收作用域隐含。peer identity 与 incarnation SHALL 用于连接认证、租约及审计归因，不得替代 source session 成为逻辑询问身份。target Timeline SHALL 防止相同逻辑身份的询问重复执行；source Timeline SHALL 固定首次启动时的目标和问题，并使重试、恢复和响应关联服从该绑定。

#### Scenario: UUID text aliases cannot create another identity
- **WHEN** 调用方将现有 UUIDv7 改写为大写、无连字符、URN 或带括号的形式来询问、查询或取消
- **THEN** 请求因不符合 runtime 生成的小写连字符标准形式而被拒绝，不按另一字符串键创建新询问。

#### Scenario: Source reconnects with a new peer incarnation
- **WHEN** 同一 source session 在 peer incarnation 变化后以相同 inquiry_id 重连并重试相同询问
- **THEN** target 将其识别为同一逻辑询问并复用已有 receipt/outcome，不因 incarnation 变化再次采样。

#### Scenario: Different source sessions use the same inquiry ID
- **WHEN** 两个不同 source session 对同一 target 使用相同 inquiry_id
- **THEN** target 将它们作为两个独立询问处理，各自保持独立 receipt 与 outcome。

#### Scenario: Target replays a completed inquiry
- **WHEN** target 收到已有 canonical Timeline 终态的相同 source/inquiry identity 与相同 payload
- **THEN** target 复用已记录 outcome，不再次执行模型询问。

#### Scenario: Target receives an inquiry with an open receipt
- **WHEN** target 对相同 source/inquiry identity 已有未终结 receipt
- **THEN** target 拒绝重复执行，不写第二份 receipt 或启动第二次 Sideband。

#### Scenario: Target restores an open inquiry receipt
- **WHEN** target 冷恢复时发现已有 open receipt 且尚无终态
- **THEN** target 将该 receipt 收束为 `TargetRestarted`，同身份重试复用该终态且不重新采样。

#### Scenario: Source retries the original operation
- **WHEN** source 对已有 `OutgoingStarted` 身份使用相同 target 和 question 重试
- **THEN** source 沿原询问 identity 查询或复用已有结果，不创建新操作。

#### Scenario: Source retries with a changed target or question
- **WHEN** source 对已有 `OutgoingStarted` identity 更换 target 或 question
- **THEN** source 拒绝冲突，不覆盖首次绑定或 dispatch 新询问。

#### Scenario: Source restores an inquiry without a terminal outcome
- **WHEN** source 恢复时发现询问已启动但没有完成 outcome
- **THEN** 查询返回 `SourceUnavailable`，不自动执行或重发询问；相同 target/question 的显式重试仍受 target durable 去重约束。

#### Scenario: Progress or outcome carries a mismatched inquiry ID
- **WHEN** 当前询问收到 inquiry_id 与请求身份不符的 progress 或 outcome
- **THEN** 调用方返回结构化关联失败，不把该响应归入当前询问，也不自动重发。

#### Scenario: Cancel response carries a mismatched inquiry ID
- **WHEN** 当前取消请求收到 inquiry_id 与请求身份不符的响应
- **THEN** 调用方返回结构化关联失败，不将其他询问视为已取消或接受取消。

#### Scenario: Cancellation wins before dispatch
- **WHEN** source 在询问 dispatch 前已为同一逻辑身份登记取消
- **THEN** 询问不被派发，取消结果按该身份返回。

#### Scenario: Terminal remote inquiry is cancelled
- **WHEN** source 对已终结的远端询问发送取消
- **THEN** target 返回 `accepted=false`，不改变已有 outcome。

#### Scenario: Cancellation is scoped to its source and target
- **WHEN** 调用方尝试以错误 source 或 target scope 取消询问
- **THEN** 请求拒绝且不影响其他询问的执行或状态。

### Requirement: Source inquiry startup admission is idempotent
Source SessionActor SHALL serialize outgoing inquiry startup admission against its canonical Timeline. For a `(source_session_id, inquiry_id)` already bound to a target and exact question, a retry SHALL return the existing admission without appending another `OutgoingStarted` fact or projecting another startup notice. A retry with a changed target or question SHALL return `Conflict` without mutation or dispatch. Question text SHALL only be compared as payload under the explicit inquiry identity and SHALL NOT serve as a deduplication key.

#### Scenario: Source retries an admitted startup
- **WHEN** 同一 source session 对已有 `OutgoingStarted` identity 以相同 target 和 exact question 重试，包括首次持久化 ACK 不明后的重试
- **THEN** actor 从 canonical Timeline 确认原绑定并返回已接纳，不追加 `OutgoingStarted` 或重复投影启动通知。

#### Scenario: Source retries with a changed target or question
- **WHEN** 同一 source session 对已有 identity 使用不同 target 或 exact question 重试
- **THEN** actor 返回 `Conflict`，不覆盖首次绑定、不追加事实或通知，也不 dispatch 冲突请求。

### Requirement: Live source inquiry startup and queries share an admission boundary

source SHALL 使首次 durable startup admission、runtime 运行记录登记与同源 inquiry 查询一致可观察。查询跨越启动提交时 SHALL 等待接纳结果或返回真实的当前运行状态，不因暂未登记 runtime 记录而把活启动误判为重启遗留。目标派发 SHALL 发生在 source durable ACK 后；接纳失败 SHALL 不派发、不写入虚构完成事实。查询 SHALL 不触发执行或自动重发。

#### Scenario: Query overlaps a durable startup acknowledgement
- **WHEN** source 的 `OutgoingStarted` 已落盘，接纳 ACK 仍被等待，并发查询同一 inquiry
- **THEN** 查询返回当前启动状态，不返回冷恢复专用的 `SourceUnavailable`，目标在 ACK 前没有收到询问。

#### Scenario: Startup admission fails or caller cancels
- **WHEN** source 接纳返回失败，或调用方在接纳期间取消
- **THEN** 失败不派发且释放接纳边界；取消成功接纳后的询问沿原取消身份终结，不派发到目标，不将查询视为重试。

#### Scenario: Cold source has only a historical start
- **WHEN** 新 runtime 只有重启前的 `OutgoingStarted` 且没有终态或当前运行记录
- **THEN** 查询继续返回 `SourceUnavailable`，不重跑；原 payload 的显式重试与冲突拒绝仍遵守 durable 身份约束。

证据与实现入口：`crates/codegen/shell/src/coordination/runtime.rs` 的 `ask_session_with_admission` / `get_inquiry`，`crates/codegen/shell/src/agent/mvp_agent/coordination.rs` 的 `ask_with_id`；回归见 runtime identity tests 和 `scripts/test_local_coordination.py`。

### Requirement: Background subagent receipts expose the existing result wait

Grow SHALL 在后台子任务启动回执中指引 parent 继续有价值的独立工作，并在需要原 child 结果时使用既有输出工具等待，不创建 sleep 命令、轮询循环或额外等待任务。有正等待额度时，回执 SHALL 提供原 child ID、实际工具名、实际参数名与当前等待上限内的 JSON 调用示例。树恢复重建该回执时 SHALL 采用当前工具映射，不另派 child 或改变身份。

#### Scenario: Default or renamed output tool
- **WHEN** 后台 task 回执生成且结果工具或参数使用默认名或重命名
- **THEN** 示例引用当前输出工具及其 task IDs、timeout 参数名，JSON 保留原子任务 ID，等待值为正且不超过当前上限。

#### Scenario: Blocking waits have no allowance
- **WHEN** 当前单次等待上限为零
- **THEN** 回执不提供被称为阻塞等待的零超时示例，原子任务身份和结果读取入口继续保留。

#### Scenario: Restored background receipt
- **WHEN** 树恢复需要补交原后台子任务的启动回执
- **THEN** 回执使用原 child ID 和恢复后当前输出工具映射生成指引与示例，不把恢复当成完成或新派发。

### Requirement: Inquiry snapshots reuse durable image descriptions

Parent-child and peer inquiries SHALL bind client, request model and image capability to one prepared receiver route. They SHALL freeze acknowledged context after cancellably waiting for primary image conversion, prefer all available descriptions and release the projection gate before answer sampling. Known rejecting routes SHALL recover selected unresolved images before provider admission. Explicit image rejection on an unknown route SHALL record that actual route's negative capability and permit one neutral text recovery attempt. Recovery SHALL use only the original frozen source, SHALL NOT mutate the primary Surface, and SHALL fail if descriptions are unavailable or the admitted budget is exceeded. Ordinary errors SHALL NOT trigger image recovery. Inquiry and session cancellation SHALL cover recovery and retry.

#### Scenario: Switch from a multimodal model to a text model
- **WHEN** a child or peer inquires after the receiver switches to text with unresolved historical images
- **THEN** the inquiry recovers neutral descriptions before admission on a known route or once after explicit rejection on an unknown route, then answers without raw images.

#### Scenario: Inquiry is cancelled while waiting
- **WHEN** inquiry or session cancellation occurs during projection waiting or recovery
- **THEN** no further answer request is emitted and the inquiry terminates as cancelled.

#### Scenario: Context advances during recovery
- **WHEN** new input reaches the receiver while its frozen inquiry recovers images
- **THEN** its answer excludes the new input and recovery leaves the primary Surface intact.

#### Scenario: Conversion awaits durable acknowledgement
- **WHEN** the primary image projection awaits its durable acknowledgement
- **THEN** inquiry capture waits and uses the acknowledged descriptions without holding the gate during answer sampling.

#### Scenario: Text-only route lacks a committed description
- **WHEN** the prepared receiver route is known text-only and selected images are unresolved
- **THEN** neutral recovery precedes answer admission and missing descriptions fail without primary Surface mutation.

#### Scenario: Unknown transport rejects unresolved images
- **WHEN** an unknown route explicitly rejects the image-bearing inquiry
- **THEN** that actual route is learned and one text recovery retry uses the original frozen sources.

### Requirement: Peer success fits its existing result transport
Peer inquiry success SHALL be admitted only when its actual serialized Inquiry response fits the existing local IPC frame limit, in addition to the rendered consultation payload limit. This check SHALL occur before successful terminal admission/proof and use the same encoder as the wire writer. An oversized response SHALL produce a typed failure retaining completed Sideband evidence and usage, without truncation, a new cap or automatic retry. Cold proof validation SHALL reject an impossible Peer success even if a receipt is already present. Direct delegation does not acquire the Peer transport constraint.

#### Scenario: JSON escaping exceeds the Peer result frame
- **WHEN** a completed answer fits the notification byte limit but its actual JSON Inquiry response exceeds the IPC frame limit due to escaping or response overhead
- **THEN** the Peer inquiry fails before success admission and creates no success proof or context receipt; the same answer may succeed through direct delegation when its own delivery limit fits.

### Requirement: Model session discovery lists online primary peers

主 Agent 的 `list_active_sessions` SHALL 只枚举同一 OS 用户和 GROW_HOME 下协调运行时可发现的其他在线 primary session。

#### Scenario: Primary session discovery with default arguments
- **WHEN** 主 Agent 调用 `list_active_sessions({})`
- **THEN** 返回当前可发现的其他在线 primary session，不枚举磁盘历史或 child session。
