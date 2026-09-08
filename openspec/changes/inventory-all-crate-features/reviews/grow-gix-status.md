# grow-gix-status 逐包核查

包路径：`crates/codegen/grow-gix-status`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/grow-gix-status/Cargo.toml`
- `crates/codegen/grow-gix-status/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Budgeted Git status workers](../specs/workspace-git-status/spec.md#requirement-budgeted-git-status-workers)：gix status 默认工作线程预算 SHALL 在 1 到 8 之间，并按 CPU 数与 soft_nproc 减已用线程数、再保留 8 个外部线程的余量缩小；余量小于 2 时使用 1。
- [Platform budget probes and constrained tests](../specs/workspace-git-status/spec.md#requirement-platform-budget-probes-and-constrained-tests)：线程预算探测 SHALL 在 Unix 查询 RLIMIT_NPROC，在 Linux 读取 /proc/self/status 的 Threads；查询失败或其他平台使用相应 None/1 回退。

## 边界

- 预算为 2；soft_nproc=19 时预算为 1，不传 gix 中表示无限制的 Some(0)。
- 使用该值，允许超过 8 且绕过 nproc 预算；0、空白或非法值回退默认计算。
- 不使用 nproc 上限；非 Linux 的已用线程估计为 1。
- 只在 child 修改 rlimit；Linux 验证 uncapped 失败及受限扫描存活，缺少可执行前置条件时显式 skip，不把 macOS 测试当作 Linux 限制证明。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
