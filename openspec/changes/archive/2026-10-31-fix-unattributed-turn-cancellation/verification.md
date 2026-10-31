# 验证记录

## 反馈与证据边界

用户确认反馈来自其他用户，无现场 session/日志。截图展示压缩完成后取消，不能据此建立因果。本次没有复现自动取消，也没有修改压缩或取消状态机。

代码证据：`session_event.rs` 的 `TurnCancelled` 只有耗时，旧 renderer 固定添加 `by user`；RPC 与 durable terminal 都可从 cancelled stop reason 得到该事件。Shell 的 Step/foreground/provider 准入关闭可返回 `TurnOutcome::Cancelled { category: None, context: ... }`，所以原文案归因不成立。`AutoCompactCompleted` handler 只应用压缩展示和 token 计数。

## 已执行

工具链为仓库要求的 Rust/Cargo 1.93.1。首次 Cargo 命令使用 `CARGO_BUILD_JOBS=2 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 RUST_MIN_STACK=16777216`。

- `cargo test --locked -p shell --lib context_window_exceeded_triggers_compaction -- --test-threads=1`：1 passed。
- 同次构建产物 `target/debug/deps/shell-92eda7b0ad9f562c`，分别以 `RUST_MIN_STACK=16777216` 运行：
  - `executing_plan_continues_after_context_compaction --test-threads=1`：1 passed。
  - `async_compaction_ --test-threads=4`：18 passed，覆盖提交/续接、提升、真实 generation 取消及迟到结果、失败、deadline、控制失效和 Goal 结算。
  - `context_window_ --test-threads=4`：8 passed，覆盖明确超窗恢复、压缩后仍超窗与重复 provider 超窗停止；包括前面的普通模式回归。
  - `compaction_size_failure_allows_the_next_user_turn_to_recover --test-threads=1`：1 passed。

Plan 测试经 artifact、submit、approve 进入 Executing，持久化 Control 后调用真实 `handle_prompt`、sampler 和 HTTP mock。验证摘要已提交、压缩后回答保留、共三次请求、只有一个 TurnStarted、结果 EndTurn、没有 cancelled TurnEnded、Plan 仍为 executing。

只读对抗性审查确认没有用新 Turn 或手动复位 owner 绕过续跑，测试中的 Cancelled 不会被当作成功；负向取消标记探针仍查找新文案，未丢失断言。

## Pager 与静态验证

- `cargo test --locked -p pager --lib turn_completion -- --test-threads=4`：23 passed。
- 同次构建产物 `target/debug/deps/pager-c332cc88a46114cf`：`scrollback::blocks::session_event::tests` 33 passed（包含精确取消文案断言）、`cancelling_terminal_status_uses_first_wins_finalizer` 1 passed、`cancel_after_first_activity_does_not_restore` 1 passed。
- 本次涉及路径的 `git diff --check` 通过；修改的 Shell 测试和 renderer 经 Rust 1.93.1 `rustfmt --edition 2024 --check` 通过。全工作树另有并发任务修改，本次未修整其文件。
- `rg` 核对 Pager 与 pager-pty-harness 的 Rust 源码已无 `Turn cancelled by user`，共用取消事件的身份和耗时格式保持不变。
- 全量严格 OpenSpec 校验归档前 20 passed；`openspec archive fix-unattributed-turn-cancellation --yes` 成功；归档后全量严格校验 19 passed，`openspec validate --archived --no-interactive` 750 passed。


## 限制

没有运行真实 provider，也没有重现反馈用户的进程、路由或键鼠时序。PTY 文案断言同步更新，本次不构建并运行全套 PTY 应用。保留原反馈根因未知的结论，不把本次文案修正称为自动取消修复。

完成验证后清理本次生成的两个测试可执行文件（约 797 MiB）；保留并发任务共享的依赖与构建缓存。上述产物名称仅用于记录本次实际执行身份。
