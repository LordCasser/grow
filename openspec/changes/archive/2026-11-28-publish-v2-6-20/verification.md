## 本地候选

- 工作区及锁文件中的 33 个本仓库包同步为 2.6.20。Locked Cargo metadata、四个受影响包的 Rustfmt 和 git diff --check 通过。
- 发布脚本单测 12 项通过，1 项跳过。OpenSpec 严格校验 19 项通过。
- 会话修复的回归与实际测量见各自的归档。主会话写入确认修复通过 682 项 ChatState 测试和 32 项 Shell 回归。

## 官方流程

- 源码提交 `80343aaa2bb2a95568fe23a7a07bf3153797c2de`，annotated tag 对象 `f9db3349b0c8270d3f346ac99b93ac68a8d50629`。干净候选树通过 `scripts/validate-release.sh v2.6.20`，远端标签解引用至该提交。
- 首轮 Core 在长历史冷恢复 PTY 用例未显示最后一条回复。第二轮在保留 steer 的冷恢复用例报告输入被改派到 FIFO，第三轮在终端提交的差异配色断言失败。同一候选在本机通过长历史 PTY 精确复跑、两项 steer 用例连续 12 轮，以及差异配色精确复跑。
- [第四轮官方流程](https://github.com/LordCasser/grow/actions/runs/46019927154/attempts/4) 在同一标签上通过全部 20 项任务。前三轮失败均保留在此运行历史中。

## 公开发行

- [Grow v2.6.20](https://github.com/LordCasser/grow/releases/tag/v2.6.20) 于 2026-11-28 09:51:47（UTC+08:00）公开。GitHub API 确认为最新正式版。
- 十个平台压缩包的 GitHub 摘要逐一与 `SHA256SUMS` 匹配。下载的 macOS arm64 压缩包的独立 SHA-256 为 `e4bbeab5437e1d7852d6373215599d72f8cca2bb716b052fae38fd9f6d38b5d5`。
- macOS arm64 包通过官方 `build-one.yml` 和源码提交证明验证。包内的 `grow` 通过严格签名检查，隔离 Grow 目录中 `--help` 通过，`--version` 输出 `grow 2.6.20 (80343aa)`。

OpenSpec 归档前严格校验通过 19 项，归档后通过 18 项，历史归档校验通过 999 项。
