## 本地候选

- 正式最新版为 v2.6.17。v2.6.18 标签在本地及远端均未占用。
- workspace 和锁文件的 33 个继承版本同步为 2.6.18。CLI 测试新增的两项依赖引用来自 MCP doctor change，外部包版本未漂移。
- Rust 1.93.1 下 Rustfmt、locked metadata、默认 CLI 检查、workspace all-targets 检查、`release-dist` 的 `-D warnings` 检查通过。all-targets 仍报告历史测试警告。
- 发布及测试执行脚本共 35 项，34 项通过，1 项按平台跳过。OpenSpec 严格校验 19 项、归档校验 986 项通过。暂存差异检查通过。
- 同一候选提交的 `cargo test --locked -p cli --test mcp_doctor` 通过，覆盖真实 CLI 的 HTTP 探测阶段与退出码。

## 官方流程

- 源码提交 `c499013a41fb2ebc4c7a89ee725cec418a9974ea`，annotated tag 对象 `b4303e5a3d83680a9156ba5883258fad8da4681f`。在干净候选树运行 `bash scripts/validate-release.sh v2.6.18` 通过。原子推送后核对 main、远端标签对象及解引用提交。
- [官方发布 run 45127426367](https://github.com/LordCasser/grow/actions/runs/45127426367) 首次运行通过全部 20 项任务，包含 10 个分发构建、Core、三平台原生协调和导出、Windows 存储、OpenSpec 及公开发布。

## 公开发行

- [Grow v2.6.18](https://github.com/LordCasser/grow/releases/tag/v2.6.18) 于 2026-11-23 16:39:33（UTC+08:00）公开，GitHub API 确认为正式最新版。远端标签继续指向已验收的源码。
- 公开资产恰有 10 个平台压缩包及 `SHA256SUMS`，所有压缩包的 GitHub SHA-256 与清单一致。独立下载 macOS arm64 包与清单并重新计算摘要。该包 SHA-256 为 `6c4209c3352a75e4581dc1aac5eeaadca06c532c7f8a8a0c2f2a5f791ce43643`。
- 下载包的构建证明绑定官方 `build-one.yml` 和完整源码 SHA。包内只有一个 `grow`，通过严格签名核验并输出 `grow 2.6.18 (c499013)`，隔离目录中的 `--help` 通过。
