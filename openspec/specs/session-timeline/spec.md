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

被废弃 attempt 的原始证据、用量及恢复决定 SHALL 保留在既有 Timeline 证据链或其不可变 artifact 引用中，但 SHALL NOT 投影成模型有效上下文、native continuation 或可执行工具。候选只有在会话 durable admission 确认后才能发布已接纳状态。接纳失败或确认不明 SHALL NOT 触发盲目重新采样。

#### Scenario: Failed generation is followed by a valid attempt
- **WHEN** 第一次候选被拒收而第二次被持久化接纳
- **THEN** 证据可以追溯两次调用及废弃原因，模型 Surface 和可执行工具只包含第二次被接纳结果。

#### Scenario: Admission write acknowledgment is lost
- **WHEN** 响应可能已经写入 Timeline 但提交确认丢失
- **THEN** 停止新采样并通过原提交身份核对，不能把确认丢失当成模型生成失败而重复提交或执行工具。

#### Scenario: Process stops before attempt closure
- **WHEN** 进程在 provider 调用后、证据/结算/接纳闭合前终止
- **THEN** 恢复保留未确认状态和可用证据，不把未确认 attempt 自动重放为成功消息，也不据此自动重发 provider 请求。

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
