## ADDED Requirements

### Requirement: Budgeted Git status workers
gix status 默认工作线程预算 SHALL 在 1 到 8 之间，并按 CPU 数与 soft_nproc 减已用线程数、再保留 8 个外部线程的余量缩小；余量小于 2 时使用 1。

#### Scenario: 资源紧张
- **WHEN** cores=16、soft_nproc=20、threads_used=10
- **THEN** 预算为 2；soft_nproc=19 时预算为 1，不传 gix 中表示无限制的 Some(0)。

#### Scenario: 显式覆盖
- **WHEN** GROW_GIX_STATUS_THREADS 为合法正整数
- **THEN** 使用该值，允许超过 8 且绕过 nproc 预算；0、空白或非法值回退默认计算。

证据：`crates/codegen/grow-gix-status/src/lib.rs` — `compute_gix_status_thread_limit_from`；`crates/codegen/grow-gix-status/src/lib.rs` — `with_budgeted_thread_limit`。

### Requirement: Platform budget probes and constrained tests
线程预算探测 SHALL 在 Unix 查询 RLIMIT_NPROC，在 Linux 读取 /proc/self/status 的 Threads；查询失败或其他平台使用相应 None/1 回退。

#### Scenario: 限制不可用
- **WHEN** getrlimit 失败或无限制，或平台不是 Unix
- **THEN** 不使用 nproc 上限；非 Linux 的已用线程估计为 1。

#### Scenario: 低 nproc 回归
- **WHEN** 执行本包 Unix 子进程回归
- **THEN** 只在 child 修改 rlimit；Linux 验证 uncapped 失败及受限扫描存活，缺少可执行前置条件时显式 skip，不把 macOS 测试当作 Linux 限制证明。

证据：`crates/codegen/grow-gix-status/src/lib.rs` — `soft_nproc_limit`；`crates/codegen/grow-gix-status/src/lib.rs` — `production_budgeted_gix_status_survives_tight_nproc`。
### Requirement: Pager git head notification root-first cache update

A GitHeadChanged notification SHALL parse the shared typed payload, search top-level agents for an exact ACP session id first, and otherwise search each agent's direct child views. The first match updates the shared per-cwd git cache plus that view's current_branch, is_worktree and main_repo, then returns true regardless of active visibility. Malformed or unmatched payloads return false. Root matches take precedence over child matches, and nested grandchildren are not searched.

#### Scenario: Root
- **WHEN** a top-level session id matches
- **THEN** shared cwd cache and root display fields update and true returns.

#### Scenario: Direct child
- **WHEN** no root matches and a direct child id matches
- **THEN** that child's cache and display fields update.

#### Scenario: Root precedence
- **WHEN** the same id could match a root and child
- **THEN** the root search returns first.

#### Scenario: Nested child
- **WHEN** only a grandchild session id matches
- **THEN** this handler returns false.

#### Scenario: Malformed
- **WHEN** typed payload parsing fails
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_git_head_changed`。

