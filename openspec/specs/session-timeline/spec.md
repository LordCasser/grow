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

被废弃 attempt 的原始证据、用量及恢复决定 SHALL 保留在既有 Timeline 证据链或其不可变 artifact 引用中，但 SHALL NOT 投影成模型有效上下文、native continuation 或可执行工具。候选只有在会话 durable admission 与其 Timeline-derived replay projection 均确认后才能发布已接纳状态。每个产生 canonical assistant response 事件的新 admission SHALL 在该事件上携带原 sampler request 与最终 attempt 组成的不可变 identity 及确定性 admission result；同 identity、同 response payload 的本地 admission 重放 SHALL 幂等返回原结果，同 identity、不同 payload SHALL fail closed。Replay projection SHALL 以 Timeline response 为 authority 并按 identity、Timeline event、payload digest 和 projection version 幂等提交；投影准备 SHALL 共享 canonical assistant/reasoning 文本存储而不复制整个正文，回放或 fork 外发 ACP 时才可建立其独立展示载荷。接纳或 projection 失败/确认不明 SHALL NOT 触发盲目重新采样，也 SHALL NOT 越过到 continuation、工具或成功 Turn terminal。

#### Scenario: Failed generation is followed by a valid attempt

- **WHEN** 第一次候选被拒收而第二次被持久化接纳
- **THEN** 证据可以追溯两次调用及废弃原因，模型 Surface、accepted UI projection 和可执行工具只包含第二次被接纳结果。

#### Scenario: Admission write acknowledgment is lost

- **WHEN** 响应可能已经写入 Timeline 但提交确认丢失
- **THEN** 系统只按原 admission identity 和原 payload 核对或重放本地 Timeline admission，不启动新 provider 请求；确认仍不明时停止当前 Step/Turn，不提交 replay projection、不发布 Accepted、不执行工具，也不进入 completion recovery。

#### Scenario: Exact admission submission is repeated

- **WHEN** 同一 response admission identity 以完全相同的 canonical response payload 再次提交
- **THEN** 返回原 admission 结果且 Timeline 只保留一份 response；不得重复安装或用历史重建 provider-native continuation，replay projection 仍以同 event/digest 幂等核对。

#### Scenario: Admission identity is reused with a different payload

- **WHEN** 已存在的 response admission identity 被用于不同 canonical response payload
- **THEN** admission 以身份冲突失败，既有 Timeline、Surface 与 replay cache 保持不变，不发布 Accepted 或执行任一 payload 的新工具。

#### Scenario: Replay projection cannot be confirmed

- **WHEN** Timeline response 已确认，但对应 replay projection durable append 失败、冲突或确认不明
- **THEN** response 保留为 canonical Timeline 事实，当前 Step/Turn 在 typed projection boundary fail closed；不发布 Accepted、不进入 completion recovery、不继续 truncation/pause-turn、不启动下一 provider request或工具。

#### Scenario: Projection is reconciled during recovery

- **WHEN** cold/replacement load 发现当前 branch 的 identity-bearing response 缺少 matching projection record
- **THEN** 只由该 Timeline event 确定性重建 projection；不得安装 native continuation、调用 provider、执行工具或复活被 rewind/discard/quarantine 排除的 candidate。

#### Scenario: Process stops before attempt closure

- **WHEN** 进程在 provider 调用后、证据/结算/接纳/projection 闭合前终止
- **THEN** 恢复保留未确认状态、原 response admission identity 和可用证据；已接纳但未投影的 response 只做本地 UI reconciliation，不把未确认 attempt 自动重放为成功消息，也不据此自动重发 provider 请求；历史无 identity 的 response 不能被猜测为该提交。

#### Scenario: Large response projection and independent replay

- **WHEN** 已接纳响应含较大的 assistant 或 visible-reasoning 正文，随后被冷恢复、resident delta 或 fork 回放
- **THEN** admission 阶段投影的正文与 canonical response 共享内存，持久投影仍可独立重建相同顺序的 ACP 内容；回放与 fork 的展示载荷不泄漏父 session 的候选身份。

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

### Requirement: Repeated rewind preserves retained response provenance

Timeline SHALL 在一次或多次 rewind 后继续把保留的历史响应关联到原 admission；compaction 或 Surface replacement SHALL NOT 将该归属重置为最近一次 rewind。切出当前 branch 的响应 SHALL 不再成为 replay authority。

#### Scenario: Rewind twice after replacing a later prompt

- **WHEN** 会话保留早期 response，rewind 较晚 prompt，生成替代分支后再次 rewind
- **THEN** 早期 response 的 admission 和重载回放保持一次，两个被切走的 response 均不复活。

#### Scenario: Repeated rewind crosses compacted history

- **WHEN** 保留前缀经过 compaction，随后发生重复 rewind 和冷 fold
- **THEN** 保留 response 的 provenance 与未压缩前缀一致，回放不遗漏也不复制历史响应。

### Requirement: Exact replay commits acknowledge durable barriers

Response projection 和独立 ACP event 的 exact commit SHALL 只有在所需文件及目录同步完成后返回成功。读取到相同 key/payload SHALL 仅证明身份一致，不得代替同步确认。

#### Scenario: Complete record remains readable after sync failure

- **WHEN** append 已写出完整记录，但 file sync 或 directory sync 失败
- **THEN** exact reconcile 在同步仍失败时返回错误，不放行 Accepted、后继 provider 或工具。

#### Scenario: Exact retry after storage recovers

- **WHEN** 同一记录在同步恢复后按原 identity/payload 重试
- **THEN** 同步已有记录后成功且不重复追加；payload conflict 仍拒绝。

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

Transient session persistence SHALL retain neither candidate notification payloads nor an unbounded window of independent notifications. The first candidate SHALL create one durable, payload-free ordering anchor before canonical Timeline admission. During an active attempt, independent untagged ACP and one-shot Grow notifications emitted by the session actor SHALL be appended in arrival order with acknowledgement before another notification from that actor is delivered; previously buffered notifications SHALL be flushed first. A failed anchor or independent append SHALL prevent canonical admission. Live preview notifications from the session actor, including Grow notifications emitted during the attempt, SHALL have a per-session byte budget through gateway completion; exhaustion or an oversized single item SHALL fail the attempt's preview boundary before canonical admission. Replay SHALL suppress an anchor for a discarded attempt and SHALL insert admitted canonical content at that anchor before later independent updates. Live delivery MAY carry candidate notifications independently of persistence.

#### Scenario: Long interleaved attempt

- **WHEN** an attempt emits many candidate chunks interleaved with independent untagged ACP or one-shot Grow notifications
- **THEN** only one payload-free candidate anchor is persisted, earlier buffered updates are flushed first, independent updates stream to storage with acknowledgement rather than accumulating in the sender, and admitted replay places canonical content at the first candidate position.

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

Subagent lifecycle Grow notifications SHALL carry status and identity metadata without duplicating the child final output. The canonical child result and admitted completion receipt SHALL remain the source of that output. The parent actor SHALL persist and forward one stamped event under the existing independent-update and active-attempt gateway budget boundaries. A failed preview gateway reservation SHALL reject that exact attempt's canonical admission. Client-origin Grow notifications SHALL remain persist-only.

#### Scenario: Child finishes with a large answer during parent sampling

- **WHEN** a child result contains a large final answer and its parent has an active sampling attempt
- **THEN** the lifecycle Grow projection contains only metadata, its forwarded event uses the same event ID as its durable record, and the completion receipt continues to reference the full answer.

#### Scenario: Lifecycle preview gateway budget is exhausted

- **WHEN** the parent actor cannot reserve preview gateway credits for a child lifecycle projection
- **THEN** no uncredited lifecycle payload enters the gateway queue, that attempt fails preview admission, and canonical child lifecycle facts remain available for reconnect projection.

### Requirement: Tool bridge Grow projections share preview credits

Background task completion Grow projections SHALL omit copied task output while preserving full output through the task file and model-facing notification. The tool bridge SHALL reserve session preview credits for queued TaskBackgrounded, TaskCompleted, and ScheduledTaskCreated Grow persistence and live gateway copies, and for ScheduledTaskFired and MonitorEvent live copies, holding each reservation until its consumer completes. A failed reservation or append during an active sampling attempt SHALL prevent canonical response admission. Durable task-completion acknowledgement SHALL remain a prerequisite for acknowledged UI projection publication. Monitor events SHALL retain their model notification.

#### Scenario: Completed task has a large output file

- **WHEN** a background task completes with a large retained output file during an active attempt
- **THEN** the Grow completion snapshot contains status metadata but no copied output, the model notification retains its existing truncated text and output-file pointer, and queued Grow copies stay within preview credits.

#### Scenario: Monitor events outrun a slow client

- **WHEN** monitor events arrive while the gateway has not completed prior Grow delivery
- **THEN** additional live Grow events cannot exceed the session preview budget, while model notification commands retain their normal admission path.
