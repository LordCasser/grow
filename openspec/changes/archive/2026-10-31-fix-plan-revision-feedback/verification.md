# 验证记录

2026-10-31，macOS arm64，Rust/Cargo 1.93.1。设置 `CARGO_INCREMENTAL=0`、`CARGO_BUILD_JOBS=4`、`RUST_MIN_STACK=16777216`。

- 修改前，两项新回归在编译后的同一 Shell 测试产物上先 `--list` 确认唯一名称，再 `--exact` 执行，均因反馈出现次数为 0 而非 1 失败。日志：`/tmp/grow-plan-revision-behavior-red.log`、`/tmp/grow-plan-revision-actor-red.log`。初次 Cargo 命令用了短名配 `--exact`，选中 0 项，不计作验证；随后使用枚举出的完整名称执行。
- 修复后 `cargo test --locked -p shell --lib plan -- --test-threads=4`：131 passed，含真实 `accept_plan_decision`、Control 上下文、Timeline replay 和重试幂等回归。日志：`/tmp/grow-plan-revision-green.log`。
- 同一最终产物的 `session::behavior::tests::`：34 passed；覆盖 Drafting/Amending、连续反馈、空白/None、快照恢复及新 artifact 不继承旧反馈。日志：`/tmp/grow-plan-revision-behavior-green.log`。此集合与上述 Plan 过滤集合有重叠，不相加。
- Actor 回归直接检查 feedback 同时出现在 replay 的有效 PlanPhase 和模型可见 Surface；不只检查 snapshot 字段。该集成场景为 idle 激活；live 路径经代码核对使用相同提交入口，本次未新增真实 PTY 或线上模型回归。
- `cargo fmt --all -- --check`、`git diff --check` 和 `openspec validate --all --strict --no-interactive` 通过。

此修复只处理反馈投影遗漏。用户随后澄清的“没有操作提示、comment 进入后续队列”继续沿 UI 路由另查，不能用上述通过结果宣称已解决该交互问题。

后续用户明确窗口卡在 `check confirmation`；对应确认竞态与旧 pending 锁住新提交已由 [决定确认修复](../2026-10-31-fix-plan-decision-confirmation/verification.md) 单独闭合。
