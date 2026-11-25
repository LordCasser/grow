## 本地候选

- 远端最新正式版为 v2.6.18，v2.6.19 在本地和远端均未占用。Workspace 与锁文件中 33 个本仓库包的版本同步为 2.6.19。
- Rust 1.93.1 下 Rustfmt、locked metadata、默认 CLI 检查、全工作区 all-targets 检查、release-dist 的 `-D warnings` 检查通过。全目标检查保留了既有测试警告。
- 发布及测试执行脚本 34 项通过，1 项按平台跳过。OpenSpec 严格校验 19 项、归档校验 992 项通过。
- 本次候选纳入上下文审计和已验收的修复。公开审计文档用代号替换现场会话和网关历史索引。测试计数、复现和集成边界见[上下文修复验收](../2026-11-25-close-context-management-audit/verification.md)。

## 官方流程

- 源码提交 `942c9235c4b01c6270f20f080333b3feaa54653c`，annotated tag 对象 `3eb0ec80af8cce1bcbb35c54e3faeaf27c4e7a67`。在干净候选树运行 `bash scripts/validate-release.sh v2.6.19` 通过。原子推送后核对入口分支、远端标签对象和解引用提交。
- [第一轮 Core](https://github.com/LordCasser/grow/actions/runs/45560619999/job/139023326692) 在 Pager 的空白引用行拖拽复制测试失败。[第二轮 Core](https://github.com/LordCasser/grow/actions/runs/45560619999/job/139053730914) 通过全库测试，长历史冷恢复 PTY 用例未显示最后一条回复。同一提交的[独立 Core](https://github.com/LordCasser/grow/actions/runs/45560608011) 全部通过，本机从候选源码构建 CLI 后精确复跑同一长历史用例也通过。
- [第三次官方运行](https://github.com/LordCasser/grow/actions/runs/45560619999/attempts/3) 使用同一标签，通过全部 20 项任务，包含十个平台构建、Core、三平台原生协调及导出、Windows 存储、OpenSpec 和公开发布。

## 公开发行

- [Grow v2.6.19](https://github.com/LordCasser/grow/releases/tag/v2.6.19) 于 2026-11-25 17:03:05（UTC+08:00）公开。GitHub API 确认为正式最新版，远端标签仍指向已验收的源码。
- 公开资产包括十个平台压缩包及 `SHA256SUMS`，每个压缩包的 GitHub SHA-256 均与清单一致。下载 macOS arm64 包并独立验证，摘要为 `f2b449265ce68ca28f580b640673b4e937b8613a29ba26142df7c6cb426f16c3`。
- macOS arm64 包的构建证明绑定官方 `build-one.yml` 与完整源码 SHA。包内只有 `grow`，通过严格签名检查，并在隔离 Grow 目录中通过 `--help` 与版本检查，输出 `grow 2.6.19 (942c923)`。

- OpenSpec 完成归档后，严格校验 18 项，历史归档校验 993 项，均通过。
