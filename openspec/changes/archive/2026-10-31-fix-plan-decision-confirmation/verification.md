# 验证记录

2026-10-31，macOS arm64，Rust/Cargo 1.93.1。

## 复现与实现核对

- 前端提交决定后立即查询状态；后端的既有测试 `plan_decision_status_is_unconfirmed_until_control_ack_commits` 明确允许查询先返回 unconfirmed。此前后续没有回执交付，只有手动 `r` 核对；新审批也不清理旧 pending。
- 回执只在 `accept_plan_decision` 的 `commit_behavior_mutation_or_restore(...).await` 成功后进入既有 FIFO CurrentModeUpdate。普通模式更新不附加回执；去除原 approve/abandon 分支的重复模式更新。
- Pager 将模式通知与状态查询合并到同一确认入口。错误 identity/payload、普通 phase update 不确认；不同提交的新审批释放旧 UI 占用，相同提交仍等待原决定。

## 验证环境

共享工作区存在其他任务的并行修改。第一次组合构建 `/tmp/grow-plan-confirmation-green.log` 被子代理配置修改的编译错误挡住（SubagentSpawnContext Clone、Option description、Workflow toggles 类型），没有作为通过记录。首次尝试新增回归的命令在等待共享 target 锁时取消，`/tmp/grow-plan-confirmation-red.log` 不作为失败反例执行记录。

最终验证使用临时 detached worktree `/tmp/grow-plan-validation`，基线 `e4d82ebd6`，只应用本次九个 Plan 源码/测试文件的 diff（`/tmp/grow-plan-validation.patch`）。设置 `CARGO_INCREMENTAL=0`、`CARGO_BUILD_JOBS=4`、dev/test debug=0 和 `RUST_MIN_STACK=16777216`，隔离编译残留在验证结束后清理。

## 结果

- 隔离执行 `cargo test --locked --lib -p shell -p pager plan -- --test-threads=4`：Pager 169 passed，Shell 132 passed，无失败/忽略。日志 `/tmp/grow-plan-isolated-tests.log`。
- 清除新增测试夹具的 IndexMap deprecated 提示后，最终 Pager `plan` 集再次 169 passed，构建无 warning。日志 `/tmp/grow-plan-isolated-pager-final.log`。
- 同一 Shell 产物先枚举完整名称，再精确运行 `session::behavior::tests::revision_context_preserves_current_feedback_across_restore_and_resubmission`：1 passed。该测试覆盖 Drafting/Amending 的连续反馈、空反馈与恢复。日志 `/tmp/grow-plan-isolated-behavior-final.log`。
- 九个源码/测试文件逐字节与主工作区核对一致；这项隔离验证不代表其他并行修改也已通过。
- 隔离工作区的 `cargo fmt --all -- --check`、主工作区 `git diff --check`、全量 OpenSpec strict 校验及 archive 校验通过。主工作区最后一次全量格式检查遇到其他任务正在编辑的 `actor/mod.rs` 和 `subagent_toggle_reload_tests.rs` 格式差异；没有修改它们。

## 覆盖范围

- Shell：approve/revise/defer/abandon 回执与已确认 Control 一致，重复决定不重复发布，Control 写入失败不发布回执。
- Pager：真实 unconfirmed TaskResult 后收到精确模式回执自动关闭；错误 session/binding/decision/action/feedback 不关闭；第二轮真实 submission 保持独立；相同提交重发仍待确认；新提交可通过按键提交新的修订意见，旧批注保持版本绑定。
- 不以单元回归替代线上模型或真实终端端到端验证；本次未运行线上模型或 PTY。
