# Verification

- `cargo test --locked -p update --lib --test test_io`：通过，133 项 library 与 25 项 I/O 测试。
- `rustfmt --check --edition 2024 --config skip_children=true`（本次三个 Rust 文件）：通过。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：通过，归档前 20 项。

- `cargo check --locked -p pager --lib`：未完成；持续等待其他任务持有的共享构建锁后，向本次 Cargo 进程发送 SIGINT 停止排队（exit 130），未终止其他构建。Pager 提示分支仅完成代码审查与格式检查，未声称通过编译或 UI E2E。

## Adversarial review

- 两个更新入口仅拦截 Some(false)，None 与 Some(true) 均继续，且仍受 installer、缓存与版本策略约束。
- 检查失败、无更新、策略 Skip/Unavailable 不交付 UpdateAvailable，因此不显示误导提示。
- full 模式在已有结果接收分支中显示 toast，pending 版本继续供欢迎页使用；minimal 分支维持原 notice。
- Ctrl+U 只在欢迎页生效，故会话内 toast 使用 restart to apply，不声称会话内可用该快捷键。
- 现有入口直接访问 GitHub 并可能启动安装，不新增测试注入框架，也未触发真实线上更新；默认值与提示分支通过代码审查确认，现有回归覆盖周边行为。
- 共享 target 有其他开发任务使用，不清理其缓存；本次没有创建独立构建目录。

## Archive verification

- `openspec archive enable-default-auto-update --yes`：完成并合入主规范。
- 归档后 `openspec validate --all --strict --no-interactive`：18 项通过。
- `openspec validate --archived --no-interactive`：751 项通过。
- 归档后格式及 diff 空白检查通过。

## Pager compilation follow-up (2026-10-31)

- 重新执行 `cargo check --locked -p pager --lib`：通过，exit 0，耗时 1m 25s；替代上次因共享锁而未完成的编译验证结论。
- Pager 及依赖仅报告未修改代码中的 `Atomic::fetch_update` 弃用警告，没有编译错误；不混入本次修复。
- 重新核对本次 diff：两个更新入口默认启用、显式 false 退出；full TUI 使用既有 show_toast，minimal 仍走原有 notice。未追加实现修改，也未运行 UI E2E。
