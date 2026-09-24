# workflow-execution Specification

## Purpose
定义 Rhai Workflow 从可编辑 Definition 到独立 Run 的执行契约。覆盖发布前内容验证、运行脚本所有权、journal 回放校验以及完成、暂停和失败等可区分结果。

## Requirements

### Requirement: Validated definition publishing
Workflow draft SHALL 在当前内容 hash 已验证后才能发布。

#### Scenario: 编辑后直接发布
- **WHEN** draft 内容已变更且 last_validated_hash 不匹配
- **THEN** 发布返回 NotValidated，必须验证当前内容。

证据：`crates/codegen/shell/src/session/workflow/workspace.rs` — `NotValidated`。

### Requirement: Run source ownership
Workflow Run SHALL 保存运行脚本副本与 revision，并按 Run identity 读取其脚本。

#### Scenario: 读取运行脚本
- **WHEN** 已创建 Run 的源 Definition 后续被编辑
- **THEN** script_for 返回 Run 保存的源，不重新读取可变 Definition。

证据：`crates/codegen/shell/src/session/workflow/store.rs` — `script_for`。

### Requirement: Deterministic journal replay
Workflow journal SHALL 按 seq、kind、request hash 校验回放，只复用匹配调用的已记录结果。

#### Scenario: 请求偏离历史
- **WHEN** 相同 seq 的 kind 或请求 hash 改变
- **THEN** 返回 JournalError::Divergence，不把旧结果交给新调用。

证据：`crates/codegen/workflow/src/journal.rs` — `replay`。

### Requirement: Typed workflow outcomes
Workflow SHALL 区分完成、暂停、等待用户、预算耗尽、取消与失败。

#### Scenario: 工作流结束或让出执行
- **WHEN** engine 返回 WorkflowOutcome
- **THEN** 保留对应类型及 result、message 或 error，调用方可区分终态与等待。

证据：`crates/codegen/workflow/src/run.rs` — `WorkflowOutcome`。

### Requirement: Resume follows durable execution settlement
Workflow resume SHALL await the previous execution terminal acknowledgment before admitting the next execution epoch. A resumable in-memory tracker projection alone SHALL NOT authorize resume while its terminal watcher is still settling.

#### Scenario: Resume during terminal persistence
- **WHEN** a run is projected paused or failed but its terminal persistence is still pending
- **THEN** resume waits and only appends Resumed after the prior Ended is acknowledged.

#### Scenario: Terminal settlement fails
- **WHEN** the pending terminal watcher reports failure or loses its acknowledgment channel
- **THEN** resume returns that failure and does not open a new execution epoch.

### Requirement: Workflow restore retains the latest valid runs
Workflow 恢复 SHALL 至多返回最新 128 个通过既有恢复校验的 run，并按 Timeline 顺序交付。cleared、缺失或不满足既有恢复校验而被跳过的候选 SHALL NOT 占用有效恢复名额。恢复 SHALL 保留目录能力校验、单文件读取上限和既有致命错误处理。

#### Scenario: More valid runs than capacity
- **WHEN** Timeline 中存在超过 128 个有效恢复 run
- **THEN** 返回最新 128 个，顺序与 Timeline 一致。

#### Scenario: Recent candidates are unusable
- **WHEN** 较新候选因 cleared、文件缺失或校验失败被跳过，但较旧候选有效
- **THEN** 继续检查较旧候选，直至获得 128 个有效 run 或候选耗尽。

#### Scenario: Corrupt sidecar is repaired before writer startup completes
- **WHEN** 冷恢复使用 Timeline seed 替代无法解码的 Workflow sidecar，且 sidecar 自读取后未变化
- **THEN** 将 reconciled seed 以持久化 ACK 写回后才完成 actor 初始化。

#### Scenario: Sidecar changes during repair
- **WHEN** 冷恢复读取到损坏 sidecar 后，该文件在修复锁取得前被替换或删除
- **THEN** 修复返回可观察的错误，不覆盖新内容，也不把 actor 初始化报告为成功。

证据：`crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `load_workflow_runs_sync`；`crates/codegen/shell/src/session/workflow/store.rs` — `WorkflowRunStore::from_restored` 和 manifest 写入；`crates/codegen/shell/src/session/persistence.rs` — Workflow persistence ACK；`crates/codegen/shell/src/session/actor/spawn.rs` — restored store 初始化。

### Requirement: Workflow listing has a bounded execution boundary
Asynchronous workflow listing SHALL isolate filesystem scans from runtime worker threads, bound concurrent scans to one, and apply a five-second deadline that includes waiting for scan capacity and scan completion. A timeout SHALL NOT claim to have interrupted an already-running filesystem operation.

#### Scenario: Scan stays blocked after request timeout
- **WHEN** a workflow scan remains blocked past the listing deadline and another listing request arrives
- **THEN** the first request returns an error, its worker retains the scan slot until it exits, and the later request cannot start an additional scan beyond the concurrency bound

#### Scenario: Listing scan fails or times out
- **WHEN** the scan worker fails or the listing deadline expires
- **THEN** `grow/workflows/list` returns an error rather than a successful empty workflow list

#### Scenario: Listing completes without results
- **WHEN** the scan completes successfully and discovers no workflows
- **THEN** the request returns a successful empty workflow list

### Requirement: Ambiguous Workflow spawn preserves recovery authority
Workflow launch SHALL treat a durable Spawned acknowledgment loss or other uncertain Timeline write failure as an unknown commit outcome. It SHALL NOT erase the run source or publish a clear tombstone unless the Timeline proves Spawned was rejected before persistence. Recovery SHALL use the actual Timeline facts to decide whether the run exists.

#### Scenario: Spawned persisted but caller acknowledgment is lost
- **WHEN** the Timeline writer appends Spawned but launch loses its acknowledgment
- **THEN** launch reports the run identity and uncertainty, leaves source files available, and cold recovery can restore the run from the Spawned seed.

#### Scenario: Spawned was rejected before persistence
- **WHEN** Timeline validation rejects Spawned without attempting an append
- **THEN** launch may roll back the local run and tombstone its sidecar; no recoverable Spawned fact exists.

#### Scenario: No Spawned fact was committed
- **WHEN** a launch fails before Spawned or its uncertain append never became durable
- **THEN** cold recovery ignores any remaining source files because run discovery is based on Timeline Spawned facts.

### Requirement: Workflow seed restore reports incomplete progress

When no valid mutable Workflow sidecar survives, restore SHALL derive Run identity and lifecycle from Timeline and SHALL mark agent usage incomplete. It SHALL NOT present the Spawn seed's zero usage as a complete cumulative count. Resume admission SHALL continue to reconcile agent calls from the durable journal.

#### Scenario: Completed run loses its progress sidecar
- **WHEN** a run has a durable terminal lifecycle but its sidecar is missing or invalid
- **THEN** restore retains the terminal status and reports `agent_usage_incomplete = true`, even if the seed contains zero agent rows and zero used calls.

#### Scenario: Valid progress sidecar survives
- **WHEN** a valid sidecar matches the Timeline frozen contract and lifecycle
- **THEN** restore uses its mutable progress and does not introduce an incomplete-usage marker solely because restore occurred.
