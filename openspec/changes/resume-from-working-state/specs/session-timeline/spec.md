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

#### Scenario: Durable commits advance an indexed frontier
- **WHEN** an admitted writer extends the journal past a published working-state frontier
- **THEN** subsequent publication advances using only the committed suffix and its working-state effects. A reader holding the previous cut can continue to resolve its indexed facts, and no snapshot may refer to journal bytes that have yet to be made durable.

#### Scenario: Pre-existing journal lacks a checkpoint
- **WHEN** a session written before incremental publication has no usable working-state checkpoint
- **THEN** rebuilding verifies the canonical facts and preserves their identities. Rebuild can run before the session is opened, independently of presentation and unrelated sessions. A direct open with no verified execution state keeps admission closed until that state is ready. Rebuild timing is reported separately from normal first resume.

#### Scenario: Only closed history grows
- **WHEN** comparable sessions differ only in the volume of unrelated closed history
- **THEN** for sessions written with incremental publication, the first and subsequent cold resumes, recent and older page reads, and confirmed exit do not read, copy or replay work proportional to that volume. Acceptance uses sessions written through the normal commit path without pre-opening them to build an index.
