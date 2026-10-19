# Core regression installs ripgrep before search tool tests

## Why

核心回归中的 Grow grep 测试调用外部 ripgrep，但 workflow 未声明或检查该依赖。runner 缺少 `rg` 时，这组测试会以 `exit_code = -1`、空结果失败。

## What Changes

在运行核心 Rust 测试前安装 ripgrep，并检查 `rg` 可解析且可运行。另为 persistent shell 的 Bash 回归辅助函数保留子进程 stderr，并让 `set -a` 阶段失败时报告 stdout/stderr，以便定位独立的 Linux 门禁失败。Python 合约测试约束 ripgrep 前置步骤；所有现有 grep 与 shell 回归继续正常执行。

本 change 仅维护 CI 与测试契约，不改变产品行为，因此不修改已归档规范。
