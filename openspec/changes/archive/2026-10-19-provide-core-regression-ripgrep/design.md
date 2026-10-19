# Design

在 `core-regression.yml` 的库测试步骤前安装 Ubuntu `ripgrep` 包，随后通过 PATH 查找和 `rg --version` 验证安装可执行。将步骤放在核心测试之前，确保 Rust 单测运行时搜索工具能解析 `rg`。

在现有 release regression Python 合约测试中检查安装命令、解析检查、版本检查均位于核心库测试之前。保留所有现有 grep 回归，不过滤或跳过测试。

在 `shell_state.rs` 的 `cfg(test)` 命令辅助函数中返回子进程 stderr，并只在失败的 `set -a` 断言中同时输出命令阶段、stdout 和 stderr。诊断不输出 shell snapshot 或环境，也不改变生产代码及断言条件；它用于获取下一次 Linux 门禁的根因证据，不表示 126 已修复。
