# 验证记录

2026-10-31，Rust 1.93.1，macOS。没有原反馈的 session、日志或运行环境；以下证据证明代码缺陷及受控交错，不确认原截图根因。

## 先失败、后修复

原实现运行 `cargo test --locked -p shell --lib completed_compaction_ -- --test-threads=1`：3 项中 2 通过、1 失败。`completed_compaction_preserves_shared_storage_admission_error` 在 Summary replacement 后、Completed ACK 前注入另一 owner 的 StorageFull，实际返回 `PromptTurnOk { stop_reason: Cancelled, ... reason: "provider admission closed before sampling" }`，违反期望的 storage Err。局部 InvalidData 对照通过。

修复后的 `cargo test --locked -p shell --lib compaction_admission_tests -- --test-threads=1`：4/4 通过。

| 场景 | 断言 |
| --- | --- |
| shared storage 在 Completed ACK 前关闭 | 原始原因随 Err 返回；Turn outcome 为 error；Completed 一次且摘要保留；仅原请求和摘要两个请求，无后继派发 |
| 另一 session 的局部 InvalidData | 当前 gate 开放；同一 Turn 正常完成；共三个请求 |
| Summary ACK 持有期间取消后台 generation | publisher 已取走 slot；取消调用无需等待 ACK；原 token 被取消；不提交 replacement；前台继续完成 |
| replacement 后、Completed ACK 持有期间取消 | 原 token 被取消；已提交摘要及一次 Completed 保留；同一前台继续完成；scope 退出后取消状态不泄漏 |

两个异步交错用例在生产修复前也通过。因此本次没有修改 cancellation 实现；没有证据支持把该交错描述为新发现的取消泄漏。

## 既有回归

用上述 Cargo 生成的 Shell 测试二进制逐项 `--exact --test-threads=1` 运行并检查每项确有 `1 passed; 0 failed; 0 ignored`，44/44 通过：

- 18 个 `async_compaction_` 场景，包括 authority transition、Goal budget、late provider、promotion deadline、model route 和跨 Step 续接。
- 3 个 `compaction_repromotion_tests`，2 个 `continuation_step_tests`。
- 6 个 `storage_admission::tests`。
- 8 个 `context_window_`，以及 Plan 执行中的压缩续接、压缩大小失败后的下一回合恢复。
- 普通取消丢弃后台摘要，以及 4 个手动压缩排队、交接、错误与 Goal 恢复场景。

本次合计 48 个不同 Shell 测试通过；不声称运行了整包 4448 项或真实磁盘满/断电测试。构建环境设 `CARGO_BUILD_JOBS=2 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 RUST_MIN_STACK=16777216`，工具链来自 `$HOME/.cargo/bin`。

## 静态与契约检查

- `rustfmt --edition 2024 --config skip_children=true --check`：本次 production 文件与新测试文件通过。
- 定向 `git diff --check` 通过。
- 只读对抗性复核：保留原控制短路优先级、错误返回释放锁、外层 error 结算保留 Completed；并发控制接管仍遵守原 recovery 规则。
- `openspec validate --all --strict --no-interactive` 在归档前、归档后均通过；`openspec validate --archived --no-interactive` 通过。

临时原始输出：`/tmp/grow-compaction-admission-red.log`、`/tmp/grow-compaction-admission-green.log`、`/tmp/grow-compaction-regression.log`。无关挂起排空疑点已独立登记 backlog。

验证结束后删除本次生成的约 385 MiB Shell 测试可执行文件，保留共享依赖缓存与其他任务的构建产物。
