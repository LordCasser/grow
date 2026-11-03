# memory-search Specification

## Purpose
定义 Markdown 记忆的存储隔离与查询结果边界。覆盖默认工作区目录、显式 flat root、embedding 不可用时的文本检索降级，以及合并评分后按配置限制输出的行为。

## Requirements

### Requirement: Scoped memory storage
默认 MemoryStorage SHALL 将全局记忆和工作区记忆置于 memory root，并由工作区标识隔离项目目录。

#### Scenario: 初始化存储
- **WHEN** 通过 MemoryStorage::new 创建工作区记忆
- **THEN** global_dir 使用 override 或 grow_home/memory，workspace_dir 追加 compute_workspace_hash；构造时不创建目录。new_flat 使用已隔离的 root。

证据：`crates/codegen/memory/src/storage.rs` — `new_inner`。

### Requirement: Search degradation
记忆检索 SHALL 在 embedding provider 缺失、失败或向量能力不可用时保留 FTS 检索路径。

#### Scenario: embedding 调用失败
- **WHEN** hybrid_search 无法取得查询向量
- **THEN** 以无查询向量进入合并阶段，仍返回符合配置的文本候选。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search`。

### Requirement: Bounded ranked search
记忆检索 SHALL 合并关键词与向量候选，并按来源、时间衰减、阈值及配置的结果数量输出。

#### Scenario: 检索候选过多
- **WHEN** 候选超过 max_results
- **THEN** 完成评分、过滤和可选 MMR 后限制返回数量；空模板不作为有用记忆返回。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search_merge`。

### Requirement: Memory flush retains bounded completed tool evidence

A memory-flush Sideband SHALL derive its provider request from one frozen Timeline Surface. It SHALL retain the result body and attachments of completed, unambiguous tool exchanges in the selected recent context, even when no Assistant message follows the result. It SHALL omit incomplete or ambiguous tool protocol, select only complete User turns within an explicit input budget, and fail without writing a summary when the newest turn cannot fit. Tool output SHALL be treated as untrusted data and SHALL NOT enter permission authority. Snapshot capture SHALL wait for an in-progress primary image conversion and its durable ImageProjection acknowledgement, observe session Sideband cancellation, and select every available acknowledged image description on the request copy before budgeting. Image evidence with a description SHALL be represented by that text without raw media, independently of negative capability. A supplied pre-compaction snapshot SHALL retain its original source IDs, revision and input range even if live context advances. The client, explicit request model and image capability key SHALL use the same prepared configuration, including the resolved explicit flush-model transport. Only the final selected window SHALL be considered for image recovery. Known rejecting routes SHALL recover neutral descriptions before provider admission. Unknown routes retain their first optimistic image request; explicit image capability rejection SHALL record that actual route and permit one recovered text attempt from the same snapshot, within the admitted budget. Missing descriptions or exceeded budget SHALL fail without a memory write. Flush SHALL NOT mutate the primary Surface and SHALL release the image projection gate before answer sampling.

#### Scenario: Tool result is the only completion evidence

- **WHEN** a User says deployment has not happened, an Assistant plans and calls a deployment tool, and the completed result reports a revision without a subsequent Assistant reply
- **THEN** the memory-flush provider request includes the completed call, its result and attachments or their acknowledged image descriptions, and the Sideband attempt identifies the frozen source revision and selected Surface coordinates

#### Scenario: Selection would split an exchange or exceed the budget

- **WHEN** the recent frozen turn contains an incomplete or ambiguous call, or its complete request exceeds the memory-flush input or attachment budget
- **THEN** incomplete protocol is omitted and older whole turns are dropped as needed; if the latest turn still exceeds the budget, the flush fails without persisting a summary

#### Scenario: Images already have acknowledged descriptions

- **WHEN** a flush snapshot contains described user or tool images and the flush request uses a text-only model
- **THEN** budgeting and the provider request use those descriptions without their raw media, preserving tool evidence and the immutable main image facts

#### Scenario: Main conversion is pending

- **WHEN** a flush snapshot is requested during primary image conversion or its pending durable ACK
- **THEN** capture waits for acknowledgement or Sideband cancellation; selected unresolved images on a known text-only route use neutral recovery, failing without a memory write when recovery is unavailable

#### Scenario: Compaction or later input changes live context

- **WHEN** a valid pre-compaction flush snapshot is captured and the live Surface subsequently changes
- **THEN** the flush uses only the supplied snapshot's content, selected source IDs, revision and input range without rebasing to the newer Surface

#### Scenario: Unresolved images are outside the selected window

- **WHEN** old unresolved images fall outside the bounded recent User turns and the retained request fits a known text-only route
- **THEN** those unsent images do not block the flush or trigger conversion

#### Scenario: Flush model override changes image capability identity

- **WHEN** an explicit flush model differs from the prepared primary model or transport
- **THEN** unresolved image validation uses the actual flush model's transport identity and does not inherit a different primary model's negative capability

#### Scenario: Frozen images are first rejected during flush

- **WHEN** the explicit flush route first rejects unresolved images after pre-compaction capture and live history advances
- **THEN** recovery and its single text retry use the old snapshot IDs, revision and input range only; accepted text is written to memory and the durable ledger reloads with those original sources.

### Requirement: Native vector dispatch rejects inconsistent state
The vendored vector backend SHALL reject missing metadata-IN dispatch entries and unsupported vector or distance dispatch values with a SQLite error before accessing absent entries, computing an undefined distance, or writing with undefined byte counts. Valid vector search behavior SHALL remain unchanged.

#### Scenario: Metadata IN has no matching argument
- **WHEN** text metadata filtering receives an empty IN dispatch array or no entry for its argument index
- **THEN** it returns SQLITE_ERROR without indexing beyond the dispatch array and releases its temporary rowid/statement resources

#### Scenario: Unsupported native dispatch value
- **WHEN** vector distance dispatch or vector blob writing receives an unsupported element or metric value
- **THEN** it returns SQLITE_ERROR through its existing cleanup boundary rather than using an uninitialized result, byte length or offset

#### Scenario: Supported vector queries
- **WHEN** the backend receives a supported MATCH/k search or a normal text metadata IN filter
- **THEN** valid nearest-neighbor and metadata-filter results retain their existing semantics

### Requirement: Native vector metadata names follow virtual table ownership
The vendored vector backend SHALL release every owned metadata shadow-table name when its virtual-table owner is destroyed or initialization fails after allocation. Statement-only cleanup SHALL retain names needed by the live table, and rename SHALL transfer ownership to replacement names without leaking or double-freeing either generation.

#### Scenario: Successful metadata table lifecycle
- **WHEN** a virtual table with multiple metadata columns is used, optionally renamed or reconnected, and then dropped or disconnected
- **THEN** valid queries retain their results and all names owned by each retired virtual-table instance are released exactly once

#### Scenario: Initialization or partial allocation cleanup
- **WHEN** construction fails after metadata-name allocation or cleanup sees partially allocated name slots
- **THEN** allocated metadata names are released, NULL slots are safe, and repeated cleanup does not double-free names

#### Scenario: Statement cleanup while the owner remains live
- **WHEN** transaction sync or rename finalizes cached statements while the table remains usable
- **THEN** current metadata-name ownership remains valid until replacement or destruction, and subsequent metadata queries succeed

### Requirement: Native float vector errors have initialized ownership

The native float vector parser SHALL return an initialized, SQLite-owned error string or NULL on failure. Allocation failure before diagnostic text exists SHALL NOT expose an uninitialized error pointer to scalar error reporting or cleanup. Valid conversions and ordinary invalid-input diagnostics SHALL retain their existing semantics.

#### Scenario: JSON vector allocation fails
- **WHEN** the initial float vector allocation fails before a diagnostic string is created
- **THEN** the parser returns SQLITE_NOMEM with a NULL error output, without transferring a vector or undefined cleanup ownership.

#### Scenario: Scalar conversion receives a parser failure
- **WHEN** vec_f32 or vec_quantize_int8 receives that failed conversion
- **THEN** it reports a SQLite error without reading or freeing an undefined pointer, and the same connection remains usable after the fault ends.

#### Scenario: Valid or ordinary invalid input
- **WHEN** float scalar conversion receives supported vector input or malformed input without allocation failure
- **THEN** the existing converted value or concrete input diagnostic is preserved.

### Requirement: Idle memory flush waits for inactivity and checkpoints only success

自动 memory flush SHALL 在用户活动停止达到正数配置时长，且 session 前台与 session activities 均空闲后才启动。用户活动及前台忙转闲 SHALL 重新开始 idle interval。零秒 timeout SHALL 禁用自动 flush。只有 flush 成功 checkpoint（写入记忆或确认无需写入）才推进最近成功历史水位；失败和 flush lock 冲突 SHALL 保留同一历史的后续重试资格。

#### Scenario: Recent activity delays idle flush
- **WHEN** 用户提交输入、输入仍在 FIFO 等待、前台 turn 正在运行或手动压缩待执行时 idle timeout 到期
- **THEN** 不启动 memory flush；用户活动与 turn 完成分别使 idle interval 从最近活动或忙转闲重新计时

#### Scenario: Successful flush checkpoints history
- **WHEN** 正数 idle interval 到期且 foreground/session activity 空闲，并且 flush 成功写入或确认无内容
- **THEN** 发起一次 flush 并推进成功历史水位；未出现新历史时后续 interval 不重复 flush

#### Scenario: Failed flush retains retry eligibility
- **WHEN** idle flush 因 provider/snapshot 错误或 flush lock 冲突未成功
- **THEN** 不推进成功水位；相同历史在下一次完整 idle interval 再次具备 flush 资格

#### Scenario: Zero timeout disables idle flush
- **WHEN** memory flush idle timeout 配置为零秒
- **THEN** run loop 不启动周期性 idle flush timer

### Requirement: Explicit memory flush models resolve their complete transport

An explicit flush_model SHALL resolve through the existing auxiliary model configuration resolver before creating its client. Provider endpoint, backend, credentials, actual request model and image capability identity SHALL come from the same resolved configuration. Session-local attribution SHALL retain the existing auxiliary stamping rules. An unavailable configured model SHALL fail without falling back to the primary route or writing memory. With no override, one prepared primary configuration SHALL remain authoritative.

#### Scenario: Flush model belongs to a different provider
- **WHEN** a visual primary uses one provider and the configured text-only flush model resolves to another endpoint or backend
- **THEN** the flush goes to the resolved flush transport, image rejection learns only that transport, neutral recovery uses its configured vision route, and the text retry can persist memory without altering the original images.

#### Scenario: Flush model cannot be resolved
- **WHEN** an explicit flush model alias is unavailable
- **THEN** flush fails without a provider request, primary-route fallback or memory write.

### Requirement: Runtime-disabled memory does not admit flush work

A memory flush SHALL observe whether memory is enabled for the current session before admitting work. Pre-compaction admission SHALL check the current state before freezing input and after asynchronous snapshot capture. A flush SHALL recheck the state before provider work and before persisting an accepted response. If memory is disabled, the flush SHALL not start a provider request or write memory data.

#### Scenario: Memory is disabled before compaction

- **WHEN** the user turns memory off and a later compaction reaches the pre-compaction flush threshold
- **THEN** pre-compaction flush does not capture or dispatch flush work and no memory-flush provider request is sent

#### Scenario: Memory is disabled while snapshot capture waits

- **WHEN** a memory flush is waiting for primary image projection and the user turns memory off before capture completes
- **THEN** the frozen read-only snapshot is not sent to a provider and no memory file is written

#### Scenario: Memory is disabled while the provider responds

- **WHEN** memory is turned off after a flush provider request begins but before its accepted response is persisted
- **THEN** the response is not written to memory

### Requirement: Manual memory flush admission is independent of foreground turns

Explicit `/flush` and `grow/memory/flush` SHALL admit an independent memory Sideband while a regular foreground turn is active, provided runtime memory is enabled and session activity admission remains open. Both SHALL reuse the same flush execution lock, frozen context and completion notifications. `/flush` SHALL NOT enter the model input queue or block the command loop while waiting for its provider. Disabled memory SHALL reject admission without provider work. Failure or lock contention SHALL NOT be treated as an automatic successful checkpoint.

The synchronous ACP extension SHALL report failure when execution did not produce a successful checkpoint. A successful checkpoint MAY mean written memory or confirmed nothing to store; the existing completion notification's result and path SHALL distinguish them.

#### Scenario: Flush while foreground is busy
- **WHEN** an enabled session receives an explicit manual flush during a regular turn
- **THEN** either entry admits Sideband work independently and the foreground continues; the original command is not added to model conversation.

#### Scenario: Disabled or closing session
- **WHEN** runtime memory is disabled or activity admission has closed
- **THEN** the manual command reports refusal and emits no provider work.

#### Scenario: Synchronous flush fails or loses execution admission
- **WHEN** the ACP flush is skipped for lock contention, rejected or fails
- **THEN** its response is an error rather than a successful empty receipt; automatic history remains eligible and completion feedback retains the outcome.

### Requirement: Memory daily-log writes report actual persistence

The memory storage API SHALL distinguish a daily log that was written from one skipped by the existing ephemeral-workspace policy. A path SHALL be returned only for a write that completed successfully. Memory flush and session-end handling SHALL NOT report an ephemeral skip as written; a skipped flush SHALL NOT advance its successful history checkpoint.

#### Scenario: Daily log write is suppressed for an ephemeral workspace

- **WHEN** `write_daily_log` is called for an ephemeral workspace
- **THEN** it returns an explicit skipped result, does not create the sessions directory or file, and callers do not present the expected path as a persisted log

#### Scenario: Daily log write completes

- **WHEN** `write_daily_log` successfully writes a daily log for a persistent workspace
- **THEN** it returns the written path, and memory flush may reindex the file and report `written`

#### Scenario: Memory flush is skipped by ephemeral policy

- **WHEN** an accepted flush summary reaches `write_daily_log` for an ephemeral workspace
- **THEN** completion and diagnostics report skipped rather than error, with no path, no reindex or persisted-write marker, and the history remains eligible for a later retry

#### Scenario: Session-end summary is skipped by ephemeral policy

- **WHEN** an eligible session-end summary reaches `write_daily_log` for an ephemeral workspace
- **THEN** the hook reports skipped rather than `Written`
