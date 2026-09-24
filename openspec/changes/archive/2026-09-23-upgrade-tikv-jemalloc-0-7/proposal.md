## Why

本 change 经可行性审计后暂不实施依赖升级。`tikv-jemallocator` 0.7.0 的确包含 jemalloc 5.3.1 和构建修复，但上游仍有针对 0.7 系列 Linux musl 构建失败的开放报告；Grow 将 jemalloc 作为 CLI 默认 Unix allocator，且正式发布 x86_64 与 aarch64 musl 版本。缺少 Linux musl 环境时，无法确认该升级能否保持发布目标可用。

这是有证据的 no-go 决定，不表示 Grow 已复现上游失败。保持当前 0.6 依赖；满足 backlog 所列重新启动条件后，再单独建立升级 change 并验证。

## 审计范围与证据

- 本地核对根 `Cargo.toml` 与 `Cargo.lock`：三项 `tikv-jemalloc-*` 直接依赖当前为 0.6，锁定为 0.6.1。
- `crates/codegen/cli/Cargo.toml` 默认启用 `jemalloc`；release workflow 包含 `x86_64-unknown-linux-musl` 与 `aarch64-unknown-linux-musl`。
- 上游报告：[tikv/jemallocator issue #166](https://github.com/tikv/jemallocator/issues/166)，报告 0.7.0 在 `x86_64-unknown-linux-musl` 构建失败；所贴日志中的 sys crate 是 0.7.1。该 issue 于 2026-05-27 创建，审计日 2026-09-23 仍显示 open。

## What Changes

- 记录不实施的决定及其证据；不改依赖、代码或既有行为契约。

## Capabilities

无。该结论不改变产品行为，因此 `.openspec.yaml` 使用 `skip_specs: true`。

## Impact

只更新本 change 的决策、验证记录和重新启动条件。
