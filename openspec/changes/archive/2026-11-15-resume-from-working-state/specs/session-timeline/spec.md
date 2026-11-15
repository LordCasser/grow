## MODIFIED Requirements

### Requirement: Bulk replay avoids cumulative lifecycle copying
Full Timeline replay SHALL validate all read events and expose only a fully validated fold, without copying all accumulated lifecycle history for every replayed event. Live prepare and accept SHALL retain failed-write atomicity.

#### Scenario: Long valid history
- **WHEN** a long Timeline is restored in bulk
- **THEN** Surface, event sequence, lifecycle ownership and pending Control activation match transactional event acceptance.

#### Scenario: Invalid history
- **WHEN** an event violates content or lifecycle constraints during bulk replay
- **THEN** restoration fails without exposing a partially restored Timeline or modifying persisted session data.

#### Scenario: Invalid live append
- **WHEN** live prepare or accept rejects an event
- **THEN** the previously accepted Timeline remains unchanged.

### Requirement: Stored Timeline facts validate their encoded integrity

新格式 SHALL 为明确版本的事实编码保存 checksum，覆盖 seq、类型、时间和 payload；恢复与历史读取 SHALL 对各自读取的原字节校验并执行相关领域验证，不为 checksum 重新序列化整个 response。执行依赖的 artifact SHALL 在执行释放前验证。冷历史 artifact SHALL 在读取或完整验证时校验；checksum 不替代 exact identity/payload 判定或执行所有权。

#### Scenario: Payload remains valid JSON after modification
- **WHEN** 读取的持久正文被修改，仍可解析但与保存的 checksum 不符
- **THEN** 恢复拒绝该完整记录，不发布部分 Timeline 或修改原文件。

#### Scenario: Incomplete final record or incompatible schema
- **WHEN** 文件含未完成尾片或旧 schema
- **THEN** 尾片沿既有完整行规则处理；不支持的 schema 明确拒绝，不自动重写旧会话或绕过验证。

#### Scenario: An incremental observer reads interleaved physical facts
- **WHEN** Trajectory 或搜索观察读者读取含领域与展示事实的统一物理前缀
- **THEN** 校验每条物理记录与连续位置，只按原 Domain seq 投影领域事件；读取预算覆盖实际物理记录，捕获后追加不可混入，刷新失败不推进已缓存前缀。

## ADDED Requirements

### Requirement: Executable resume hydrates committed working state

Working state SHALL bind the session, storage generation, branch, physical record frontier and domain event frontier to one durable publication. It SHALL restore the current model Surface with its original identities and revision, effective control and model selection, pending inputs, interactions, work, results, accounting, and exact next identities. Earlier facts consulted by live admission or recovery SHALL be available by exact indexed lookup. The previous complete working state and canonical facts SHALL suffice after an interrupted publication. Writer ownership and exact settlement requirements apply before execution release.

#### Scenario: A session is created and used without a graceful exit
- **WHEN** Grow creates a session and commits facts during normal operation
- **THEN** creation establishes the initial publication and ordinary writes incrementally persist the indexes and working-state effects. A process stopping without Ctrl+D leaves a complete publication and a replayable tail. Building the initial index for these sessions is not deferred until exit or resume.

#### Scenario: Normal exit with long closed history
- **WHEN** confirmed exit is requested with a valid resident owner
- **THEN** it settles current work, confirms the required writes and retires the writer without scanning or copying unrelated closed records.

#### Scenario: Cold execution after checkpoint
- **WHEN** a supported working state and committed tail exist
- **THEN** the writer validates and hydrates the state, applies the committed tail and resolves necessary execution dependencies without reading unrelated closed history.

#### Scenario: Crash during publication or tail write
- **WHEN** the process terminates before or after checkpoint publication or leaves a partial final record
- **THEN** recovery selects a complete publication and applies each complete later fact once, preserving original identities. Invalid required facts fail closed and partial records do not advance ownership.

#### Scenario: A large indexed session loses its writer at publication
- **WHEN** the process exits before or after manifest replacement for a journal above 515 MB
- **THEN** the selected publication and committed suffix retain exact execution and accounting authority, and opening the derived index does not require rebuilding its allocator or scanning the canonical prefix.

#### Scenario: Durable commits advance an indexed frontier
- **WHEN** an admitted writer extends the journal past a published working-state frontier
- **THEN** subsequent publication advances using only the committed suffix and its working-state effects. A reader holding the previous cut can continue to resolve its indexed facts, and no snapshot may refer to journal bytes that have yet to be made durable.

#### Scenario: Pre-existing journal lacks a checkpoint
- **WHEN** a session written before incremental publication has no usable working-state checkpoint
- **THEN** rebuilding verifies the canonical facts and preserves their identities. Rebuild can run before the session is opened, independently of presentation and unrelated sessions. A direct open with no verified execution state keeps admission closed until that state is ready. Rebuild timing is reported separately from normal first resume.

#### Scenario: A writer starts during background migration
- **WHEN** an interactive writer acquires ownership or changes the journal while background migration builds an unpublished generation
- **THEN** migration leaves the writer's journal and publication untouched. Migration publishes only under the same writer lease after verifying that its original cut is still current.

#### Scenario: Only closed history grows
- **WHEN** comparable sessions differ only in the volume of unrelated closed history
- **THEN** for sessions written with incremental publication, the first and subsequent cold resumes, recent and older page reads, and confirmed exit do not read, copy or replay work proportional to that volume. Acceptance uses sessions written through the normal commit path without pre-opening them to build an index.


#### Scenario: Closed tool identity remains available after eviction
- **WHEN** a Tool call closes and a later checkpoint evicts its historical lifecycle from the execution fold
- **THEN** a later duplicate start or PostToolUse Hook observes the same exact call identity and state as full replay, and a historical range can still retrieve the original facts. An incomplete or invalid lookup does not prove absence.

#### Scenario: Closed Turn and Step boundaries are archived
- **WHEN** a published prefix contains closed Turn and Step pairs unrelated to any current Surface activation, wait, or continuation
- **THEN** cold execution does not replay those boundaries. Their exact identities and original prompt coordinates remain queryable. A new admission rejects reuse, and a later Hook or input route can resolve an archived Turn at the required predecessor position.

#### Scenario: A session title has been revised
- **WHEN** a published cut contains multiple title revisions
- **THEN** execution recovery uses the latest revision and any required source evidence. Older revisions remain addressable at their original event coordinates. A later generated or fallback title still cannot override a user title.

#### Scenario: The startup ContextRebuild is in the unpublished tail
- **WHEN** the committed suffix contains the validated pre-turn context replacement
- **THEN** cold recovery applies it to the saved Surface and restores the new branch coordinate. History paging uses that branch for admitted responses and keeps independent inherited display records at their original positions.

#### Scenario: Model routing changed before a suspension
- **WHEN** a published cut covers multiple model transitions and a suspended owner refers to its checkpoint frontier
- **THEN** publication validates exact transition continuity. Cold execution uses the latest route and terminal sampling receipts, and suspended execution validates the route at its original frontier. Other route transitions remain addressable without replay during execution recovery.

#### Scenario: An auxiliary Sideband has settled
- **WHEN** a working publication verifies a terminal Sideband whose purpose has no later consumer of the original result, including side questions, recaps, permission or laziness judgments, memory processing, progress reports, and ContextRecall with a write-time seal bound to its exact ledger prefix
- **THEN** the next executable recovery omits its archived body and preserves its exact spawn identity. A started ledger without a terminal remains eligible for interruption recovery; a terminal ContextRecall without a valid seal stays in the executable fold for dense validation. Complete history validation can still verify every ledger against its canonical frozen branch.

#### Scenario: A closed Step has already consumed a Stop continuation
- **WHEN** a published cut contains an active Turn and an earlier closed Step consumed a response or a completed Stop decision
- **THEN** suspension and recovery use the same response-consumption and committed-continuation evidence as dense replay without retaining the closed Step solely to derive that progress.

#### Scenario: Compaction replaced historical message bodies
- **WHEN** an executable cut has been published after a compaction replacement
- **THEN** the restored Surface preserves the exact replacement, revision and item coordinates. An explicit rewind or historical response lookup reads the original canonical evidence and applies the existing branch selection rules.

#### Scenario: A branch has no executable publication after a transformation
- **WHEN** a rewind or image projection requires branch evidence that the published cut does not cover
- **THEN** recovery validates that evidence against the canonical journal before opening admission. A presentation marker cannot select an execution branch by itself. The supported-cut performance bound applies only when the cut covers the selected branch.

#### Scenario: A startup context rebuild preceded ordinary work
- **WHEN** the journal published a ContextRebuild before the first prompt on its current branch and has no rewind or image projection requiring different branch evidence
- **THEN** cold recovery restores the replacement Surface and its original source coordinates. Its branch boundary still invalidates stale history cursors and excludes earlier admissions from current branch projection. The existence of that startup rebuild does not force closed Tool or Turn facts back into cold recovery.

#### Scenario: A closed Request is consulted after checkpoint
- **WHEN** an original Request is covered by a working publication and a new fact refers to its identity, attempt, owner or provider terminal
- **THEN** the live validator resolves the original Request at the new fact's predecessor position and enforces the same ownership, attempt and retry rules as dense replay. Open Requests preserve their original start and may be settled in the suffix. Closed Request bodies do not contribute to cold replay work.

#### Scenario: Closed families grow while the working set stays fixed
- **WHEN** a series of sessions adds only already settled Request, Hook, Input, child delivery and usage facts, while the current Surface, active obligations and unpublished suffix are fixed
- **THEN** cold execution and confirmed exit visit only the live snapshot, its necessary exact references and the fixed suffix. Exact retry, previously committed results and historical usage remain recoverable by key or page. Settlement that still owes a result, notification or usage charge remains live.

#### Scenario: A completed Hook still owns a pending decision
- **WHEN** a publication covers a completed Hook whose result can still be consumed by input admission, Tool preparation or a suspended Turn
- **THEN** the checkpoint restores the exact causal decision at its original position. A fully settled Hook stays available by occurrence and run identity for exact validation and history, without entering cold execution replay.

#### Scenario: Published sampling audit is outside the working state
- **WHEN** the published frontier covers a sampling audit reference and its body is only used as historical evidence
- **THEN** indexed execution resume does not open the body. A full audit or transfer still verifies the referenced chunks and rejects missing or altered evidence.

#### Scenario: Closed tool history grows inside an active Turn
- **WHEN** sessions have the same open execution state but different amounts of completed Tool lifecycle history, including within an ongoing Turn
- **THEN** resume, the first subsequent operation and confirmed exit do not scan or copy the completed lifecycle prefix.

#### Scenario: An older consumed input was replaced by compaction
- **WHEN** the published cut covers a consumed input whose user item was replaced on the Surface
- **THEN** cold execution restores the remaining Surface without decoding the older input body. The original input and its ordered echo blocks remain available at their canonical identities and positions.

#### Scenario: Consumption and its echo straddle publication
- **WHEN** the writer published an echo before committing the consuming input fact, or a new writer recovers a suffix containing that consume
- **THEN** the proof still binds the exact ordered blocks to the canonical input. A provisional echo does not become visible before its consume; a failed proof cannot authorize a history row or release execution.

#### Scenario: A response candidate precedes the published cut
- **WHEN** a response in the unpublished suffix names a candidate anchor from the published prefix
- **THEN** recovery loads that exact anchor by Request and attempt, verifies the original physical position and preserves its display ordering. Unrelated historical candidates stay on disk.

#### Scenario: The local plan has many earlier revisions
- **WHEN** the working publication covers successive local Planning commits
- **THEN** cold recovery restores the latest validated plan and its original compare-and-swap coordinate. A suffix update must name that coordinate. Earlier revisions remain available for exact history reads.

#### Scenario: A settled Hook fired inside a closed boundary
- **WHEN** a Hook ran during a Turn or Step that subsequently closed, and its result no longer has a live owner
- **THEN** neither the Hook nor its closed boundaries enter the cold replay solely to revalidate their original causal relationship. The published Hook remains readable at the exact trigger and handler coordinates.

#### Scenario: Recovery observations supersede earlier snapshots
- **WHEN** a published prefix contains multiple valid analytics or announcement snapshots
- **THEN** cold recovery loads the latest snapshot of each family at its original coordinate. Earlier snapshots remain available to full readers and exact history queries.

#### Scenario: An interaction attempt closes before the working cut
- **WHEN** the prefix contains resolved interaction attempts and an optional open wait
- **THEN** only open attempts enter the execution fold. A repeated attempt identity is rejected from the index, and suffix resolution can still close its published open attempt. A suspended resolution preserves its Runtime wait under the existing release rule.

#### Scenario: A suspended Turn waits for an original child
- **WHEN** the execution owner settles an original child wait and the lifecycle root actor is available
- **THEN** it resolves only the named spawn and terminal from the root's current ownership and its published index. Child seed and result proof still bind to that spawn. Repeated wait observations do not copy the rest of the lifecycle root history.

#### Scenario: A child terminal points to a published result
- **WHEN** the parent terminal names an exact result covered by the child's published cut or its bounded tail
- **THEN** verification reads the original child Seed and Result by position, checks their record digests and verifies the same cross-ledger link and terminal fields as full replay. Full export and audit still validate all facts.

#### Scenario: A retired child needs an interruption result
- **WHEN** a recovery writer has acquired the child lease and needs to close its execution and settle usage before issuing an exact result
- **THEN** recovery uses the child's verified working publication and suffix. The result still counts the original Turn and Tool starts and uses the same cumulative usage and completeness as dense recovery. A retry preserves the canonical result and previously committed usage.

#### Scenario: The user opens a legacy session before maintenance migrates it
- **WHEN** no usable working-state publication exists for the selected journal
- **THEN** interactive recovery validates the original journal once to establish execution state. Building and syncing the derived generation runs independently of input readiness. A generation built while the session is active may publish only under the current writer epoch after checking the same journal prefix. It must not replace or overwrite a concurrent publication.

#### Scenario: Earlier Control revisions are superseded
- **WHEN** a published execution cut covers multiple Control revisions
- **THEN** the latest durable selection, effective and pending model context per layer, and highest durable terminal intent of each client remain recoverable. Cold recovery does not replay earlier Control transitions that no longer own those projections.

#### Scenario: A recap covered an earlier set of main turns
- **WHEN** a working publication covers recap display rows
- **THEN** the recap scheduler restores the last explicit frozen coverage without loading earlier recap bodies. A newer success in the suffix supersedes it. Unavailable or legacy recap rows do not erase known coverage. The historical rows remain pageable.

#### Scenario: Completed retry, compaction, and memory notices accumulate
- **WHEN** published history contains standalone terminal notices from retries, compaction, or memory processing
- **THEN** cold execution does not decode those notice bodies. Recent and older history pages can still return the original notice positions and identities, and complete history includes the original notices.

#### Scenario: Historical Goal display revisions accumulate
- **WHEN** a working publication covers multiple Goal display revisions
- **THEN** cold execution restores the canonical Goal from Control. Goal display revisions remain ordered and verifiable in history pages and complete reads. Old display revisions cannot replace the live Goal.

#### Scenario: Source inquiry audit leaves the cold execution fold
- **WHEN** a working publication covers outgoing coordination start or completion facts
- **THEN** the writer resolves the first durable request and terminal by inquiry identity and applies the same duplicate and conflict rules across the publication boundary. Explicit retries can still consult the original facts. Ordinary cold recovery does not decode the archived questions or results.

#### Scenario: Agent communication has been delivered
- **WHEN** the published cut covers agent message receipts and their settled inbox dispositions
- **THEN** cold execution hydrates only pending inbox entries and the current Surface. A retry or reply resolves the original receipt by sender and operation, preserving the source and payload match. The payload remains a retention root. Historical presentation reads the original receipt at the saved display anchor or, if no anchor has been published at the page cut, at its original domain position.

#### Scenario: A consultation terminal and its receipt cross publication
- **WHEN** an incoming consultation starts, ends, or durably injects its answer across a working-state cut
- **THEN** recovery retains exactly the unresolved notice or answer obligation. Before admitting a receipt it verifies the original local terminal, its InfoRequest result, and the exact quoted payload. Settled consultations remain available by sender and inquiry ID and the original display position. They do not reopen their InfoRequest ledger during cold execution.

#### Scenario: A presentation identity is retried after publication
- **WHEN** a committed presentation record is retried after its cut has been published or recovered
- **THEN** the writer validates the exact original payload by identity across all presentation families. It accepts an identical retry without appending a row and rejects conflicting content.

#### Scenario: The background image sweep encounters archived physical references
- **WHEN** an indexed writer reconciles image assets after recovery
- **THEN** the sweep obtains committed ownership from indexed image roots and the suffix, including images removed from the model Surface. It does not materialize the unrelated journal. An unavailable or corrupt ownership read cannot authorize deletion.

#### Scenario: A compaction closes with a sealed Sideband proof
- **WHEN** publication covers a compaction terminal and its linked Sideband result and replacement have been verified
- **THEN** the checkpoint keeps the resulting Surface and latest completed prompt coordinate. A later cold resume does not reopen that compaction's historical ledger. Open transactions and unresolved Sidebands remain available to recovery. Full history validation, recall and rewind continue to use the original facts.

#### Scenario: A consumed completion or task signal is outside the working set
- **WHEN** a cut covers the consumption, dismissal or supersession of a task or child receipt
- **THEN** the receipt and its disposition remain addressable by their original identities. Cold execution restores only the pending receipts and the model-facing Surface. A later receipt for an already delivered child or a closed task observes the same disposition as full replay.

#### Scenario: A background command settled in a prior runtime
- **WHEN** a published cut covers a background start and its durable Runtime settlement or ordinary completion
- **THEN** cold execution does not replay their historical bodies. A display page reads the matching source metadata and terminal at their original positions. Only unresolved starts are needed to reconcile the current task surface, including a start whose terminal is in the unpublished tail.

#### Scenario: Workflow execution epochs have settled before publication
- **WHEN** a publication covers closed Workflow runs or earlier epochs of a current run
- **THEN** execution restores the current open epochs with their original start coordinates. Lookup by run identity and predecessor verifies the original spawn and relevant epoch boundary. Restoring the bounded recent Workflow list visits candidates in descending spawn order, including older valid candidates when more recent sidecars are unavailable.

#### Scenario: A Workflow or Plan handoff was acknowledged
- **WHEN** the journal publishes a handoff receipt and its consumption or dismissal
- **THEN** the inbox does not replay the settled receipt on cold recovery. Exact identity and disposition remain available to retries and restoration. Workflow projection revisions remain available at their original history positions.

#### Scenario: Delivery of an AgentMessage batch has committed
- **WHEN** the published cut covers the consumption of a typed AgentMessage batch
- **THEN** the working Surface preserves any still-visible messages and the inbox observes the original disposition. A query for a consumed message resolves its original batch by identity. Historical batches do not require their closed Turn to be replayed.

#### Scenario: The current Surface contains a SkillLoad
- **WHEN** the primary request or recovery materializes a published Surface containing a SkillLoad
- **THEN** it preserves the exact unconsumed SkillLoad identities and applies later response admissions and Surface changes to that decision. An original model-origin SkillLoad still requires exactly one matching call from an admitted response.
