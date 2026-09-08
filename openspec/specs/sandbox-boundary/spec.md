# sandbox-boundary Specification

## Purpose
定义进程隔离、子进程网络与 Hook 写保护的边界。明确执行依赖 enforce feature 和平台能力，并记录需要保护时的拒绝继续行为；这里不把所有平台的实现描述为等价隔离保证。

## Requirements

### Requirement: Process and child boundary
启用 enforce 且平台支持时，Sandbox SHALL 在进程启动阶段应用 OS 隔离，覆盖进程内文件访问与子进程；子进程网络由独立策略处理。

#### Scenario: 启动隔离会话
- **WHEN** 进程选择需执行的 sandbox profile
- **THEN** 应用相应平台隔离；模型网络与 child network 分开处理。未启用 enforce 的构建不承诺内核隔离。

证据：`crates/codegen/sandbox/src/lib.rs` — `SandboxManager`。

### Requirement: Hook write protection
需要直接 Hook 写保护的 profile SHALL 在无法应用该保护时拒绝继续。

#### Scenario: 保护无法建立
- **WHEN** 选择需要 hook write deny 的非 devbox enforcing profile
- **THEN** 宿主必须确认保护已执行；requires_hook_write_deny 对 Off 和 Devbox 返回 false。

证据：`crates/codegen/sandbox/src/lib.rs` — `requires_hook_write_deny`。
