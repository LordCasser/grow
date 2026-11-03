## Verification

- `PAGER_BINARY=/Users/lordcasser/.local/bin/grow CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --test pty_e2e_persistence suspend_nested_task_tree::ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree -- --ignored --exact --test-threads=1`：1 passed，0 failed，0 ignored，22 filtered out；场景耗时 8.60s。测试 harness 在当前工作树编译，实际被测 grow 是当前安装的 release binary，不是本次重建的 CLI。
- 真实 PTY 覆盖连续两次 Ctrl+D、root/child/grandchild 运行中命令的确认中断、原身份和 Turn 冷恢复、已完成 child 不再执行、用户输入不重接纳、原命令不重放及唯一结果链接。全部使用隔离 HOME、临时项目和 loopback provider，没有操作用户真实会话。
- `git diff --check`：通过。本轮没有 Rust 源码修改；编译提示已有的 `Atomic::fetch_update` deprecated warnings，未顺带修改。
- 本 change strict 校验：通过。全量 strict 首次结果为 18 passed、1 failed；失败属于调查过程中并行出现的 `analyze-multi-subagent-memory-growth`，其尚未声明 delta/skip_specs。未替其他工作修改该 change。
- 归档前 `openspec validate --archived --strict --no-interactive`：798 passed，0 failed。
- 本 change 已以 `--skip-specs` 归档。归档后全量 strict：17 passed、1 failed，仍只有上述并行 change 缺少 delta/skip_specs；archive strict：799 passed，0 failed。
- 测试完成、确认没有 cargo/rustc 活动后，清理本次生成的 pager/shell/PTY incremental 目录及该 PTY runner（约 4.2 GiB），保留其余既有构建缓存。

## Adversarial review

- 将 parent Ended 写入时间与 child Turn terminal 时间分别核对；不以 UI 聚合时间推导实际取消发生时间。
- `user_cancellation` 标签没有被解释为用户逐个取消；采用同期 root 的 host/shutdown trigger 与五 child cancel 日志归因。
- 旧进程启动时间、当前文件 mtime 和相同版本号不足以精确重建旧构建；报告保留具体退出分支未知的限制。当前 PTY 成功只证明该已安装程序的已覆盖路径，不宣称复现或证明所有退出分支。
- 已取消 child 的终态没有被改写，未消费挂起 checkpoint 的自动恢复与显式 Task resume 没有混为一谈。本轮不宣称已恢复五个用户任务。
