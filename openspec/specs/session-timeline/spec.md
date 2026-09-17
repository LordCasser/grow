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
An acknowledged ImageProjection SHALL retain original image payloads alongside their nonempty descriptions in materialized image parts, while advancing existing replacement Surface identities and validating source fingerprints, counts and provenance. Local OCR descriptions SHALL identify their local engine rather than claim a provider Sideband.

#### Scenario: Resume a described image
- **WHEN** a session with a completed image description is replayed
- **THEN** the image and description are both available with consistent causal Surface identities.

#### Scenario: Select request representation
- **WHEN** a known unsupported canonical provider/model pair prepares a request
- **THEN** the request uses available descriptions without mutating the retained image, and unresolved descriptions prevent a lossy retry.

#### Scenario: Switch to an unknown pair
- **WHEN** another canonical provider/model pair prepares its first request
- **THEN** original image parameters remain available even if another pair previously used descriptions.

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

### Requirement: Turn terminals identify their actual authority

新 Turn terminal SHALL 明确区分 provider 响应驱动、宿主控制/错误、用户取消和中断恢复。provider 驱动终态 SHALL 关联相同 Turn 的已完成 request，其原始 provider terminal 由 request/attempt 证据保存。宿主 stop_reason/completion_kind SHALL 仅是本地分类。历史未记录来源的终态 SHALL 保持未知，不据旧字符串猜测或回写。

#### Scenario: Natural response closes the turn
- **WHEN** 完整正常 provider 响应使普通 Turn 收尾
- **THEN** Turn terminal 标识 Provider 来源并关联其 request，可追溯到对应 attempt 的原生终止。

#### Scenario: Host stops after a provider response
- **WHEN** provider 已结束，但宿主因预算、控制、持久化错误或用户取消收尾
- **THEN** 保留原始响应证据，同时追加宿主或用户来源的 Turn terminal，不覆盖原始原因。

#### Scenario: Process recovery closes an open turn
- **WHEN** 重启时发现未闭合生命周期
- **THEN** 追加 Recovery 来源的 interrupted 终态；不伪造 provider 事件或正常成功。

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

Session Timeline SHALL 持久记录每个主模型 attempt 的已知/未知 Usage、每个子 Agent 的最终 Usage 结算，以及 session-level incomplete 事实。冷恢复 SHALL 按结算身份去重重建 lifetime aggregate、provider/model 分项和 Agent 归属；不得把已有消费重置为零或重复计费。已知用量事件 SHALL 在 actor 发布及恢复事件持久化前完成载荷校验；格式损坏或同身份冲突 SHALL 使恢复失败并保留原始记录。

#### Scenario: Restore settled main attempts
- **WHEN** Timeline 含多个已持久化主模型 attempt 结算并创建新的 actor incarnation
- **THEN** 新 actor 在接受后续调用前恢复这些 attempt 的累计 token、model、cost、duration 与 incomplete 状态，每个 attempt 至多计入一次。

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

### Requirement: Cold resume creates a durable usage segment boundary

成功发布新的冷恢复 actor incarnation 前，session SHALL 持久提交一个 Usage resume boundary。恢复投影 SHALL 以初始运行和这些边界切分结算，aggregate SHALL 等于所有 segment 的累计结果；resident client reconnect 不创建新的 actor segment。

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
