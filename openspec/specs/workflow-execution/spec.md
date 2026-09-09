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
