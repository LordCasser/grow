# session-timeline Specification

## Purpose
定义会话事实如何持久化、恢复并投影为模型上下文。覆盖完整消息写入的确认边界、事件序号延续及工具协议修复时的证据保留，供修改 ChatStateActor 与恢复路径时核对。

## Requirements

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

#### Scenario: Completed results surround model skill metadata
- **WHEN** 同批工具结果被其匹配的 Model SkillLoad 元数据隔开
- **THEN** 当前 repair 仅稳定归整元数据与结果，不删除真实成功正文或将它替换为取消/未执行；归整本身持久化且重放一致，再次修复幂等。

#### Scenario: Missing results and real message boundaries remain distinct
- **WHEN** 结果确实缺失、重复，或被真实用户消息/不匹配元数据隔开
- **THEN** 仍执行原配对规则，不跨真实边界伪造完整交换；已有已提交 IntegrityRepair 精确证明保持可验证，不重写旧事件。

### Requirement: Control context identity survives activation and replay
Timeline SHALL 为一次控制事件中的每条模型上下文保留独立且稳定的身份，立即投影、延迟激活与恢复重放一致。

#### Scenario: 空会话恢复时重建上下文
- **WHEN** 会话包含一次控制事件的多条上下文，恢复需要补入项目指令
- **THEN** 完整上下文替换可持久化，保留的事实可再次重放，不能因重复身份报 shadow set 不完整。

#### Scenario: 延迟激活只保留部分上下文
- **WHEN** 同一事件不同层的上下文在 step 或 turn 边界激活，或同层被后续控制取代
- **THEN** 每个激活项保留原事件内身份，模型 Surface 与 branch 投影保持一致。

证据范围：`crates/codegen/chat-state/src/timeline.rs`。

### Requirement: Fallback session titles fit canonical title bounds
Fallback session titles SHALL be non-empty and at most 160 Unicode scalar values after whitespace normalization. They SHALL retain the existing first-ten-word selection and use the default title for empty source text.

#### Scenario: Long unbroken user text
- **WHEN** title generation fails and the user text contains a long URL, CJK paragraph or other long word
- **THEN** the fallback is truncated safely to fit the canonical title character limit

#### Scenario: Ordinary or empty source text
- **WHEN** fallback source text is short or empty
- **THEN** existing first-ten-word or default-title behavior is retained

### Requirement: Automatic title provenance identifies admitted user input
Automatic session-title Sidebands SHALL identify the durable user input whose text generated the title. Later notification or control events SHALL NOT replace that source identity merely by becoming the Timeline tail.

#### Scenario: Notifications arrive before title scheduling
- **WHEN** a user input commits and notification events append before title generation is scheduled
- **THEN** the title Sideband source identifies the admitted user input rather than the notification event

#### Scenario: Input identity cannot be proven
- **WHEN** title scheduling cannot establish the admitted user input identity
- **THEN** it does not start title generation with an unrelated Timeline tail reference

#### Scenario: Direct terminal command input
- **WHEN** a direct command input commits without a prompt index
- **THEN** its title scheduling uses the event returned by the durable user-message commit, which is acknowledged only after persistence succeeds

### Requirement: Manual title route revocation survives background failure
Revoking automatic title generation for a manual title command SHALL remain effective when an already-running title worker later fails. Such a worker SHALL NOT re-enable automatic generation for subsequent input. Ordinary transient failures without revocation SHALL retain retry eligibility.

#### Scenario: Manual rename during an in-flight title attempt
- **WHEN** a title worker owns the route, a manual title command revokes it, and the worker later encounters a retryable setup or persistence failure
- **THEN** subsequent input cannot claim the old automatic title route

#### Scenario: Transient failure without manual revocation
- **WHEN** title setup fails while its ownership remains valid
- **THEN** the route can be restored for a later eligible input without creating simultaneous owners

### Requirement: Hydrate reasoning effort before actor publication
Cold session load SHALL initialize the actor sampling effort from the persisted selection before publishing the actor or admitting model controls. Reopening an unchanged selection SHALL NOT append a model transition from a configuration default that differs from the durable selection.

#### Scenario: Saved effort differs from model default
- **WHEN** a valid session ends with effort high and the configured model default is max
- **THEN** cold load initializes the actor with high, does not record max → high for hydration, and a later resume preserves continuity.

#### Scenario: User switches after loading
- **WHEN** a loaded session accepts an actual effort or model change
- **THEN** the new model observation starts from the preceding durable selection and a subsequent resume succeeds.

#### Scenario: Existing discontinuity
- **WHEN** a stored model observation does not continue the preceding durable selection
- **THEN** load continues to reject the invalid history rather than silently rewriting or skipping it.

#### Scenario: Persisted unset effort
- **WHEN** the saved effort is unset and the model configuration now has a default effort
- **THEN** cold load preserves the unset value rather than substituting the default.

#### Scenario: Unsupported historical effort
- **WHEN** the configured model no longer admits a saved explicit effort
- **THEN** cold load reports that incompatibility before publishing an actor, without inventing a replacement selection.

#### Scenario: Resident reconnect
- **WHEN** a client reconnects to a resident session
- **THEN** load retains the live actor sampling selection, including unset effort, without submitting a model-control request for hydration.

### Requirement: Drain the complete session writer incarnation
Session lifecycle drain SHALL include termination of the persistence task and release of its storage writer ownership as well as termination of the dedicated actor thread. A flush acknowledgement alone SHALL NOT establish writer termination.

#### Scenario: Actor exits before persistence owner
- **WHEN** the actor thread has terminated but its persistence task still owns the writer lease
- **THEN** close/replacement drain remains pending and does not admit a replacement writer.

#### Scenario: Complete writer exit
- **WHEN** both the actor thread and persistence owner have terminated
- **THEN** lifecycle drain may finish and a subsequent cold load can acquire the writer lease.

#### Scenario: Drain timeout
- **WHEN** the persistence owner does not terminate before the existing drain deadline
- **THEN** drain reports the outstanding shutdown and preserves the old incarnation's ownership record for a later retry.

#### Scenario: Retained handle after actor exit
- **WHEN** an external resource retains a persistence sender after the actor thread exits
- **THEN** lifecycle drain explicitly closes persistence admission, drains already accepted messages, and waits for owner exit without requiring that handle to be dropped.

### Requirement: Request persistence stop during final thread-owner cleanup
When the last SessionThread owner is dropped, fallback cleanup SHALL request persistence stop after its dedicated actor thread has exited, including an already-joined thread. This destructor path SHALL NOT claim that persistence has finished or that replacement admission is safe before its task completion.

#### Scenario: Live thread enters reaper
- **WHEN** the final thread owner is dropped while the dedicated thread is still running
- **THEN** the reaper joins the thread before requesting persistence stop.

#### Scenario: Joined owner is dropped
- **WHEN** the last owner is dropped after its OS thread has already been joined
- **THEN** cleanup requests persistence stop through the retained weak route.

#### Scenario: Another owner remains
- **WHEN** a SessionThread clone is dropped while another logical owner remains
- **THEN** that drop does not request persistence stop.

### Requirement: Completed session cleanup is independent of live predecessors
The session thread reaper SHALL process completed session threads without waiting for earlier still-running sessions. It SHALL request each persistence stop only after joining that session's actor thread and SHALL wait without busy polling when there is no work.

#### Scenario: Earlier session remains alive
- **WHEN** a live actor is queued for final-owner cleanup before another actor that has already exited
- **THEN** the reaper joins and requests persistence stop for the exited actor without waiting for the live actor to exit.

#### Scenario: Live actor later exits
- **WHEN** the pending actor eventually exits
- **THEN** the same worker joins it and requests its persistence stop without creating another cleanup worker.

### Requirement: Local resource publication syncs a usable directory handle
Acknowledged local resource persistence SHALL synchronize the published parent directory using a sync-capable descriptor relative to its pinned directory capability. Failures after publication SHALL remain distinguishable from pre-publication failures.

#### Scenario: Persist resources on Linux
- **WHEN** resource state is atomically published and durable acknowledgement is requested
- **THEN** directory synchronization succeeds for a valid writable store even if its original capability descriptor uses O_PATH.

### Requirement: Image descriptions retain their original image evidence

An acknowledged `ImageProjection` SHALL retain original image payloads in immutable Timeline message evidence while advancing the affected current-branch Surface identities. Each image shadow SHALL carry an exact source fingerprint/count and a typed disposition: provider description and local OCR dispositions SHALL retain the image with a nonempty reusable description, while unsupported-model disposition SHALL remove the image from the materialized Surface and insert the canonical replacement `当前模型不支持多模态，图片已经被删除`. Live apply and bulk replay SHALL validate and produce identical Surface, compaction-reference and image-tool-path redaction results. Local OCR SHALL identify its engine, provider descriptions SHALL reference a valid ImageDescription Sideband, and unsupported-model replacement SHALL require the exact canonical text rather than arbitrary unproven content.

#### Scenario: Resume a described image

- **WHEN** a session with a completed provider description or local OCR projection is replayed
- **THEN** the current Surface contains the image and its description with consistent causal Surface identities, and immutable Timeline evidence remains available.

#### Scenario: Resume a removed unsupported image

- **WHEN** a session with an acknowledged unsupported-model image projection is replayed
- **THEN** the original message event still contains the raw image as evidence, while the current Surface contains one canonical replacement at that image group's causal position and no raw image from the group.

#### Scenario: Select request representation

- **WHEN** a known unsupported canonical provider/model pair prepares a request after successful description or OCR projection
- **THEN** the request uses available descriptions without mutating the retained image.

#### Scenario: Unresolved group is removed atomically

- **WHEN** an exact-revision projection contains both description-backed groups and unsupported-model groups
- **THEN** Timeline validates every source, fingerprint, count and disposition before accepting one event, then atomically attaches descriptions and removes unresolved Surface images.

#### Scenario: Image-bearing tool result is removed

- **WHEN** unsupported-model projection targets an image-bearing tool result
- **THEN** replay removes the result images, retains one canonical replacement in the same causal item and applies the validated tool-call, response-carrier and compaction-reference redactions so no model-visible path can re-inject the image.

#### Scenario: Projection persistence is not acknowledged

- **WHEN** a prepared image projection is invalid, cannot be durably written or has an unconfirmed acknowledgement
- **THEN** the accepted Surface remains unchanged and sampling cannot claim removal or resubmit from a temporary request copy.

#### Scenario: Switch to an unknown pair

- **WHEN** another canonical provider/model pair prepares its first request after a description-backed projection
- **THEN** original image parameters remain available even if another pair previously selected descriptions.

#### Scenario: Removed evidence is not implicitly resurrected

- **WHEN** another model prepares a request after an unsupported-model projection removed an image from the current Surface
- **THEN** request assembly reads the projected Surface and does not recover raw media from historical Timeline evidence without an explicit branch operation.

### Requirement: Bulk replay avoids cumulative lifecycle copying
Timeline restoration SHALL validate all events and expose only a fully validated fold, without copying all accumulated lifecycle history for every replayed event. Live prepare and accept SHALL retain failed-write atomicity.

#### Scenario: Long valid history
- **WHEN** a long Timeline is restored in bulk
- **THEN** Surface, event sequence, lifecycle ownership and pending Control activation match transactional event acceptance.

#### Scenario: Invalid history
- **WHEN** an event violates content or lifecycle constraints during bulk replay
- **THEN** restoration fails without exposing a partially restored Timeline or modifying persisted session data.

#### Scenario: Invalid live append
- **WHEN** live prepare or accept rejects an event
- **THEN** the previously accepted Timeline remains unchanged.

### Requirement: Attempt evidence and accepted response have distinct authority

废弃 attempt 的证据、用量及恢复决定 SHALL 保留，但不授予模型上下文、native continuation 或工具执行权。每个 canonical response admission SHALL 在同一 Timeline 事实中持久化 request/attempt identity、canonical payload、确定性 admission result 及闭合的展示来源/anchor 引用；必要 artifact 与 anchor 先完成原 durable publication，该 admission 的 file/directory barrier 确认后才允许 Accepted 和依赖动作。Replay projection SHALL 从已验证事实派生并共享正文，不再单独持久提交。相同身份相同 payload SHALL 返回原结果，不同 payload SHALL fail closed；无法准备必要展示依据时不得提交 admission。

#### Scenario: Failed generation is followed by a valid attempt
- **WHEN** 较早 attempt 被拒收，较后 attempt 获得 durable admission
- **THEN** 两者证据可追溯，只有后者进入 accepted history、Surface 或可执行工具。

#### Scenario: Admission write acknowledgment is lost
- **WHEN** Timeline 记录可能已写入但 durable ACK 不明
- **THEN** 只核对原 identity 和 payload 并完成原同步；确认前不发布 Accepted、不继续工具/provider/成功 terminal，不重新采样。

#### Scenario: Exact admission submission is repeated
- **WHEN** 同一身份及 canonical payload 再次提交
- **THEN** 返回同一 admission，不重复事实或展示，不重复安装或从历史恢复 native continuation。

#### Scenario: Admission identity is reused with a different payload
- **WHEN** 已占用身份收到不同 payload
- **THEN** 拒绝冲突，已有 Timeline、Surface 与历史不变，不执行新 payload。

#### Scenario: Replay projection cannot be confirmed
- **WHEN** response 的 anchor、正文或必要展示引用不能验证或尚未持久确认
- **THEN** admission 失败或保持未确认，不写入一个需另补 cache 才成立的成功事实。

#### Scenario: Delivery fails after admission
- **WHEN** admission 已确认，但通知编码或客户端交付失败
- **THEN** 保留唯一 canonical admission，报告交付问题；恢复只派生同一历史，不改写 admission、不重采样或重发工具。

#### Scenario: Projection is reconciled during recovery
- **WHEN** cold/replacement load 发现已确认 response 尚未交付展示
- **THEN** 从同一 Timeline 来源派生历史，不写第二份投影、不恢复 native continuation、不执行 provider/工具，不复活被排除内容。

#### Scenario: Process stops before attempt closure
- **WHEN** 进程在采样证据、结算或 admission 闭合前停止
- **THEN** 保留未确认状态和原身份，不把 candidate 当作成功；已确认 admission 的历史可直接重建，不依赖第二份 projection 提交。

#### Scenario: Large response projection and independent replay
- **WHEN** 大正文被冷恢复、resident delta 或 fork 展示
- **THEN** 内存投影共享 canonical 正文，最终 ACP 输出保持顺序，fork 不继承父 admission/candidate 权威。

### Requirement: Turn terminals identify their actual authority

新 Turn terminal SHALL 明确区分 provider 响应驱动、宿主控制/错误、用户取消和中断恢复。provider 驱动终态 SHALL 关联相同 Turn 的已完成 request，其原始 provider terminal 由 request/attempt 证据保存。宿主 stop_reason/completion_kind SHALL 仅是本地分类。历史未记录来源的终态 SHALL 保持未知，不据旧字符串猜测或回写。存在未消费的有效挂起意图或 checkpoint 时 SHALL 先核对挂起边界；有效挂起保留原逻辑 Turn，不生成 Recovery terminal。物理 request/工具仍按事实记录中断和结算，不能据此终结所属逻辑 Turn；挂起证据不足时保持恢复受阻。

#### Scenario: Natural response closes the turn
- **WHEN** 完整正常 provider 响应使普通 Turn 收尾
- **THEN** Turn terminal 标识 Provider 来源并关联其 request，可追溯到对应 attempt 的原生终止。

#### Scenario: Host stops after a provider response
- **WHEN** provider 已结束，但宿主因预算、控制、持久化错误或用户取消收尾
- **THEN** 保留原始响应证据，同时追加宿主或用户来源的 Turn terminal，不覆盖原始原因。

#### Scenario: Process recovery closes an open turn
- **WHEN** 重启时发现未闭合生命周期，且没有尚可消费的挂起续跑依据
- **THEN** 追加 Recovery 来源的 interrupted 终态；不伪造 provider 事件或正常成功。

#### Scenario: Suspended logical turn continues under its original identity
- **WHEN** root 或 child 的原 Turn 具有可验证的挂起续跑位置，且恢复获得合法执行所有权
- **THEN** 保留该 Turn 和既有已提交前缀，在原位置继续，不重复 TurnStarted 或接纳原 prompt；只有后续真实完成、取消或失败才按实际来源生成一次 Turn terminal。

#### Scenario: Historical source is absent
- **WHEN** 读取没有记录来源的旧终态
- **THEN** 来源保持 Unknown，不把 end_turn 推断为 provider 返回。

### Requirement: Independent Windows session loads coexist with live writers
Independent Windows session loads SHALL use contained observation handles that coexist with the live writer's publication capability. Observation handles SHALL preserve identity and no-reparse validation and SHALL NOT enter the adapter's writer capability cache. Sideband crash reconciliation SHALL remain exclusive to an admitted replacement writer.

#### Scenario: Observe active sideband then replace its writer
- **WHEN** an independent adapter loads a session with a running Sideband and current projections while its writer is alive
- **THEN** ordinary full and light loads succeed without modifying the Sideband, replacement-writer admission remains rejected, and only after the old writer exits can a new writer append one recovery terminal.

#### Scenario: Loaded entity retains its authority
- **WHEN** a session load includes Workflow state or an adapter already has a pinned writer capability
- **THEN** dependent reads reuse the validated entity and do not reopen it through an incompatible or redirected ambient path.

### Requirement: Windows session storage preserves long-path publication and scan exclusions
Windows contained storage SHALL publish immutable artifacts and session entities under valid paths exceeding MAX_PATH while retaining source identity and no-replace semantics. A regular file encountered where a session or cwd directory is expected SHALL be excluded as an invalid entity; operational I/O failures SHALL still fail the scan.

#### Scenario: Publish and restore input artifacts beneath a long profile
- **WHEN** a text/image input payload is stored under a session path exceeding 260 UTF-16 units and then read or stored again
- **THEN** the complete payload round-trips, its reference remains stable, and conflicting pre-existing bytes are preserved and rejected.

#### Scenario: Scan contains ordinary files and valid sessions
- **WHEN** session enumeration encounters ordinary files at cwd and session directory levels alongside a valid session
- **THEN** it skips those files and invalid summaries while returning the valid session.

#### Scenario: Publication target has any legal filename alignment
- **WHEN** a file or session directory is published with a legal name of any UTF-16 alignment
- **THEN** the exact intended name is committed, no extra suffix appears, and existing targets remain unchanged on collision.

#### Scenario: Coordination reads a live source's durable inquiry history
- **WHEN** coordination resolves its source Timeline by session id while that session's publication handle remains live
- **THEN** the read uses the existing independent observation capability, validates the same identity and Timeline, and does not acquire or cache writer authority.

#### Scenario: List sessions while a publication handle is alive
- **WHEN** an independent adapter enumerates summaries while a session writer retains its publication handle
- **THEN** the live session remains visible through observation handles, which do not enter the writer cache; later mutations still acquire their normal writer capability and lease.

### Requirement: Tool preflight failures retain their actual category

Tool preflight SHALL distinguish an unregistered tool name from arguments that cannot be parsed for a registered tool. An unregistered name SHALL NOT be dispatched and SHALL produce exactly one failed tool result that states the tool was unavailable and not executed. Invalid arguments for a registered tool SHALL retain the argument-parse diagnostic and original-argument recovery context.

#### Scenario: Unknown name with valid JSON
- **WHEN** an admitted model response calls an unregistered tool name with syntactically valid JSON
- **THEN** preflight records the call as a non-existing invalid tool, emits one failed result, and performs no tool dispatch.

#### Scenario: Registered tool has invalid arguments
- **WHEN** an admitted model response calls a registered tool with arguments its parser rejects
- **THEN** the existing argument-parse result remains available and is not relabeled as an unknown tool.

### Requirement: Session usage is a durable lifetime projection

Session Timeline SHALL 持久记录每个主模型 attempt 的已知/未知 Usage、每个子 Agent 的最终 Usage 结算，以及 session-level incomplete 事实。缓存读写可用性、已知桶累计和覆盖分母 SHALL 作为原结算事实的一部分保留；总量已知但分类未知 SHALL NOT 被改为整个总消费不完整。冷恢复 SHALL 按结算身份去重重建 lifetime aggregate、provider/model 分项和 Agent 归属；不得把已有消费重置为零或重复计费。已知用量事件 SHALL 在 actor 发布及恢复事件持久化前完成载荷校验；格式损坏或同身份冲突 SHALL 使恢复失败并保留原始记录。

#### Scenario: Restore settled main attempts
- **WHEN** Timeline 含多个已持久化主模型 attempt 结算并创建新的 actor incarnation
- **THEN** 新 actor 在接受后续调用前恢复这些 attempt 的累计 token、model、cost、duration、incomplete 状态及缓存桶可用性和覆盖分母，每个 attempt 至多计入一次。

#### Scenario: Restore settled child usage
- **WHEN** 父 session 已持久结算一个子 Agent 的多模型 Usage 后冷恢复
- **THEN** lifetime 总计、provider/model 分项与该 `subagent_id` 的 Agent 分项均恢复；相同结算重放不重复累计，冲突结算失败关闭而非覆盖。

#### Scenario: Restore incomplete accounting
- **WHEN** session-level incomplete 事实已持久化
- **THEN** 后续 resume 仍将 lifetime Usage 表示为已知下界并隐藏不可信 cost，不因进程重启恢复为精确账本。

#### Scenario: Exact duplicates and conflicting attempt payloads
- **WHEN** 同一主模型 attempt 或子 Agent 身份有多份用量结算
- **THEN** 完全相同载荷只计一次；任一载荷冲突使恢复失败，不选择第一份、不覆盖，并采用与实时结算相同的冲突规则。

#### Scenario: Malformed known usage facts fail before publication
- **WHEN** 已知 attempt/child 结算无法按既有 typed schema 解码，或 incomplete/resume 标记包含不属于其格式的 data
- **THEN** 恢复返回带事件位置的错误，不发布可用 actor、不写入恢复事件，原持久化记录保持不变。

#### Scenario: Unrelated diagnostic observations remain extensible
- **WHEN** Timeline 包含不属于已知用量 scope/name 的合法 Observation
- **THEN** 用量恢复忽略该诊断事实，不将其当成损坏的结算；其他 Timeline 校验仍执行。

#### Scenario: Child cache details are partially available
- **WHEN** 子 Agent 的最终账单包含总消费已知但部分 attempt 缓存明细缺失的多模型汇总
- **THEN** 父 session 保留已知桶、未知状态与覆盖分母，各聚合维度只折叠一次，不能将分类未知归零或把精确总消费降为未知。

#### Scenario: Conflicting cache availability
- **WHEN** 相同结算身份的重复载荷在缓存可用性或覆盖计数上冲突
- **THEN** 实时和恢复均沿既有冲突规则失败，不按重复成功忽略，也不覆盖先前事实。

### Requirement: Cold resume creates a durable usage segment boundary

成功发布新的冷恢复 actor incarnation 前，session SHALL 持久提交一个 Usage resume boundary。恢复投影 SHALL 以初始运行和这些边界切分结算，aggregate SHALL 等于所有 segment 的累计结果，包括各缓存桶已知累计、未知标记和覆盖分母；命中率 SHALL 由累计计数重新计算而非平均 segment 百分比；resident client reconnect 不创建新的 actor segment。

#### Scenario: First cold resume
- **WHEN** 已有 session 在新 actor incarnation 中成功恢复
- **THEN** 后续结算进入 Resume #1 segment，此前结算保留在 Initial run，lifetime aggregate 同时包含两段。

#### Scenario: Repeated cold resumes
- **WHEN** session 多次关闭并冷恢复
- **THEN** 每个成功发布的 incarnation 形成有序的新 segment，后续结算只进入当前段，历史段保持不可变。

#### Scenario: Resident reconnect
- **WHEN** 客户端重新连接仍存活的同一 actor incarnation
- **THEN** 不创建新的 Usage segment，既有当前段继续累计。

证据入口：`crates/codegen/chat-state/src/usage.rs`、`actor/mutations.rs`、`actor/state.rs` 与 `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs`。

### Requirement: Session observation does not repair durable projections
普通会话 full/light observation SHALL 从有效 Timeline 派生 title/model 的内存投影，不写 Summary、不获取 writer lease，也不执行 sideband crash repair。显式 replacement-writer load SHALL 在取得独占 lease 后修复滞后的持久投影。观察 SHALL 保留 canonical 数据校验和投影冲突拒绝。

#### Scenario: Observe lagging projections beside a live writer
- **WHEN** 当前 writer 仍存活而 Summary 的 title/model 落后于有效 Timeline
- **THEN** 独立 full/light observation 成功返回 canonical 内存值，Summary 和 sideband 文件不变，观察 adapter 不获得 writer capability。

#### Scenario: Replacement writer repairs the lag
- **WHEN** 原 writer 退出后新 writer 获得 lease 并加载相同会话
- **THEN** 落后的 Summary 被修复为 Timeline 的 canonical title/model。

#### Scenario: Conflicting or malformed canonical data
- **WHEN** Summary title 在相同或更高序号与 canonical title 冲突，或 model change 数据损坏
- **THEN** 观察与 writer load 都拒绝恢复，不以只读模式绕过校验。

证据：crates/codegen/shell/src/session/storage/jsonl/mod.rs 与 tests.rs。

### Requirement: Parent message receipts retain replayable presentation evidence

持久接收的父消息 SHALL 在所属会话保留期间保留可重建接收 UI 的来源、身份、投递模式和原始正文。正文的展示证据 SHALL 在消息消费后继续有效，UI 投影丢失不得删除收件事实。UI 恢复 SHALL 不重新接纳或消费消息，并保留现有模型输入的 agent guidance 来源语义。

#### Scenario: Consume then restore without UI cache
- **WHEN** 父消息已消费且会话的可丢失 UI 投影不可用，随后加载会话
- **THEN** 从持久收件事实恢复唯一的来源明确、正文完整的接收展示，同时该消息仍保持已消费。

#### Scenario: Cleanup and shared payloads
- **WHEN** 即时 payload 清理或启动 sweep 处理已消费通知，且其正文仍被父消息历史引用
- **THEN** 保留该正文引用及可读内容，包括与其他通知共享的内容；无引用 orphan 继续沿原规则清理。

#### Scenario: Durable commit and presentation publication are separated
- **WHEN** 持久接收完成而 UI 发布失败，或者同一父消息重试
- **THEN** 保留原接收身份和投递结果，后续恢复补全 UI；不写第二个接收事实或生成第二份模型输入。

#### Scenario: Historical body is unavailable
- **WHEN** 历史父消息正文缺失或校验失败
- **THEN** 展示保留可验证的收件身份并明确正文不可恢复，不伪造原文；未消费通知的模型上下文恢复仍执行原有严格校验。

#### Scenario: Retry changes delivery mode
- **WHEN** 同一父消息身份和正文被重试，但 interrupt 模式发生变化
- **THEN** 拒绝冲突请求，保留原接收事实与投递模式，不让 UI 或执行路径把重试参数当成已接收状态。

#### Scenario: Unsupported parent message representation
- **WHEN** 父消息使用不支持的 payload 表示版本
- **THEN** 明确拒绝该格式，不从自然语言包装猜测原始正文或静默改变模型输入。

### Requirement: Repeated rewind preserves retained response provenance

Timeline SHALL 在一次或多次 rewind 后继续把保留的历史响应关联到原 admission；compaction 或 Surface replacement SHALL NOT 将该归属重置为最近一次 rewind。切出当前 branch 的响应 SHALL 不再成为 replay authority。

#### Scenario: Rewind twice after replacing a later prompt

- **WHEN** 会话保留早期 response，rewind 较晚 prompt，生成替代分支后再次 rewind
- **THEN** 早期 response 的 admission 和重载回放保持一次，两个被切走的 response 均不复活。

#### Scenario: Repeated rewind crosses compacted history

- **WHEN** 保留前缀经过 compaction，随后发生重复 rewind 和冷 fold
- **THEN** 保留 response 的 provenance 与未压缩前缀一致，回放不遗漏也不复制历史响应。

### Requirement: Exact replay commits acknowledge durable barriers

响应与独立持久展示事实的 exact commit SHALL 由其唯一 Timeline 记录及必要引用承担；所需 file/directory sync 完成后才返回成功。读取到相同 key/payload 不得代替同步确认，也不得通过派生 UI 数据建立第二个 durable gate。

#### Scenario: Complete record remains readable after sync failure
- **WHEN** 完整记录可读但 file 或 directory sync 失败
- **THEN** exact reconcile 仍失败，依赖该确认的 Accepted、provider 或工具不放行。

#### Scenario: Exact retry after storage recovers
- **WHEN** 同一身份/payload 在同步恢复后重试
- **THEN** 同步原记录后成功，不重复追加；不同 payload 仍拒绝。

### Requirement: Sampling recovery stop evidence survives session storage

会话存储 SHALL 接受采样器持久记录的 `sampling_evidence/recovery_stop`，在恢复及导入导出中保留原始恢复决定，并继续校验记录名称、类型及 artifact 引用完整性；停止恢复记录 SHALL NOT 被当作已接纳的模型响应或重发请求的指令。

#### Scenario: Load a session after recovery stops
- **WHEN** 会话 Timeline 包含格式有效的 recovery_stop 证据
- **THEN** 观察加载和写者恢复均能读取该会话，证据保持原值，不因该记录启动 provider 请求或增加模型上下文。

#### Scenario: Transfer mixed sampling evidence
- **WHEN** 导入导出的 Timeline 同时包含 recovery_stop 和带 body 的 response 证据
- **THEN** 保留停止决定并完整校验二进制 artifact；缺失或篡改 body 仍被拒绝。

#### Scenario: Invalid evidence remains invalid
- **WHEN** 证据类型未知、名称与类型不一致或分块引用不合法
- **THEN** 读取失败并保留原始记录，不以支持 recovery_stop 为由跳过校验。

证据入口：`crates/codegen/shell/src/session/sampling_evidence.rs::decode_record`、`crates/codegen/shell/src/session/storage/jsonl/tests.rs`、`crates/codegen/shell/src/extensions/session_state.rs::sampling_blob_tests`。

### Requirement: Sideband coordinates follow canonical Timeline Surface producers

Sideband parent validation SHALL accept a historical Surface coordinate exactly when the referenced parent Timeline event canonically produced that item, including consumed user input and consumed notification input. It SHALL continue to reject non-Surface lifecycle facts, wrong event identity, out-of-range items and coordinates outside the frozen source/input ranges. Strict reload SHALL apply the same rule as live Timeline materialization.

#### Scenario: Reload a compaction selected from consumed inputs

- **WHEN** a completed compaction Sideband selected valid coordinates produced by `Input::Consumed` or `Notification::Consumed` with model input
- **THEN** strict session reload accepts the Sideband ledger and reconstructs the same Surface without migration or ledger rewrite.

#### Scenario: Reject a non-Surface coordinate

- **WHEN** a Sideband manifest names an Observation, lifecycle-only Input/Notification event, or an item outside a producing event's cardinality
- **THEN** parent validation rejects the ledger as an invalid Surface selection.

### Requirement: Canonical cancelled subagent lifecycles are resumable

A cancelled subagent SHALL be a valid durable resume source when its parent Timeline contains matching Spawned and Ended facts, its child identity and seed match that spawn, and the terminal result reference resolves to an exactly matching validated child SubagentResult. Resume eligibility SHALL be based on this canonical linkage and the existing security/workspace checks, not on requiring a successful or completed outcome. Resuming SHALL preserve the source agent, model route, reasoning effort, context and worktree semantics and SHALL NOT relabel the cancelled source as successful.

#### Scenario: Goal stop cancels a child using another model

- **WHEN** a root Goal stop cancels a child whose model route differs from the root, and Spawned, Ended(cancelled), SubagentSeed and SubagentResult(cancelled) form a valid canonical link
- **THEN** a later explicit resume accepts that child as its source and pins the original child route instead of rejecting it because its outcome is cancelled or silently replacing it with a fresh spawn.

#### Scenario: Cancelled result has an exact canonical link

- **WHEN** parent and child identities, spawn/seed coordinates, outcome, duration, tool/turn counts, usage, error and result reference all match and both Timelines validate
- **THEN** resume uses the child as a durable source under the existing security and workspace constraints.

#### Scenario: Cancelled lifecycle is incomplete or inconsistent

- **WHEN** the source lacks a spawn or terminal, the child identity/seed does not match, the result reference is missing or invalid, or either Timeline fails validation
- **THEN** resume fails closed without starting a child, provider request or worktree mutation and without treating the source as a valid canonical lifecycle.

### Requirement: Subagent resume rejection preserves its cause

Durable subagent resume resolution SHALL preserve a typed cause for parent or child storage failure, lifecycle incompleteness, identity or security rejection, Timeline validation failure and invalid result linkage. Live activity SHALL remain a separate observation. User-visible errors SHALL distinguish actionable authorized-source categories and SHALL NOT describe every rejection as a missing completed lifecycle. Security rejection SHALL NOT reveal whether an unauthorized source session exists.

#### Scenario: Source is still settling

- **WHEN** the durable lifecycle has no terminal and the live coordinator still owns the source child
- **THEN** resume reports that the source is still running or settling and does not claim that a completed-only outcome is required.

#### Scenario: Authorized source has invalid durable linkage

- **WHEN** the requester is authorized for the source lineage but child loading, Timeline validation or exact result-link validation fails
- **THEN** resume reports the corresponding durable source category, records the internal typed cause and starts no replacement child.

#### Scenario: Requester is outside the security lineage

- **WHEN** the requester is neither the lifecycle root nor the recorded security parent
- **THEN** resume rejects the request without exposing whether storage, lifecycle or result facts exist for that source.

### Requirement: Subagent resume admission is fail-closed and retry-safe

Subagent resume SHALL reject a source while its original runtime remains live, even if terminal facts are already visible. After durable source validation, agent/model/transport/effort, workspace, context and derived-child admission failures SHALL stop only the requested resume epoch, preserve the immutable source lifecycle and return the failing stage. No valid resume request SHALL silently become a fresh child. A later retry SHALL revalidate the source and current environment.

#### Scenario: Durable terminal is visible while the source remains live

- **WHEN** the parent and child contain an exact terminal link but the coordinator still owns the source as pending, active or settling
- **THEN** resume reports that the source is still running or settling and does not start an overlapping child epoch.

#### Scenario: Historical non-worktree cwd is missing

- **WHEN** a validated non-worktree source names a cwd that no longer exists and the current parent workspace is valid
- **THEN** resume uses the current parent workspace and preserves the source transcript and route; an existing non-directory, unverifiable path or canonical path outside the parent workspace remains rejected.

#### Scenario: Source route or context is incompatible

- **WHEN** the source agent type, model, reasoning effort or transport is unavailable, its transcript cannot be validated or fit safely, or required prompt artifacts cannot be loaded
- **THEN** resume identifies the incompatible stage, starts no fresh replacement and does not change the source lifecycle or guess a substitute route, effort, context or artifact.

#### Scenario: Current child System head cannot be rendered

- **WHEN** a validated resume source already contains its stable System head but the current child-audience head renderer is unavailable or invalid
- **THEN** resume preserves the inherited head and does not invoke the current renderer; a new or normalized child without a renderable head still fails before persistence.

#### Scenario: Historical completion output artifact is missing

- **WHEN** a validated source has an exact terminal/result link and materializable Timeline Surface but its optional completion output artifact is absent
- **THEN** resume remains eligible because the display artifact is not context authority; any immutable prompt blob directly referenced by the Surface remains required.

#### Scenario: Workflow route authority no longer exists

- **WHEN** a workflow-owned resume source is canonical but its owning Workflow Run or frozen runtime route is no longer registered
- **THEN** resume rejects the derived epoch and does not replace the route with the current global agent definition.

#### Scenario: Isolated source workspace cannot be restored

- **WHEN** a source worktree is absent without a snapshot or snapshot rehydration cannot produce the exact recorded target
- **THEN** the derived resume epoch fails without discarding the snapshot reference or claiming an empty workspace continuation.

#### Scenario: Derived child admission fails after source validation

- **WHEN** parent spawn persistence, child storage, session startup, catalog convergence, promotion or first-prompt admission fails
- **THEN** the requested derived epoch is failed or left for existing canonical recovery as appropriate, while the original source remains unchanged and eligible for a later fully revalidated retry.

#### Scenario: First prompt is not durably admitted

- **WHEN** child control publication, Goal snapshot mailbox delivery, QueuePrompt delivery or the durable prompt persistence acknowledgment fails
- **THEN** the derived epoch reports the exact launch stage, admits no further provider work, settles any prompt that may already have started before committing its child result, and leaves the original resume source unchanged.

### Requirement: Agent opinion consumption preserves source and exact context

正式 agent 消息 SHALL 复用 durable Received/Consumed 生命周期。消费与准确的 runtime agent-message context item SHALL 在同一持久事实中提交，保留 receipt、双方身份、reply 关联和正文；不得拆成先确认消费后另写模型输入。发送方既有调用正文 SHALL NOT 再作为自己的接收消息重复注入。

#### Scenario: Consume an opinion
- **WHEN** 目标在安全步骤消费已接收意见
- **THEN** 该 receipt 与有来源的准确正文一起进入 Surface，回复内容不带人类权限证据。

#### Scenario: Restore after consumption
- **WHEN** 消费后崩溃并重建 Session
- **THEN** 同一消息只恢复一次，保留 reply 关系，不重新投递、重新唤醒或伪造新输入。

#### Scenario: Send and receive on the same agent
- **WHEN** agent 发出意见后收到另一方回复
- **THEN** 自己的意见保留在原发送调用/回执中，对方意见作为接收 item；不会为自己的发送增加重复入站正文。

### Requirement: MCP image evidence survives text-output truncation

主工具结果中的 MCP 图片 SHALL 先与会被截断的文本分离，再按现有图片验证、正规化与预算进入模型可见附件；已接纳的附件顺序和省略说明 SHALL 可由 Timeline 恢复的 Surface 重建。工具结果本体与附件 SHALL 不因 direct extension 调用而混入另一条会话。

#### Scenario: Long mixed result is restored
- **WHEN** MCP 结果含超长文本、结构化内容和多张有效图片，文本预览被截断后会话恢复或切换模型
- **THEN** 截断正文、完整输出指针和预算内图片各保留一次且顺序一致；恢复/portable 请求不把图片退化为 base64 字符串。

#### Scenario: Rejected image is explicit
- **WHEN** 一个图片附件因类型、解码或预算被拒收
- **THEN** Surface 有明确替代说明，后续有效附件仍按原序处理，不产生损坏的图片 part。

### Requirement: Auxiliary provider attempts enter the owning session usage projection

Session Timeline SHALL durably record known or unknown usage for each admitted main and Sideband provider attempt, each child's final usage bill and session-level incomplete facts. Sideband attempt identity SHALL be `(sideband_id, attempt_no)` in its owning session; each attempt SHALL be counted at most once, including failed and retried requests. A cold recovery with an admitted Sideband attempt lacking terminal usage SHALL retain an incomplete lower-bound ledger. Recovery SHALL reject malformed or conflicting known billing facts before publishing an actor. A child Sideband SHALL be charged to its child ledger and reach the parent only through the child's final bill.

#### Scenario: Failed attempt followed by a successful retry

- **WHEN** one Sideband issues two provider requests under successive attempt numbers, the first fails with known usage and the second succeeds
- **THEN** the owning session records both usages once under the selected route model, while the Sideband Result is not billed again

#### Scenario: Cancellation or crash after admission

- **WHEN** a Sideband provider request has been admitted but usage cannot be confirmed before owner cancellation or cold recovery
- **THEN** the owning session retains an incomplete usage lower bound rather than an exact zero, without fabricating token counts

#### Scenario: Duplicate and conflicting Sideband bills

- **WHEN** a Sideband attempt settlement is repeated with an identical payload or with a different payload
- **THEN** the identical payload is a no-op and the conflicting payload fails closed in live and restored projections

#### Scenario: Child Sideband final bill

- **WHEN** a child consumes tokens in its main loop and a Sideband, then settles its final bill to its parent
- **THEN** the child ledger includes both attempts and the parent includes their aggregate once under that child identity

#### Scenario: Known auxiliary totals with missing cache fields
- **WHEN** Sideband 的 full input/output 已知但缓存读写字段部分缺失
- **THEN** 原 Sideband attempt 结算保留缓存可用性与覆盖数据，总消费保持精确；经过 child final bill、冷恢复和报表分段后仍保持同义。

### Requirement: Resident reconnect preserves live turn ownership

A viewer reconnecting to a resident session SHALL NOT run interrupted-scope recovery against that session's live turn. Such recovery SHALL occur only after a replacement writer claims a new incarnation. Subagent fact reconciliation MAY continue during resident reconnect without closing unrelated active work.

#### Scenario: Viewer attaches during a live turn

- **WHEN** a leader viewer loads a resident session while its original writer still has an active turn
- **THEN** the viewer receives replay/live updates without an interrupted Recovery terminal for that turn, and the original actor records its matching real terminal before later prompts run.

### Requirement: Cold model route changes continue durable selection
Fresh sessions SHALL durably record a secret-free baseline for the selected catalog model, reasoning effort, provider-facing model, and backend/endpoint transport identity before admitting prompts. Cold load SHALL compare the last durable model route with the currently resolved catalog route and, when they differ, durably append an exact from/to transition before publishing the actor or admitting sampling. A failed append SHALL prevent publication. A historical Timeline without a route observation SHALL receive an explicit current-route baseline without inventing an earlier route. Resident reconnect SHALL retain its live route. Stored discontinuous transitions SHALL still be rejected.

#### Scenario: Catalog route changes across restart
- **WHEN** the same catalog model ID resolves to a different backend, endpoint, query route, or wire model after a cold restart
- **THEN** the replacement writer records a transition from the last durable route to the selected current route before the resumed actor can sample, and a later reload validates the continuous chain.

#### Scenario: Catalog route is unchanged
- **WHEN** a cold load resolves the same model, effort, provider model, and transport identity as the latest durable observation
- **THEN** it does not append a redundant transition.

#### Scenario: Stored history is already discontinuous
- **WHEN** two existing model observations disagree on the exact preceding route
- **THEN** load rejects the invalid history rather than bridging or rewriting it.

### Requirement: Candidate preview uses a durable payload-free anchor

Transient session persistence SHALL retain neither candidate notification payloads nor an unbounded window of independent notifications. The first candidate SHALL create one durable, payload-free Timeline ordering anchor before canonical response admission; the response SHALL reference that anchor. Independent durable presentation SHALL be backed by typed Timeline facts or references to their existing canonical source, not a separate updates ledger. During an active attempt, independent untagged ACP and one-shot Grow notifications emitted by the session actor SHALL be appended in arrival order with acknowledgement before another notification from that actor is delivered; previously buffered notifications SHALL be flushed first. A failed anchor or independent append SHALL prevent canonical admission. Live preview notifications from the session actor, including Grow notifications emitted during the attempt, SHALL have a per-session byte budget through gateway completion; exhaustion or an oversized single item SHALL fail the attempt's preview boundary before canonical admission. Replay SHALL suppress an anchor for a discarded attempt and SHALL insert admitted canonical content at that anchor before later independent updates. Live delivery MAY carry candidate notifications independently of persistence.

#### Scenario: Long interleaved attempt

- **WHEN** an attempt emits many candidate chunks interleaved with independent untagged ACP or one-shot Grow notifications
- **THEN** only one payload-free candidate anchor is persisted, earlier buffered updates are flushed first, independent facts stream to Timeline storage with acknowledgement rather than accumulating in the sender, and admitted replay places canonical content at the first candidate position.

#### Scenario: Preview persistence fails

- **WHEN** an anchor or independent append cannot be confirmed before response admission
- **THEN** the response is not admitted to the canonical Timeline, no candidate body enters replay, and the turn reports a projection-boundary failure.

#### Scenario: Discarded or interrupted attempt

- **WHEN** an attempt is discarded or stops without canonical admission
- **THEN** its anchor is suppressed in replay, while independently committed untagged notifications remain in their arrival order.

#### Scenario: Slow live gateway exhausts preview credits

- **WHEN** client completion stalls until the per-session preview byte budget is full, or one preview notification exceeds that budget
- **THEN** no additional candidate or Grow preview payload is enqueued to the gateway, the attempt records a preview-boundary failure, and canonical admission is refused without persisting candidate body text.

#### Scenario: Buffered update precedes an independent Grow notification

- **WHEN** a persistence writer holds an earlier merged ACP update while an active-attempt Grow notification arrives
- **THEN** it writes the earlier update first, confirms the Grow append before the actor can deliver another independent notification, and rejects canonical admission if either write fails.

### Requirement: Auxiliary Grow delivery shares the session preview budget

Goal and Workflow Grow presentation snapshots SHALL reserve the session's shared byte credits before entering persistence or gateway queues; their credits SHALL remain held through the respective consumer completion. A failed auxiliary reservation or append during an active attempt SHALL prevent canonical admission. A subagent progress publisher SHALL retain no more than one outstanding transient gateway delivery and SHALL remain cancellable while it waits. Canonical Goal control and Workflow run state persistence SHALL remain independent of optional presentation snapshots.

#### Scenario: Auxiliary Grow sender meets a slow consumer

- **WHEN** Goal or Workflow snapshots are produced faster than persistence or gateway delivery completes
- **THEN** retained snapshot payloads stay within the shared session byte credits, and an exhausted sender skips the optional presentation snapshot instead of growing either queue without bound.

#### Scenario: Auxiliary Grow delivery fails during an attempt

- **WHEN** a Goal or Workflow snapshot cannot reserve credits or its durable append fails during an active attempt
- **THEN** the attempt's admission barrier or projection commit fails, and no candidate body becomes durable replay content.

#### Scenario: Transient child progress meets a slow client

- **WHEN** a subagent progress notification is awaiting gateway completion while further progress ticks occur
- **THEN** its publisher does not enqueue another progress notification and remains cancellable.

### Requirement: Subagent lifecycle projections are bounded metadata

Subagent lifecycle Grow notifications SHALL carry status and identity metadata without duplicating the child final output. The canonical child result and admitted completion receipt SHALL remain the source of that output. The parent actor SHALL persist and forward one stamped event under the existing independent-update and active-attempt gateway budget boundaries. It SHALL forward a spawned lifecycle projection before emitting a later-stamped Goal status refresh caused by that spawn, so live clients can retain the running child. If an independent live update overtakes a lifecycle projection, the client SHALL reconcile missing spawn/finish facts by child identity without regressing its Grow highwater or reconnect cursor or reviving an already terminal child. A failed preview gateway reservation SHALL reject that exact attempt's canonical admission. Client-origin Grow notifications SHALL remain persist-only.

#### Scenario: Child finishes with a large answer during parent sampling

- **WHEN** a child result contains a large final answer and its parent has an active sampling attempt
- **THEN** the lifecycle Grow projection contains only metadata, its forwarded event uses the same event ID as its durable record, and the completion receipt continues to reference the full answer.

#### Scenario: Lifecycle preview gateway budget is exhausted

- **WHEN** the parent actor cannot reserve preview gateway credits for a child lifecycle projection
- **THEN** no uncredited lifecycle payload enters the gateway queue, that attempt fails preview admission, and canonical child lifecycle facts remain available for reconnect projection.

#### Scenario: Goal-owned child spawn refreshes Goal status

- **WHEN** a Goal-owned child spawn reaches the parent actor and triggers a Goal status refresh
- **THEN** the stamped SubagentSpawned projection is persisted and forwarded before the later-stamped GoalUpdated projection, and the live Tasks pane retains the running child.

#### Scenario: Independent live update overtakes a child lifecycle projection

- **WHEN** a higher-ID Grow update reaches a live client before a child spawn or finish whose durable projection is still pending
- **THEN** the client applies the missing child lifecycle fact once by child identity, retains its higher Grow highwater and reconnect cursor, and does not resurrect a child with an already visible terminal row.

### Requirement: Tool bridge Grow projections share preview credits

Background task completion Grow projections SHALL omit copied task output while preserving full output through the task file and model-facing notification. The tool bridge SHALL reserve session preview credits for queued TaskBackgrounded, TaskCompleted, and ScheduledTaskCreated Grow persistence and live gateway copies, and for ScheduledTaskFired and MonitorEvent live copies, holding each reservation until its consumer completes. A failed reservation or append during an active sampling attempt SHALL prevent canonical response admission. Durable task-completion acknowledgement SHALL remain a prerequisite for acknowledged UI projection publication. Monitor events SHALL retain their model notification.

#### Scenario: Completed task has a large output file

- **WHEN** a background task completes with a large retained output file during an active attempt
- **THEN** the Grow completion snapshot contains status metadata but no copied output, the model notification retains its existing truncated text and output-file pointer, and queued Grow copies stay within preview credits.

#### Scenario: Monitor events outrun a slow client

- **WHEN** monitor events arrive while the gateway has not completed prior Grow delivery
- **THEN** additional live Grow events cannot exceed the session preview budget, while model notification commands retain their normal admission path.

### Requirement: Subagent spawn projections retain effective reasoning effort

The subagent spawn Grow projection SHALL carry the effective reasoning effort selected for the child when one exists. Live publication and reconnect projection SHALL derive it from the same durable spawn fact, without substituting the model catalog's default effort.

#### Scenario: Child overrides its model's default effort

- **WHEN** a child is launched with an effective effort different from the catalog default
- **THEN** both its live spawn notification and a later projection from the durable spawn fact identify that effective effort.

#### Scenario: Model has no effort

- **WHEN** a child has no effective reasoning effort
- **THEN** the spawn projection omits the optional effort field.

### Requirement: Runtime observations preserve query uncertainty and ownership

会话与子任务状态查询 SHALL 区分已确认观察、已确认不存在、不可达、超时及证据无效，并明确不存在所指的 runtime、prompt 或持久记录范围。观察 SHALL 保留 session、适用的 child/prompt identity 及运行实例身份；不得把通信错误、注册表缺项或取消发送失败解释为持久任务已结束。查询 SHALL 在既有授权边界内处理。

#### Scenario: Runtime registration is absent but history exists
- **WHEN** 运行实例已移除，但授权会话仍有可验证持久记录
- **THEN** 查询区分实例不存在和记录存在，返回可确认的持久状态，不报告整个 session 不存在。

#### Scenario: Coordinator cannot answer
- **WHEN** 查询发送失败、响应丢失或超过期限
- **THEN** 返回对应的不可达或超时，保持执行 outcome 未确认，不据此生成 interrupted 或 cancelled 终态。

#### Scenario: Caller lacks source access
- **WHEN** 调用者无权查询目标来源
- **THEN** 返回授权拒绝，不通过状态分类泄漏目标是否存在。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/backend.rs::inspect/query/cancel`、`crates/codegen/shell/src/extensions/session_admin.rs::handle_prompt_status`。上述区分为本 change 新增契约。

### Requirement: Session health observation remains bounded and read only

状态观察 SHALL 分别报告可确认的执行、持久提交、交付和观察新鲜度。控制查询 SHALL 在总计不超过五秒或调用方更早期限内结束等待，包含排队与加载等待；其完成不得依赖被观察的模型、工具、hook 或写入操作结束。查询并发与快照积压 SHALL 有界。读取持久会话信息 SHALL 遵循只读观察契约，不隐式恢复执行或修改磁盘投影。

#### Scenario: Persistence acknowledgment never arrives
- **WHEN** 会话写入 ACK 一直不返回且运行实例尚未退出
- **THEN** 状态查询仍能返回最后确认的提交位置和观察限制，或在期限内返回超时，不能永久排在该 ACK 后面。

#### Scenario: Durable metadata read exceeds the deadline
- **WHEN** 无 handle 的会话只读扫描超过查询期限
- **THEN** 调用方获得超时；仍在运行的读取保留其工作许可，后续请求不能绕过并发上限，也不启动 writer 修复。

#### Scenario: Execution returned before result publication
- **WHEN** owner 已确认执行返回，但结果提交尚未确认
- **THEN** 观察同时保留这两个事实，不显示为仍等待模型，也不发布未确认的 durable Finished。

证据与实施入口：`crates/codegen/shell/src/session/handle.rs`、`crates/codegen/shell/src/session/actor/spawn.rs::SessionThread`、`crates/codegen/shell/src/session/storage/jsonl/mod.rs`；既有只读与 writer drain 契约继续适用。

### Requirement: Subagent finalization retains one accountable owner

子任务执行返回、失败、panic 或取消后，系统 SHALL 保留该 child 的收尾责任，直到所需持久事实和交付责任已确认，或明确展示仍待修复。执行 outcome、提交状态和交付状态 SHALL 分离。未确认收尾不得被普通完成缓存淘汰、作为已完成依赖使用或用于绕过源实例仍存活时的 resume 拒绝。并发推进和暂存资源 SHALL 有界。

#### Scenario: Runner exits after output publication fails
- **WHEN** runner 已返回，但 output artifact 或 child result 提交失败
- **THEN** 查询显示执行已返回且提交受阻，保留同一 child 的收尾责任；不会把它静默移入可淘汰的完成项。

#### Scenario: Panic competes with cancellation
- **WHEN** runner panic 与取消或正常收尾竞争
- **THEN** 只有一个收尾 owner 推进原 child 的事实；已确认 outcome 不被覆盖，未确认状态仍可查询。

#### Scenario: Pending finalization reaches capacity
- **WHEN** 待收尾元数据或未持久正文达到容量上限
- **THEN** 新准入受到反压，超限正文保全失败可见，既有未确认责任不被当作成功完成或静默丢弃。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs::finish_child/finish_panicked_child`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`。

### Requirement: Subagent reconciliation advances only verified commit prefixes

运行期与重启恢复 SHALL 按同一 child/parent 身份链核对已提交前缀，依序补齐结算、结果、父引用和回执。重试 SHALL 保留原 immutable event identity 与 payload；ACK 不明不得解释为未写入。writer 失效后 SHALL 先满足完整 writer drain、重新取得独占 lease 并校验记录，才执行后续写入。普通观察不得隐式触发修复。

#### Scenario: Commit succeeds but acknowledgment is lost
- **WHEN** 任一收尾事实可能已写入但调用方未取得 ACK
- **THEN** 系统核对或 exact 重放该事实并完成所需同步屏障，不追加重复事实、不改变原 outcome，也不重发 provider 或工具。

#### Scenario: Space becomes available after writer failure
- **WHEN** StorageFull 使 writer 失效，随后磁盘空间恢复
- **THEN** 只有旧 actor/persistence owner 结束且 lease 释放后，新 owner 才能校验原记录并继续收尾；旧 handle 不被重新视为有效 writer。

#### Scenario: Actor exited while persistence still owns storage
- **WHEN** actor 已退出但 persistence task 或已派发 I/O 尚未完成
- **THEN** 替代 writer 仍被拒绝，等待者可得到待确认状态，原写入所有权不会因超时被清除。

#### Scenario: Parent terminal already exists
- **WHEN** child result 与 parent Ended 已通过严格链接校验，但回执或展示投影缺失
- **THEN** 恢复只补缺失部分，保留原终态及一次性的用量累计。

#### Scenario: Conflicting evidence is found
- **WHEN** 同 identity 的 payload、序号或父子链接冲突
- **THEN** 停止自动收尾并保留证据，不覆盖记录或猜测另一份结果。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::reconcile_orphaned_subagents_with_backend/ensure_recovered_child_result_with_opened`、`crates/codegen/shell/src/session/storage/jsonl/mod.rs`、`crates/codegen/chat-state/src/actor/mod.rs`。

### Requirement: Recovered child outcomes preserve evidence and availability

已经验证的 child terminal outcome SHALL 保持不变，正文可用性与交付失败分别报告。只有证明原执行实例已结束、合法 writer 已取得所有权且无有效终态或有效挂起续跑依据时，恢复才可在结算未闭合 scope 后记录 Recovery 来源的 interrupted 结果。存在未消费的挂起意图或节点 checkpoint 时 SHALL 先按挂起协议核对；有效挂起不生成 child terminal，部分挂起证据不足时保持恢复受阻。未知用量 SHALL 保持 incomplete，未知时间不得以整个离线间隔冒充执行耗时。

#### Scenario: Completed child loses its output artifact
- **WHEN** child terminal 及父子身份可验证，但结果正文文件缺失或校验失败
- **THEN** 已确认 outcome 仍可只读展示，并明确正文不可用；不得改写成新的失败 outcome 或拼凑正文交付。

#### Scenario: Inspection failed while child may still run
- **WHEN** backend 不可达或超时，无法确认旧实例已退出
- **THEN** 保持收尾待确认，不生成 interrupted/cancelled 结果，也不抢占 writer。

#### Scenario: Cold recovery finds an interrupted child
- **WHEN** 新 writer 验证原实例已结束，没有有效 child terminal 或尚可消费的挂起续跑依据，且还有未闭合执行或结算
- **THEN** 先按既有结算规则恢复这些 scope，再记录明确的恢复中断来源及用量下界；不伪造用户取消或零消费。

#### Scenario: Cold recovery finds a suspended child
- **WHEN** 新 writer 找到与 root 挂起操作匹配的 child 续跑位置
- **THEN** 分流到挂起核对与同身份恢复，不由孤立 child 修复生成 interrupted terminal。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::result_from_inspection/finish_from_durable_facts/reconcile_orphaned_subagents_with_backend`、`crates/codegen/chat-state/src/timeline.rs::SubagentResultEvent`。

### Requirement: Finalization wait deadlines preserve durable delivery ownership

单次收尾推进与回执交接的 caller 等待 SHALL 默认总计不超过三十秒，回执每次 ACK 等待不超过五秒，调用方更早期限优先。收尾写入重试 SHALL 共享本周期绝对期限，期限到达后不再派发新的 append。等待超时 SHALL 返回提交/交付未确认而非虚构执行失败，且不取消仍需持有的写入所有权。未确认操作仍在途时不得叠加 mutation 重试；明确可重试的回执尝试每周期至多三次。迟到结果 SHALL 通过原身份的持久回执交接，每个 tool call 只提交一份 tool_result。

#### Scenario: Receipt producer remains alive without replying
- **WHEN** 回执 ACK sender 始终存活但不回复
- **THEN** caller 在期限内退出等待并报告交付未确认，原操作继续受 owner 管理，不因 sender 存活无限挂起 caller。

#### Scenario: Caller already received a pending result
- **WHEN** 等待工具已结束原 tool call，之后 canonical terminal 和回执提交成功
- **THEN** 结果经既有通知交付，不给原 call id 再提交第二份 tool_result，回执消费按原 identity 去重。

#### Scenario: Transient failures exhaust the recovery period
- **WHEN** 收尾持久化持续返回可重试错误直至本周期期限
- **THEN** 不再派发新的重试，返回待修复状态并保留责任；重连不重新起表，在途 I/O 的所有权保持到其实际结束。

#### Scenario: Parent writer also fails
- **WHEN** 父会话无法持久接纳 tool_result 或回执
- **THEN** 显示未确认交付并保留恢复事实，不把内存发送成功声称为持久接纳。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::admit_notification_receipt`、`crates/codegen/shell/src/session/actor/completion_delivery.rs`、`notification_drain.rs`。

### Requirement: Recovery dependencies constrain automatic resource reclamation

自动 session、artifact、worktree 及 snapshot 回收 SHALL 保留未闭合子任务链、未确认输入和未完成交付所需的可验证资源。清理资格 SHALL 由持久身份、引用及实际 owner 规则决定，不仅由 TTL、最近 activity、runtime handle 或 completed cache 决定。无需通知的 child 及已持久消费/按规则 dismissed 的回执 SHALL 按各自事实判定，不人为制造永久交付依赖。

#### Scenario: Inactive child has an unresolved parent spawn
- **WHEN** 旧 writer 已退出且 TTL 已到，但原 child/parent 结果链尚未闭合
- **THEN** 自动清理保留恢复所需 session、结果和工作资源，并报告保留原因。

#### Scenario: Parent terminal exists but delivery is pending
- **WHEN** parent Ended 已提交，但对应结果尚需回执或消费
- **THEN** 清理保留尚需读取的结果及关联证据，不能因任务已显示完成就删除。

#### Scenario: Worktree snapshot failed on a full disk
- **WHEN** 含修改的工作树未取得可验证持久 snapshot 及必要引用
- **THEN** 原工作树保持可恢复，正常收尾和自动 GC 都不能把快照尝试当作保全成功。

#### Scenario: Internal child needs no completion notification
- **WHEN** 原 spawn 明确不产生模型完成回执且其其他生命周期义务已确认
- **THEN** 不因不存在本就无需创建的回执而保留无效交付依赖，仍需满足其他清理条件。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs::cleanup_stale_sessions_sync`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/workspace/src/worktree/mod.rs`、`crates/codegen/fast-worktree/src/api.rs`。

### Requirement: Destructive maintenance revalidates recovery references at deletion

自动维护 SHALL 在实际删除的所有权边界内重新核对同一资源实体、引用和可回收资格，并与相关发布/写者操作串行化。扫描缓存、临时路径相同或无 live handle 不构成删除许可。无法取得所需所有权、校验失败、扫描超预算或缺少上层恢复证据时 SHALL 保留该候选；仍在运行的读取或删除不得因 caller 超时而被重复派发。

#### Scenario: New reference appears after candidate scanning
- **WHEN** 资源曾被扫描为候选，但在取得删除所有权前产生了应保留的引用
- **THEN** 删除边界的重新核对发现引用并保留原资源，不依据旧扫描结果删除。

#### Scenario: Standalone worktree GC lacks session authority
- **WHEN** 独立 GC 遇到任务关联 worktree 或其 snapshot，但不能验证相应恢复义务
- **THEN** 普通自动清理跳过该资源，不以年龄或进程消失替代资格证明。

#### Scenario: Recovery dependency scan cannot complete
- **WHEN** 所需记录不可读、损坏或扫描达到预算
- **THEN** 报告本轮资格未确认并保留候选，未扫描的引用不被解释为不存在。

#### Scenario: Fresh pristine worktree was never admitted
- **WHEN** 创建失败有明确证据表明新 pristine worktree 尚未发布 spawn、未交给执行器且没有引用
- **THEN** 允许既有回滚清理；ACK 不明或包含用户修改的资源不能使用此例外。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs::delete_if_still_stale`、`crates/codegen/shell/src/session/worktree.rs::cleanup_worktree_on_failure`、`crates/codegen/workspace/src/worktree/mod.rs::run_auto_gc_best_effort`、`crates/codegen/shell/src/session/input_inbox.rs`。

### Requirement: Confirmed storage failure bounds new execution admission

已确认使持久写入不可用的故障 SHALL 关闭受影响 owner，或当前运行管理实例内已确认共享故障存储域的新执行准入，包含 prompt promotion、child spawn 和后续 provider/tool 派发。故障范围 SHALL 根据来源与存储身份确认，不能仅凭 errno 影响无关域或声称控制其他独立进程。诊断、取消及合法收尾恢复仍可执行。准入恢复 SHALL 确认有效 writer 的持久屏障，并继续受既有 Goal/权限/预算约束；剩余磁盘字节变化本身不构成恢复成功。运行中的外部副作用不得被声称已经撤回。

#### Scenario: Shared session storage reports no space
- **WHEN** 写入确认共享 Grow 存储域空间或配额耗尽
- **THEN** 当前管理实例内该域不继续准入依赖持久化的新工作，已有任务保持可观察和可请求取消，收尾责任仍被保留。

#### Scenario: Failure belongs to a single owner
- **WHEN** 一个 session 的权限或完整性故障没有证据影响其他存储 owner
- **THEN** 关闭该 owner 的准入，不无依据关闭无关域。

#### Scenario: Space recovers but old writer remains invalid
- **WHEN** 磁盘空间恢复而旧 writer 已失效或尚未退出
- **THEN** 不重开执行；合法 writer 恢复并确认持久屏障后才重新评估准入。

证据与实施入口：`crates/codegen/chat-state/src/persistence.rs::persistence_error_is_permanent`、`crates/codegen/shell/src/session/actor/run_loop.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`；此处新增故障准入契约，不替代既有 Goal 准入权威。

### Requirement: Linked session history is reclaimed as one durable unit

Automatic TTL cleanup SHALL treat sessions connected by a parent/child result reference as one reclaim component. It SHALL reclaim the component only when every member is independently stale and all parent spawn, child seed, child result, parent terminal, delivery receipt, and local recovery obligations are proven complete while exclusive writer ownership is held for every member. If any member is fresh, active, missing, unreadable, changed, unresolved, or over budget, cleanup SHALL retain the whole component.

Before removing any member, cleanup SHALL durably record the exact validated component and SHALL resume that committed reclaim after restart. An unresolved or unprovable reclaim intent SHALL block admission or deletion of its listed sessions and SHALL retain the evidence for repair. Per-session quarantine alone SHALL NOT be treated as group-atomic.

#### Scenario: Both members of a closed chain are stale
- **WHEN** a parent and all linked child sessions are stale, no writer is active, and their exact result and delivery chain is complete
- **THEN** automatic cleanup durably decides the whole component and removes every member, resuming the same decision after a crash if needed.

#### Scenario: One member of a closed chain is still fresh
- **WHEN** one session in a closed parent/child component is fresh while another member exceeds TTL
- **THEN** cleanup retains every member and does not remove only the stale endpoint.

#### Scenario: A writer or source cannot be proven
- **WHEN** a member has a live writer, changed identity, missing or malformed source, unresolved receipt, or exhausted scan budget
- **THEN** cleanup retains the entire component without writing a reclaim decision.

#### Scenario: Cleanup restarts after committing a component decision
- **WHEN** process interruption occurs after a durable component reclaim intent but before every listed directory is removed
- **THEN** cleanup resumes only that exact intent and does not expose a remaining half-chain as an ordinary session.

### Requirement: Explicit deletion preserves cross-session result sources

An explicit single-session delete SHALL reject a target whose canonical Timeline participates in a cross-session parent/child result chain. The refusal SHALL identify the linked session and explain that single-session deletion would remove recovery evidence. It SHALL NOT recursively delete related sessions.

#### Scenario: Explicit delete targets a linked parent or child
- **WHEN** a user explicitly deletes one session that has a validated cross-session result-chain edge
- **THEN** deletion is refused with the related session identity and no session directory is removed.

#### Scenario: Explicit delete targets an unlinked session
- **WHEN** a user explicitly deletes a session with no cross-session result-chain edge and no unfinished runtime handoff
- **THEN** the existing single-session quarantine and deletion behavior applies.

### Requirement: Human interaction waits have durable attempt boundaries

Timeline SHALL persist typed open and resolution facts for user question, permission, and Plan approval reverse-request attempts. Each fact SHALL identify the original tool call, interaction kind, and unique attempt. The facts SHALL NOT contain question text, user answers, permission option contents, or tool arguments. Timeline SHALL reject duplicate opens, mismatched resolutions, duplicate resolutions, and resolutions without an open. Live dispatch SHALL persist Opened before sending the reverse request and persist a terminal resolution for answer, rejection, cancellation, timeout, transport failure, malformed response, or suspension. A request SHALL NOT be sent when its Opened fact is unconfirmed. Cold recovery SHALL leave an unclosed historical attempt unknown rather than assigning it a synthetic resolution time. A rebound request SHALL use a fresh attempt identity.

#### Scenario: Human question is answered

- **WHEN** a user question reverse request is sent and answered
- **THEN** Timeline contains one matching Opened/Resolved pair and exposes the interval from their recorded event times without persisting question or answer content in those facts.

#### Scenario: Permission is decided automatically

- **WHEN** local permission policy grants or denies a tool without sending a human reverse request
- **THEN** Timeline creates no human-wait interval for that decision.

#### Scenario: Request cannot be sent or resolved durably

- **WHEN** Opened cannot be confirmed, or a request returns but its matching Resolved append cannot be confirmed
- **THEN** the first case does not send the reverse request; the second remains unresolved/unknown and is not projected as a completed wait.

#### Scenario: Restore rebinds an unanswered request

- **WHEN** a suspended unanswered request is reissued after restoration
- **THEN** the old attempt is closed at the durable suspension boundary and the new reverse request uses a distinct attempt identity; the offline interval is not counted as human response time.

### Requirement: Activity excludes validated suspension offline intervals

Replay activity SHALL exclude time between a validated `TreeSuspended` event and its matching `ExecutionReleased` event for the same operation. `Resumed` alone SHALL NOT end the offline interval. A missing, mismatched, or reversed pair SHALL remain unknown and SHALL NOT be converted into confirmed idle or execution time. Other operation spans that overlap the offline interval SHALL remain protected. Recovered terminal timestamps SHALL continue to use explicit recovery-gap treatment rather than representing the full offline interval as active work. After idle, recovery, and offline ranges are split around one another, only final fragments strictly longer than 30 seconds SHALL be compressed; shorter fragments SHALL retain their recorded duration.

#### Scenario: Runtime resumes after suspension

- **WHEN** an open tool/request spans a valid tree suspension and later execution release
- **THEN** the suspension-to-release interval is excluded from protected activity while pre-suspend and post-release work remains protected.

#### Scenario: Another operation remains active

- **WHEN** one node is suspended while a separate captured operation remains active outside that suspension owner
- **THEN** that operation's known activity remains protected.

#### Scenario: Restore commit is not release

- **WHEN** `Resumed` is durable but `ExecutionReleased` is absent
- **THEN** the runtime remains treated as offline/closed through the captured frontier.

#### Scenario: Short fragments around a long gap

- **WHEN** verified offline ranges split a longer candidate gap into pieces shorter than or equal to 30 seconds
- **THEN** those short pieces remain at recorded speed, only longer final fragments are compressed, and source-to-playback mapping remains monotone.

### Requirement: Session tree suspension preserves logical work

会话 SHALL 将主动挂起记录为独立于完成、取消和失败的生命周期，覆盖指定 root 拥有的全部未结束递归 child、前后台工作和等待关联。挂起 SHALL 保留逻辑 session/task/prompt 身份、已提交进度、模型路线、工作区与结果交付责任；不得仅因运行实例退出生成 child cancelled/completed 结果。真实完成或已明确取消的任务 SHALL 保留原终态。

#### Scenario: Suspend a root with mixed child states
- **WHEN** root 等待两个 child，一个已经完成，另一个仍在执行且拥有更深层 child
- **THEN** 完成结果保持原值，未结束子树进入挂起，原父子等待关联保留；不会把挂起转换成取消或重新委派。

#### Scenario: Spawn competes with suspension
- **WHEN** 挂起关闭准入时另一个 child spawn 已进入提交或 ACK 不明
- **THEN** 挂起依据原身份核对该 spawn，纳入所有已准入 child；未准入者不启动，未知者不会被忽略后声称整树挂起成功。

#### Scenario: Child finishes during suspension
- **WHEN** child 的真实结果与挂起请求竞争
- **THEN** 已验证完成结果与待交付责任优先保留，不覆盖为 Suspended，也不重复结算结果和用量。

目标入口：`crates/codegen/shell/src/session/actor/run_loop.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`、`crates/codegen/chat-state/src/timeline.rs`。

### Requirement: Suspension acknowledges a durable tree boundary

挂起 SHALL 在停止后继执行准入后核对参与节点及其持久续跑位置，只在全部必要事实确认后报告 Suspended。重复请求 SHALL 复用同一次挂起身份。ACK 丢失、写入失败、节点不可达或等待期限到达 SHALL 报告未确认，保留恢复依据和仍在途的写入所有权，不推断未写入或虚构成功。部分挂起 SHALL 不自动重启已经停住的节点。

#### Scenario: All participating nodes acknowledge
- **WHEN** root 和所有参与 child 的续跑位置、结算与依赖引用均已持久确认
- **THEN** root 可确认整树 Suspended，资源关闭沿原 writer drain 规则完成，随后退出。

#### Scenario: Checkpoint requires command settlement evidence
- **WHEN** 节点准备提交 checkpoint，且挂起 operation 尚未记录命令结算
- **THEN** Timeline 拒绝 checkpoint；节点先为同一 operation 持久记录一次 CommandsSettled（没有活跃命令时也记录空集合），相同内容的 ACK 重试幂等，冲突内容或不同 operation 被拒绝。

#### Scenario: Command runtime state was retired after settlement
- **WHEN** CommandsSettled 已持久确认，命令 runner 已退役或其完成缓存已淘汰，但 checkpoint 写入或 ACK 需要重试
- **THEN** 从同一 operation 的持久结算复用原 call/command 身份、完成或中断证据、后台属性与有界输出；不再次终止命令，不依赖已退役的内存缓存，不将已有证据改为空集合或 Unknown。

#### Scenario: Background command settlement reaches a durable checkpoint

- **WHEN** 挂起已持久记录一个后台命令的 CommandsSettled，且该命令的完成通知尚未接纳
- **THEN** 挂起 SHALL 以 Timeline 接受的终态通知身份持久接纳该结果，再提交节点 checkpoint；相同 operation 的重试 SHALL 复用命令结算和通知，不重新运行命令或写入第二个逻辑通知。

#### Scenario: Root acknowledgment is lost
- **WHEN** 所有节点已保存，但 root 挂起提交的 ACK 丢失
- **THEN** 重试或重启核对同一挂起事实，不追加另一份逻辑任务，不重新执行已保存的工作。

#### Scenario: Process exits during partial suspension
- **WHEN** 挂起意图已提交但只有部分节点保存后进程退出
- **THEN** 恢复先核对原节点与 writer，补齐可验证的挂起事实；无法证明续跑位置的节点保持恢复受阻，不将整树当作完整 checkpoint 自动启动。

#### Scenario: Checkpoint binds the exact runtime source
- **WHEN** SessionActor 持久化运行时 checkpoint
- **THEN** checkpoint 包含工作区、Agent 定义及其跳过序列化的能力源、selector/plugin、role prompt 与模型路线的 64 位十六进制 source hash；原始配置、凭据与 prompt 正文不进入 checkpoint，source hash 格式无效时 Timeline 拒绝写入。

目标入口：`crates/codegen/shell/src/session/timeline_persistence.rs`、`crates/codegen/shell/src/session/actor/spawn.rs`；复用 `session-timeline` 的完整 writer drain、exact ACK 和收尾核对契约。

### Requirement: Suspended trees resume the original identities automatically

用户通过执行型 resume 打开已挂起 root 时，系统 SHALL 先验证并重建原树的全部未结束节点、运行配置、工作区、等待与交付关联，再允许可运行节点继续。不得通过新 child identity、`resume_from` 派生任务或重新接纳原用户 prompt 代替恢复。重复或并发 resume SHALL 不启动重叠 writer、重复 child 或重复结果交付。只读查询、export、replay 和历史 child 查看 SHALL 不触发执行。

#### Scenario: Resume the suspended tree
- **WHEN** 用户恢复已确认挂起的 root，且全部必要来源与运行环境有效
- **THEN** 未结束 child 自动以原逻辑身份继续，完成 child 不再执行，父 Agent 按原依赖继续等待或消费原结果，无需模型重新规划或用户逐个恢复。

#### Scenario: A required node cannot be restored
- **WHEN** child 来源、运行路线、工作区或恢复 artifact 缺失、不匹配，或旧 writer 尚未退出
- **THEN** 整树保持执行准入关闭并指出受阻节点与原因；不静默换模型、创建空白 child 或先启动部分树。

#### Scenario: Resume is repeated after work has begun
- **WHEN** 同一挂起状态的 resume 请求重复、ACK 丢失或两个客户端并发恢复
- **THEN** 复用已接纳的恢复实例或返回其状态，不再次创建执行、消费输入或发出完成通知。

#### Scenario: A resumed runtime crashes without another suspension
- **WHEN** 挂起恢复资格已消费，节点已重新运行，随后无新的挂起事实就异常退出
- **THEN** 根据最新执行证据走既有异常恢复，不回退旧 checkpoint 或重发已派发工作；旧挂起记录不授予永久自动重启资格。

目标入口：`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs::load_session`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`。

### Requirement: Suspended operations retain interruption evidence

快速挂起 SHALL 请求中止活跃工具和后台命令，不等待其业务自然完成。操作结果 SHALL 区分已确认完成、确认终止但未完成的 Interrupted、无法确定执行或副作用的 OutcomeUnknown，以及确认尚未派发。取消信号、超时或断线本身 SHALL NOT 证明操作已终止或未产生副作用。命令中断 SHALL 不终结其所属 Agent 任务。

#### Scenario: Local command termination is confirmed
- **WHEN** 挂起中确认原命令及受控进程组停止，但命令没有正常完成
- **THEN** 保存原调用身份、可得输出、终止证据及 Interrupted 结果；所属 child 挂起并可继续，不伪造业务成功。

#### Scenario: Remote operation has an unknown outcome
- **WHEN** MCP、远程命令或脱离控制的进程无法确认停止或副作用结果
- **THEN** 保存 OutcomeUnknown 及可核对的原操作信息，明确外部工作可能仍在运行；挂起仅确认 Grow 自身停止推进，不声称外部世界已冻结。

#### Scenario: Resume encounters an interrupted or unknown call
- **WHEN** 恢复后 Agent 继续处理被中断或结果未知的操作
- **THEN** 获得对应事实并检查现场；恢复机制不自动重发原调用，也不以“继续”提示指示盲重试。确认中断后的新调用遵循正常授权；未知副作用先核对或取得明确处置决定。

#### Scenario: A batch contains completed and undispatched calls
- **WHEN** 同一批工具已有完成结果，另一些调用可证明尚未派发
- **THEN** 复用已保存结果，保留未派发调用及原身份，经当前权限检查后继续；不重新执行整个批次。

目标入口：`crates/codegen/shell/src/session/actor/tool/{mod,dispatch,result}.rs`、命令/task runner 与现有 MCP transport 取消边界。

### Requirement: Suspended waits and deliveries remain attributable

父子等待、人工问题、待交付结果与已挂起的调用 SHALL 保留稳定关联。恢复 SHALL 重建实际等待，不把临时 Future、channel 或 waiter 消失解释为任务完成。每个 tool call SHALL 至多提交一份 tool_result；已提交中断或未知结果的调用，其迟到事实 SHALL 经既有通知或观测路径交付。

#### Scenario: Parent is waiting for a suspended child
- **WHEN** 父调用正在等待 child，而 child 达到挂起位置
- **THEN** 父调用保存对原 child 的等待，不要求 child 完成才能挂起；恢复后重新绑定该 child，不收到虚构取消结果。

#### Scenario: Result was committed before delivery
- **WHEN** child result 已确认而父回执尚未完成时发生挂起
- **THEN** 恢复只补原身份的交付，保留一次性的用量累计，不重新运行 child。

#### Scenario: A late result follows an unknown tool result
- **WHEN** 原 call 已提交 OutcomeUnknown tool_result，后续取得可验证最终事实
- **THEN** 保留原未知事实并追加可归属的新事实，不为同一 call 写入第二份 tool_result。

目标入口：`crates/codegen/shell/src/session/actor/completion_delivery.rs`、`notification_drain.rs`、`crates/codegen/shell/src/agent/subagent/mod.rs`。

### Requirement: Suspended recovery references protect their resources

未恢复或恢复受阻的挂起事实 SHALL 保护必要的 child 存储、worktree、脚本、prompt/result artifact 和交付引用；运行实例退出或完成缓存淘汰 SHALL NOT 授权清理这些依赖。fork、导入副本或 rewind SHALL NOT 复制原树的执行所有权，只有原树的合法 writer 可消费其挂起恢复资格。

#### Scenario: Cleanup sees suspended work
- **WHEN** 自动 TTL、任务缓存淘汰或工作区清理遇到仍被挂起树引用的资源
- **THEN** 保留该资源或报告清理受阻，不破坏原任务的恢复依据。

#### Scenario: History is copied or rewound
- **WHEN** 用户 fork、导入或回退包含挂起记录的历史
- **THEN** 复制的历史不启动原 child；会破坏当前挂起引用的 destructive 操作须沿现有删除/所有权核对拒绝或先明确终止原工作。

目标入口：`crates/codegen/shell/src/session/storage/`、子 Agent worktree 清理及 `session-timeline` 的恢复引用删除护栏。

### Requirement: Planning changes durably own their delivery obligations

局部计划 SHALL 由 SessionActor 经 Timeline 提交，TODO Resources、模型和 UI SHALL 只读取其投影。产生重规划的来源提交 SHALL 同时包含可重建的目标交付责任；跨 session 投递经既有 inbox 与 reconciliation 推进，不要求跨 session 原子事务。ACK 不明 SHALL 核对原 identity，不重复来源修改或副作用。

#### Scenario: ST-01 Source commits before delivery fails
- **WHEN** 父项变化已持久提交但投递前进程退出，或 child inbox 暂不可用
- **THEN** 合法 writer 恢复后可核对原责任并补投递；不会丢失变更或重新提交同一父项修改。

#### Scenario: ST-02 Source write fails or acknowledgment is unknown
- **WHEN** 来源写入失败或 ACK 不明
- **THEN** 不先发布成功投影或执行依赖该变更的通知；明确失败可重试，未知状态先核对原事实，不能另造 identity。

#### Scenario: ST-03 Resources or UI contains stale data
- **WHEN** Resources、UI 缓存与已验证 Timeline 计划不同
- **THEN** 从 Timeline 重建投影，缓存不能覆盖计划或重新产生变更事件。

证据与实施入口：`crates/codegen/chat-state/src/timeline.rs`、`crates/codegen/tools/src/registry/types.rs`、`crates/codegen/tools/src/persistence.rs`、`crates/codegen/shell/src/session/actor`。

### Requirement: Replanning receipt consumption and handling remain distinct

重规划 SHALL 复用 agent 消息的 durable Received/Consumed 与准确 context 提交；Handled SHALL 是 child 对指定来源集合的独立持久事实，与该次局部计划修改或 no-change 决定一起提交。恢复 SHALL NOT 把已消费消息重新消费/注入，而应保留尚未处理的工作责任；只有合法同身份冻结恢复可以继续原检查，终态执行则明确记录未应用。处理确认与父级交接 SHALL 可按原身份核对。

#### Scenario: ST-04 Consumed update has no handling acknowledgment
- **WHEN** child 已消费更新，但在提交局部计划/处理确认前完成了可验证的同身份冻结
- **THEN** 已消费消息仍只出现一次，未处理责任保留；合法树恢复通过当前工作投影继续检查，不伪造 Handled。

#### Scenario: ST-05 Handling commits but parent misses acknowledgment
- **WHEN** child 已持久确认处理，父级未收到 ACK
- **THEN** 核对原确认收敛义务，不要求 child 重改 TODO；迟到旧确认不能清除新变化。

#### Scenario: Child and parent plans advance independently
- **WHEN** child 的 Handled 回执引用其实际局部计划版本，该版本大于父计划的当前版本
- **THEN** 父级按原委派和来源身份接纳该回执，不比较两个 owner 的版本计数；child 自己仍拒绝引用尚不存在的本地版本，较新的父级变更继续待处理。

#### Scenario: ST-14 Received batch is followed by a newer update
- **WHEN** A 已 durable Received 但尚未 Consumed，B 在消费边界之前或之后到达
- **THEN** A 的正文和身份不变；边界前可按顺序消费 A/B 并一次确认覆盖二者，边界后只能先确认 A、保留 B 待处理，不能用发送方失效标记跳过接收方事实。

#### Scenario: ST-15 Unhandled update meets a verified terminal child
- **WHEN** 已消费未处理的 child 正常完成/取消，或普通崩溃恢复验证旧实例结束并记录 interrupted
- **THEN** 原消息不重复消费，变更记录为未应用并交父级决定，不复活 child；旧实例状态尚不明时保持未确认，不能提前制造终态来关闭责任。

证据与实施入口：`crates/codegen/shell/src/session/actor/notification_drain.rs`、`crates/codegen/chat-state/src/timeline.rs` 的通知消费事实。既有 “Agent opinion consumption preserves source and exact context” 继续适用。

### Requirement: Delegated task results reach their direct parent once

新接纳的普通 task 委派 SHALL 为直接父 session 保存可恢复结果交付责任，独立于 root lifecycle owner 与 Task/Goal owner。嵌套委派 SHALL NOT 仅因 root 重挂接或展示抑制而跳过直接父级完成回执。等待工具与通知 SHALL 使用同一 canonical result/receipt，最多一次逻辑模型消费，不重复 tool_result。child 最终结果 SHALL 保留本地交付的有界验收摘要/引用与未闭合必需项，不复制整棵后代结果树。Workflow journal 自有交付和真正无模型消费者的内部执行保持原协议；历史无回执记录不凭空生成新工作。

#### Scenario: ST-06 Nested result goes to the delegating child
- **WHEN** grandchild 完成，其 lifecycle spawn 存于 root，直接委派者为 child
- **THEN** child 获得原结果回执并可验收，root 保留生命周期观察但不收到第二份需消费的模型交付。

#### Scenario: ST-07 Wait completion races with steering or notification
- **WHEN** task_output 等待被用户/父指导打断，child 随后完成且通知并发到达
- **THEN** 原 tool call 至多一份 tool_result；结果按同一回执消费一次，迟到完成不丢失且不重复唤醒。

#### Scenario: ST-08 Result finalization or delivery is unavailable
- **WHEN** child 已返回，但结果 artifact、严格链接或直接父级 durable 接纳尚未确认
- **THEN** 分别显示提交/交付未确认或正文不可用，不把执行返回当作已交付，也不改写已验证终态。

#### Scenario: ST-16 Parent result summarizes its local deliverable decisions
- **WHEN** child 验收自己委派的 grandchild 并产生自身最终结果，或提前结束仍留有必需子交付
- **THEN** child 的 canonical result 保留可验证局部验收摘要/引用及未闭合项，供直接父级审计；不为 root 再生成一份 grandchild 模型交付，也不赋予 child Goal 修改权。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`、`crates/codegen/shell/src/agent/subagent/{mod.rs,handle_request.rs}`、`crates/codegen/shell/src/session/actor/completion_delivery.rs`。

### Requirement: Planning restoration preserves authority and unresolved work

计划、关联、等待、待处理重规划与未验收结果 SHALL 从持久事实恢复。与 `suspend-session-tree` 联合时 SHALL 在开放执行前恢复原树身份与依赖，遵循原预算、权限及业务暂停状态；Interrupted/OutcomeUnknown SHALL 进入检查而非工具重放。无冻结恢复权威的冷崩溃和历史 fork/rewind/export/replay SHALL NOT 因计划未完成而自动恢复执行。

#### Scenario: ST-09 Frozen tree contains both updates and interrupted commands
- **WHEN** 合法树级冻结保存了 child 等待、未处理父项变化和中断/未知命令，随后用户 resume
- **THEN** 先恢复同身份树及原责任，再在允许准入的节点检查最新要求和命令现场；不新建替代 child 或自动重复副作用。

#### Scenario: ST-10 Suspended child cannot currently receive a message
- **WHEN** 父项已提交变化而目标 child 挂起/正在恢复，原 inbox 不接纳新消息
- **THEN** 来源责任保留，合法恢复后投递；不为送达消息绕过暂停、用户/权限等待或预算 gate。

#### Scenario: ST-11 Historical state does not resume old work
- **WHEN** 普通崩溃没有有效冻结 checkpoint，或用户 fork、rewind、resume_from、导出及播放历史
- **THEN** 按原恢复/历史规则处理；旧 TODO、等待和 transcript 不能授权自动重启 child 或命令，新委派需新的当前 owner 校验。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs` 的 orphan reconciliation、`crates/codegen/shell/src/session/storage`、`crates/codegen/chat-state/src/timeline.rs`；冻结协议遵循本规范中的树挂起与恢复条款，本条不替代它。

### Requirement: Planning obligations participate in bounded retention

未闭合通知、处理确认、有效等待和必需结果验收 SHALL 保留所需身份与证据，复用现有回收规则；已处理、被合并覆盖并确认、明确取消或终态无法应用的责任 SHALL 可持久收敛。并发交接与内存排队 SHALL 有界，容量或存储失败 SHALL 可见且不得丢失已提交责任。

#### Scenario: ST-12 Cleanup overlaps pending planning work
- **WHEN** TTL 到期但变更交付、处理确认或结果验收仍需某 artifact/session
- **THEN** 必要资源保留；责任收敛后可按原清理条件回收，不靠永久保留全部历史解决交付。

#### Scenario: ST-13 Delivery capacity is exhausted
- **WHEN** 待交付内存或存储达到边界
- **THEN** 保留已提交来源责任并有界反压/重试，新接纳失败明确返回；不无限排模型轮次或将未写入变更声称成功。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs` 的清理、原 inbox 与 subagent reconciliation。

### Requirement: Retained plan references and decisions are durable facts
计划候选、其当前资格及影响执行的决定 SHALL 由现有 Timeline/Control owner 持久记录并引用 immutable artifact。稳定决定重试 SHALL 核对原接纳/交接状态，不生成新的执行意图；未知 ACK 不构成未执行证据。feedback SHALL 只消费一次，不附加普通 HumanIntent 副本。

决定身份 SHALL 绑定不可变 action、候选提交和 feedback。同身份同内容 SHALL 返回原状态，同身份不同内容 SHALL 拒绝。只读快照未发现记录 SHALL NOT 单独证明未接纳；负面结论需要原操作明确拒绝或完整旧 writer 退出后合法 owner 的核对证据。

#### Scenario: Approval acknowledgment is lost
- **WHEN** 决定可能已提交但客户端未取得 ACK
- **THEN** 以原身份核对或重试，不重新生成 prompt 或第二次 handoff。

#### Scenario: Two clients decide concurrently
- **WHEN** 不同决定身份对同候选的批准和放弃或两次批准竞争
- **THEN** 仅一个合法决定改变权威状态，另一方得到明确过期/冲突结果。

#### Scenario: Retry changes feedback or decision
- **WHEN** 同一决定身份被用于不同 action、feedback 或候选
- **THEN** 在改变控制状态或消费反馈前返回身份冲突，原决定保持不变。

#### Scenario: Status read overtakes the original decision
- **WHEN** 查询快照未包含仍可能接纳的原决定，或旧 writer 的在途写入尚未退出
- **THEN** 返回未确认，不能授权新决定、新 prompt 或第二次 handoff。

#### Scenario: Process exits at the defer commit boundary
- **WHEN** 候选保留与清除审批 gate 的提交失败、ACK 丢失，或已提交但原工具终态尚未完成时进程退出
- **THEN** 失败/未确认不乐观释放用户内容；恢复核对原决定和工具记录，已确认保留只补合法收尾，不自动审批、实施或重弹已延后的 waiter。

实施入口：`crates/codegen/chat-state/src/timeline.rs`、`shell/src/session/actor/tool/mod.rs::admit_plan_handoff_notification`、既有 Control 提交；新字段/schema 待实现评审。

### Requirement: Retained plan recovery validates artifacts without starting work
读取或恢复展示候选 SHALL 校验引用与 artifact 的大小、hash、编码和可用性，不隐式执行、修复 writer 或从模型历史猜正文。恢复执行 SHALL 另行核对决定、handoff 消费和执行 owner。

#### Scenario: Read a retained plan after restart
- **WHEN** 用户仅打开已保留计划且 artifact 有效
- **THEN** 显示原版本，不生成实施 turn、writer 修复或审批 waiter；明确再次审阅/执行才可申请新审批关系。

#### Scenario: Artifact is missing or corrupt
- **WHEN** 候选引用的正文缺失、超限、hash 不符或编码无效
- **THEN** 明确不可用，不从旧消息拼接并执行替代正文。

实施入口：`shell/src/session/behavior.rs` artifact 读写和 `actor/tool/mod.rs::reconcile_restored_plan_approval`。

### Requirement: Retained plan artifacts participate in existing reclamation checks
有效保留候选及未确认决定/交接所需 artifact 和承载它们的 session 目录 SHALL 在既有回收所有权边界内受引用保护；UI 收起、进程离线或 TTL 本身不解除保护。放弃候选后仍按剩余历史/恢复引用判定，不直接删除正文。扫描后的引用变化 SHALL 在实际删除边界重新核对。

#### Scenario: Retained candidate outlives the cleanup window
- **WHEN** 自动清理扫描到仍有有效候选引用的计划 artifact
- **THEN** 保留正文；只有实际删除边界验证无保护引用时才按既有规则回收。

#### Scenario: Abandoned candidate still has recovery references
- **WHEN** 候选已放弃，但存在历史/未确认交接引用，或扫描后新候选引用已发布
- **THEN** artifact 与所属 session 不被旧 GC 候选列表删除；无法验证引用时保持资源并报告资格未确认。

实施入口：`shell/src/session/storage/jsonl/mod.rs` 和计划 artifact 引用；须与当前 working tree 恢复资源保全契约对齐，不另建 GC owner。

### Requirement: Plan history copying does not transfer execution authority
fork/导入新 session 或 rewind SHALL NOT 复制有效的旧审批关系、重新消费已结算决定或自动启动来源 handoff。可见正文不等于目标会话的执行资格；跨 session 实施 SHALL 以目标新提交和新审批建立关系。无法验证候选来源时 SHALL 拒绝执行，而非从 UI 历史重建授权。

#### Scenario: Fork or import a retained or approved plan
- **WHEN** 计划正文或控制历史复制到不同 session/workspace
- **THEN** 目标不复用来源 waiter/批准/handoff；若使用正文，按目标 owner 新提交并重新审批，不启动原工作。

#### Scenario: Import preserves a Plan execution relation
- **WHEN** `grow/session/import` receives an exact Timeline mirror that still contains a reviewable Deferred/Reviewing Plan candidate, pending approval or unresolved PlanApproval Interaction, AwaitingApproval/Amending/Executing Plan state, a `last_plan_handoff`, or an unconsumed PlanHandoff receipt
- **THEN** import is rejected before publishing the target session, with an explanation that the target owner must create and approve a new Plan submission; an import containing only closed Plan history and no retained execution relation remains valid.

Closed Approved/Abandoned candidate metadata alone does not grant review or execution authority and remains importable when the current state is Normal and there is no handoff or unconsumed receipt.

#### Scenario: Rewind meets a consumed plan decision
- **WHEN** rewind 后旧批准窗口或已消费 handoff 再次出现
- **THEN** 旧操作不能重新获执行资格；候选保留/失效由当前 Control 与来源校验决定，展示回退不复活执行。

实施入口：SessionControl、`shell/src/session/fork.rs`、`actor/rewind.rs` 与现有 Timeline 分支恢复，不建立独立计划历史引擎。

### Requirement: Skill loads persist independent context facts
加载结果 SHALL 关联真实 session、输入引用和工具调用身份，持久保存来源、调用参数、结果及实际正文快照。用户触发 SHALL 在一次权威发布中消费该输入并追加原用户消息及整组 Runtime 来源的独立结果，不改写原始用户正文或伪造模型响应；模型触发 SHALL 复用真实工具结果，不重复注入正文。Loaded SHALL 只由完整正文及上下文贡献的提交确认派生，准备事实本身 SHALL 不进入模型 Surface。

#### Scenario: User-triggered load commits
- **WHEN** 用户触发的加载成功完成
- **THEN** 一次权威发布同时确认输入消费、原始用户消息和有序独立结果，结果关联输入、引用序号及真实 ToolEvent；整组确认后才显示 Loaded，依赖采样还须通过首次消费完整性检查。

#### Scenario: Model-triggered load commits
- **WHEN** 已接纳模型工具调用发起技能加载
- **THEN** 结果关联真实 call identity，正文只作为该次工具结果贡献一次，不增加虚假用户输入或 Runtime 副本。

#### Scenario: Prepared body without durable confirmation
- **WHEN** 正文已准备但提交失败或确认状态不明
- **THEN** 不宣称 Loaded，不放行依赖采样；确定失败按失败处理，未知结果按权威日志对账。

#### Scenario: Crash at the user group publication boundary
- **WHEN** 进程在用户输入与多项技能结果发布期间退出，随后恢复
- **THEN** 权威历史只能确认整组未发布或整组已发布，不出现原输入已进入 Surface 但部分技能缺失的状态；未确认时先对账，不重复消费或执行 Hook。

证据入口：`crates/codegen/chat-state/src/timeline.rs` 的 MessageCause 与事实校验、`crates/codegen/sampling-types/src/conversation.rs` 的 ConversationItem、`crates/codegen/shell/src/session/event_tracker.rs` 的持久工具事件。新增行为不得借用虚假的 sampler response admission。

### Requirement: Runtime skill loading reuses causal tool execution boundaries
用户加载 SHALL 在真实 Runtime Step 中使用现有工具开始、终态与 Hook 因果关系，不需要前置 provider 请求。工具开始 SHALL 先于对应 PreToolUse 持久化，Post Hook SHALL 晚于工具终态确认。加载失败的输入 SHALL 合法结算且不被重新消费；用户加载 SHALL 不伪造 Request、模型响应或用量。

#### Scenario: First user load without a model step
- **WHEN** 首条或队列用户技能输入获准执行且尚无模型步骤
- **THEN** 在所属 turn 的真实 Runtime Step 记录加载工具及合法 Hook cause，结束后再决定是否进入模型步骤；该 Runtime Step 不产生 provider 采样用量。

#### Scenario: Failed FIFO and steering settlement
- **WHEN** 已被 turn 预留的 FIFO 技能输入失败，或未消费的技能插话失败
- **THEN** FIFO 沿未送模型的 Handled 语义结算，插话沿适用的 Dismissed / Invalidated 语义结算并保留错误；不对 reserved 输入直接 Dismissed，不关闭原任务来处理失败插话。

证据入口：`crates/codegen/shell/src/session/actor/tool/dispatch.rs` 的 handle_direct_bash_command、`event_tracker.rs` 的 tool_started、`actor/input_admission.rs` 的 complete_unmodeled_fifo_inputs / dismiss_input_ids，以及 `crates/codegen/chat-state/src/timeline.rs` 的 Hook 和 Input fold（Shell 的相对文件均位于 `crates/codegen/shell/src/session/`）。

### Requirement: Skill load identity makes result delivery idempotent
相同加载 identity 与相同 payload 的重复提交 SHALL 返回原结果，不重新读取正文或执行 Hook；相同 identity 的冲突 payload SHALL 拒绝。再次主动调用 SHALL 使用新的输入或模型调用身份。提交确认不明 SHALL 先对账，不以新执行猜测旧结果。

#### Scenario: Acknowledgement lost after commit
- **WHEN** 已提交加载结果的确认丢失，调用方重送同一 identity 与 payload
- **THEN** 返回原正文快照和结果，不重复加载、Hook 或上下文贡献。

#### Scenario: Identity conflict
- **WHEN** 相同加载 identity 携带不同来源或参数
- **THEN** 拒绝冲突，不覆盖原结果。

#### Scenario: Intentional repeat
- **WHEN** 用户再次主动提交相同技能命令
- **THEN** 新输入身份允许新的加载，不因旧历史成功而跳过。

证据入口：`crates/codegen/chat-state/src/actor/mutations.rs` 与 `timeline.rs` 的持久提交、身份校验，以及现有输入 identity 契约；本条只增加加载所需幂等边界。

### Requirement: Skill history replay is observational
恢复、导出导入、fork 与 rewind SHALL 根据持久事实和原正文快照投影加载结果，不执行工具、重跑 Hook 或读取当前技能文件。快照损坏 SHALL 明确失败，不以当前文件代替。加载事实及前提 SHALL 遵守既有历史因果边界，不形成全局已加载状态。

#### Scenario: Source edited after loading
- **WHEN** 技能加载后源文件被修改或删除，随后恢复或导出会话
- **THEN** 已提交记录仍显示原来源、参数和正文快照，不产生新的加载。

#### Scenario: Missing body artifact
- **WHEN** 被持久结果引用的正文快照缺失或损坏
- **THEN** 加载历史明确报告完整性错误，不重新读源文件修补成功记录。

#### Scenario: Fork and rewind
- **WHEN** 从包含加载结果的历史 fork 或 rewind
- **THEN** 仅保留因果边界内的加载事实和上下文贡献，不重新执行它们，也不从被截断历史继承 loaded 标记。

#### Scenario: Incomplete operation during replay
- **WHEN** 历史包含尚未确认终态的加载操作
- **THEN** replay 显示未确认状态，只有取得执行所有权的恢复路径可以对账或继续，纯历史加载不自行重启它。

#### Scenario: Published content withdrawn before first consumption
- **WHEN** 已发布加载在首次关联模型步骤获得已接纳响应前因执行终止而从 Surface 撤下，随后恢复、fork 或导出
- **THEN** 原发布和 Loaded 作为历史保留，当前 Surface 及续跑判断遵守关联到该组的撤下事实，不重新注入正文或改动无关输入 / 工具结果；纯回放不触发执行。

证据入口：`crates/codegen/chat-state/src/timeline.rs`、Shell 会话恢复与 export/fork/rewind 路径，主规范既有只读观察与 Hook 恢复契约。

### Requirement: Cold writer identity recovery is independent of client replay

Cold load SHALL 在发布 writer 或新持久事实前恢复全部 Timeline 展示/领域身份占用与序列下界；noReplay、cursor、rewind 可见性不得降低该下界。派生通知 SHALL 使用稳定来源身份，不依赖已删除的物理 updates 记录；相同身份不同 payload 仍拒绝。

#### Scenario: Headless loads without replay
- **WHEN** cold noReplay load 不向客户端发送历史
- **THEN** 仍恢复全部已占用身份，新持久事实不与历史冲突。

#### Scenario: Historical row is hidden from replay
- **WHEN** rewind 或 cursor 隐藏历史事实
- **THEN** 其合法身份仍参与 writer 身份恢复。

#### Scenario: Display expectation precedes cache publication
- **WHEN** Timeline 已提交事实但进程在通知交付前停止
- **THEN** 该身份仍被占用，恢复派生同一展示而不创建另一个逻辑事实。

### Requirement: Suspension retries reuse canonical completion receipts

挂起 SHALL 复用相同后台任务终态身份已接纳的完成回执，包括已消费回执；仅无回执时提交 CommandsSettled 对应通知。原正文及消费状态 SHALL 保持不变，不能通过放宽异正文身份冲突或新建通知版本绕过重试。

#### Scenario: A normal completion precedes suspension
- **WHEN** 后台任务正常完成回执已接纳，挂起又收集到同一任务结算，且首次挂起失败后按同 operation 重试并恢复
- **THEN** checkpoint 与恢复成功复用原回执及正文，不重复提交或重新消费终态通知。

#### Scenario: Suspension creates the first receipt
- **WHEN** 挂起结算尚无终态回执，通知接纳后 checkpoint 前失败
- **THEN** 同 operation 重试复用该回执，恢复仅交付一次原结果。

### Requirement: Suspension retries retain human wait evidence

挂起 SHALL 从当前周期的持久交互事实和匹配恢复 checkpoint 明确保留的等待保留原调用、等待种类及 attempt。物理 owner 退出或首次挂起超时 SHALL NOT 把未回答等待变成 OutcomeUnknown 或 tool_result。已回答、取消、失败或未被匹配恢复 checkpoint 保留的历史交互 SHALL NOT 被复活。

#### Scenario: Owners drain after the first deadline
- **WHEN** 人工问题、权限或计划审批 owner 在挂起中释放，checkpoint 前首次挂起因期限失败，随后以同 operation 重试
- **THEN** 重试 checkpoint 保留 AwaitUser 或 AwaitPermission 与原调用依赖，原 attempt 合法结算为 Suspended，恢复重建原等待而不伪造结果。

#### Scenario: A wait was already resolved
- **WHEN** Timeline 已记录交互的真实解决，或历史 Suspended 未被匹配恢复 checkpoint 保留
- **THEN** 本次挂起不因旧 Opened 事实恢复该等待，不覆盖真实完成结果。

#### Scenario: Question receiver closes before service shutdown
- **WHEN** 挂起已关闭执行准入，前台 owner 先释放问题接收器，而问题服务 shutdown token 尚未取消
- **THEN** 该交互按 Suspended 结算并保留原等待资格，不能因清理顺序被记录为业务取消；普通非挂起的接收器关闭仍沿原取消语义。

### Requirement: Tree admission freeze has a durable suspension identity
树级挂起 SHALL 在首次 coordinator freeze 副作用前持久确认 root operation。冻结后的初始化超时或 ACK 未确认 SHALL 保留该身份，重试 SHALL 复用它，不允许覆盖不同 owner。

#### Scenario: Initializing child exceeds the freeze deadline
- **WHEN** coordinator 已冻结，但 child 初始化未在期限内完成
- **THEN** root 保留持久挂起身份，重试继续同 operation 并重新核对 child，不发生 conflicting suspension owner。

#### Scenario: Freeze acknowledgement is lost
- **WHEN** coordinator 接纳冻结后 ACK 丢失
- **THEN** 首轮报告未确认，重试向 coordinator 发送相同持久 operation。

### Requirement: Suspension wait evidence spans admission closure
人工等待 SHALL 在当前执行周期内保留因挂起结算的 attempt，即使 Suspended 早于该节点 SuspendRequested。ExecutionReleased SHALL 隔离未被匹配 checkpoint 明确保留为 AwaitUser/AwaitPermission 的历史证据；真实交互解决和工具结果 SHALL 不被旧等待覆盖。

#### Scenario: Child wait settles before node suspension intent
- **WHEN** root 或 child 准入已关闭，问题、权限或计划审批先记为 Suspended，节点随后提交挂起意图，且首次 drain 超时后重试
- **THEN** checkpoint 与冷恢复保留原 AwaitUser/AwaitPermission，不伪造 OutcomeUnknown 或工具结果。

#### Scenario: A previous suspension was released
- **WHEN** 历史 Suspended 之后存在 ExecutionReleased，再进入新的挂起周期
- **THEN** 只延续匹配 checkpoint 明确保留的逻辑等待，其余历史 attempt 不复活；后续新 attempt 与真实解决事实优先。

#### Scenario: Re-suspend before a restored human wait rebinds
- **WHEN** 问题、权限或计划审批的 checkpoint 已恢复并放行，但物理 worker 尚未建立新 attempt，此时再次挂起
- **THEN** 新 checkpoint 保留原逻辑等待且无伪造工具结果，不将其转为 OutcomeUnknown；真实回答或工具结果不得被旧 checkpoint 覆盖。

### Requirement: Timeline wire decoding rejects ambiguous typed fields

Timeline 解码 SHALL 保留现有 canonical wire、任意合法字段顺序与 JSON Value 调用入口，并拒绝未知 envelope/event 字段和重复的 typed envelope/event 字段。不得通过先构造通用 Value、以 last-wins 覆盖重复字段来接受歧义事件。版本不兼容仍 SHALL 在存储读取边界先报告准确 persisted/current schema，不以解析未来 payload 的错误掩盖版本错误。

#### Scenario: Valid fields arrive in another order
- **WHEN** 合法 Timeline 记录的 event、type 与公共字段重新排列，或调用方从 JSON Value 解码
- **THEN** 得到与 canonical 编码一致的事件，恢复结果不变。

#### Scenario: Typed field repeats or an unknown field appears
- **WHEN** 原始记录重复 typed envelope/event 字段，或带未知 envelope/event 字段
- **THEN** 解码明确拒绝，不覆盖前一个值或发布部分恢复状态。

#### Scenario: A future schema has an unknown shape
- **WHEN** 存储记录的 schema 与当前版本不兼容且 payload 缺失或是未知形状
- **THEN** 先返回 persisted/current schema mismatch，仍拒绝恢复。

### Requirement: Timeline owns reconstructable durable history

Timeline SHALL 保存完整重建历史所需的 typed 事实或不可变来源引用、稳定展示身份与顺序依据；同一 canonical 正文不得为 replay 再持久化一份。现有独立 ACP/Grow 持久展示义务 SHALL 由其 Timeline 事实承担，保留 preview budget、脱敏、原 owner 与准入失败边界；瞬时状态不因本次改造获得永久或执行权威。客户端原始更新不得未经领域校验变成通用 payload 事件。

#### Scenario: Display metadata alone cannot reconstruct a row
- **WHEN** Workflow/Control 或用户 echo 只有 identity/revision/digest，缺少重建所需来源
- **THEN** 新格式写入前必须保留对应版本的事实引用或最小 typed 内容；不可只写悬空 expectation 再依赖 updates 正文。

#### Scenario: Anchor and independent event interleave
- **WHEN** anchor A、独立事实 B、引用 A 的 admission C 依次提交
- **THEN** 展示为 A 位置的 response 后跟 B；C 不把 response 改排到 B 后，discarded A 不产生 accepted 正文。

#### Scenario: Artifact publication succeeds before fact failure
- **WHEN** artifact 已写入但引用事实未确认
- **THEN** 不宣称 admission 成功；允许无引用对象沿原策略回收，不允许已确认事实引用未持久正文。

### Requirement: Stored Timeline facts validate their encoded integrity

新格式 SHALL 为明确版本的事实编码保存 checksum，覆盖 seq、类型、时间和 payload；恢复对原字节校验并执行全部领域验证，不为 checksum 重新序列化整个 response。已有必要 artifact 验证 SHALL 保留；checksum 不替代 exact identity/payload 判定或执行所有权。

#### Scenario: Payload remains valid JSON after modification
- **WHEN** 持久正文被修改，仍可解析但与保存的 checksum 不符
- **THEN** 恢复拒绝该完整记录，不发布部分 Timeline 或修改原文件。

#### Scenario: Incomplete final record or incompatible schema
- **WHEN** 文件含未完成尾片或旧 schema
- **THEN** 尾片沿既有完整行规则处理；不支持的 schema 明确拒绝，不自动重写旧会话或绕过验证。

#### Scenario: An incremental observer reads interleaved physical facts
- **WHEN** Trajectory 或搜索观察读者读取含领域与展示事实的统一物理前缀
- **THEN** 校验每条物理记录与连续位置，只按原 Domain seq 投影领域事件；读取预算覆盖实际物理记录，捕获后追加不可混入，刷新失败不推进已缓存前缀。

### Requirement: Cold child reconciliation settles historical scopes before delivery

Cold reconciliation SHALL 先核对并补齐 child result 与 parent terminal，关闭旧进程遗留的本地 scope，随后才发布可触发当前 actor 工作的恢复展示与完成回执。恢复清理不得关闭本次 owner 新启动的通知回合或 Hook；补发历史 child 展示不得重新执行 SubagentStart Hook。已有 identity、结果引用、独占 lease 与持久确认要求保持。

#### Scenario: Recovered completion starts a notification turn
- **WHEN** orphan child 的结果与 parent terminal 已恢复，完成回执可启动新的通知回合
- **THEN** 旧 scope 的 interrupted 清理先完成，新通知回合不被该清理关闭，session/load 保持有效 owner，parent terminal 仍引用同一 canonical child result。

#### Scenario: Spawn hook completed before its display was committed
- **WHEN** parent Spawn 与对应 SubagentStart Hook 已完成，但进程在持久展示前退出
- **THEN** 恢复补发缺失展示并保留原 Hook occurrence，不再次执行或追加该 Hook。

### Requirement: Unavailable child output cannot establish a new successful result
Recovery SHALL NOT synthesize a successful canonical child result from CompletedOutputUnavailable alone. An existing valid canonical result SHALL preserve its outcome independently of output availability. Without such a result the parent spawn SHALL remain open and no successful terminal or completion delivery SHALL be forged.

#### Scenario: Child exists without a published result
- **WHEN** inspection says CompletedOutputUnavailable and valid child storage contains Seed but no SubagentResult
- **THEN** recovery leaves child and parent terminal facts unchanged and reports result unavailable.

#### Scenario: Existing completed result loses its output
- **WHEN** a valid canonical Completed result exists but its referenced output is missing or corrupt
- **THEN** recovery reuses the exact result reference and outcome without rewriting history or guessing output.

### Requirement: Suspension drains compaction before retiring the foreground
挂起 SHALL 在活动压缩的持久化与结算 owner 退出后收回并 join foreground，不等待已经为 runtime handoff 停驻的 foreground 自行退出。挂起 SHALL 沿用同一 deadline 和 operation 身份；压缩尚未排空时不得提前 abort 或确认 Suspended。

#### Scenario: Active compaction settles into a parked foreground
- **WHEN** 挂起关闭准入并取消活动压缩，压缩完成结算并释放 lease，foreground 随后停驻等待 handoff
- **THEN** 挂起 owner 收回并 join foreground，在原 deadline 内完成 checkpoint，不生成 Turn cancelled/error/completed。

#### Scenario: Compaction acknowledgement exceeds the suspension deadline
- **WHEN** 压缩持久确认仍在途且共享 deadline 已到
- **THEN** 返回未确认并保留压缩任务，不提交 Suspended；ACK 结算后同 operation 重试可以收回 foreground 并完成挂起。

#### Scenario: An admitted manual worker reaches lease acquisition after suspension
- **WHEN** 手动压缩已获得会话活动许可但尚未取得 publication lease，挂起先关闭执行准入
- **THEN** 该 worker 不能取得新压缩 lease；原活动 lease 释放后仍保持关闭，只有恢复执行才重新开放。

### Requirement: Restored children retain their original birth authority

原 suspended child 重建 SHALL 从原 Spawn 的已持久化有效 capability_mode 恢复出生 RWX，并与当前直接父权限上限相交；当前定义默认或已选 profile 不得扩大原生命周期的出生权限及后代 ceiling。恢复事实缺失或无效 SHALL 阻止整树执行准入。逐调用授权仍须按当前策略重新检查，不复用旧 permit。

#### Scenario: A readonly child resumes with an all-capability profile
- **WHEN** 原 Task 未指定 capability override，child 出生为 ReadOnly，当前 profile 默认为 All，整树挂起并销毁后从持久化 checkpoint 恢复
- **THEN** child 保持原身份和 ReadOnly 出生权限，超出出生 RWX 的工具仍需当前授权，后代上限不扩大。

#### Scenario: Persisted birth authority is unavailable
- **WHEN** 原 Spawn 缺失有效 capability_mode
- **THEN** 恢复报告出生权限事实不可用并保持树执行准入关闭，不回退当前 profile 默认。
