# Verification

审计日期：2026-09-23。

- 本地源码核对：根 `Cargo.toml` 的 `tikv-jemalloc-ctl`、`tikv-jemalloc-sys`、`tikv-jemallocator` 均声明 0.6；`Cargo.lock` 锁定为 ctl 0.6.1、sys 0.6.1+5.3.0、allocator 0.6.1。
- 本地源码核对：`crates/codegen/cli/Cargo.toml` 默认 feature 启用 `jemalloc`；`.github/workflows/release.yml` 定义 `x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl` 两个正式目标。
- 上游证据：[issue #166](https://github.com/tikv/jemallocator/issues/166) 标题为 “v0.7.0 no longer builds on target x86_64-unknown-linux-musl”，描述版本 0.7.0 失败，并含 `tikv-jemalloc-sys 0.7.1` 构建日志。issue 创建于 2026-05-27，审计时仍为 open（2026-09-23）。
- 未验证：本地宿主为 Darwin；没有运行 `cargo build` / Cargo 相关测试。未验证 Grow 的候选依赖在 `x86_64-unknown-linux-musl` 或 `aarch64-unknown-linux-musl` 上是否失败。不得把上游报告写成 Grow 的复现结果。
- 决定：暂不升级，等待上游修复发布，或由 Grow 两个 musl release-dist 目标的 CI 对候选版本成功构建后重启审查。此结论是风险门槛判断，不是实现完成或实测失败。
- `git diff --check`：通过。
- `openspec validate upgrade-tikv-jemalloc-0-7 --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`：通过，19 项。
- `openspec validate --archived --no-interactive`：通过，422 项。
