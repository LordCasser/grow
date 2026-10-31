# Verification

## Scope and evidence

本次只改 LocalPlan 的未派发转换、SessionActor 的恢复投影与完成检查，以及对应测试和开发者说明。开始时工作区已有大量未提交改动，修改前留存所涉文件快照；没有回退或清理这些改动。

现场报告没有包含 dispatch_failure 的原始 Timeline，因此本次不声称已经导入原会话复现。回归直接重放当前代码允许的旧 Planning 状态，并通过生产 SamplerActor、turn loop 和本地 mock provider 验证实际调用次数。

## Red regressions

- `cargo test --locked -p tool-types --lib undispatched -- --test-threads=1`：两项新增测试失败、既有测试通过。先失败再编辑得到 `Some(1)`；重复失败确认返回 false，未清理遗留来源。
- `cargo test --locked -p shell --lib -- hydration_closes_legacy_replans_after_startup_failure_once unchanged_planning_blockers_stop_sampling_and_goal_recovery --test-threads=1`：旧代码恢复后 pending 仍在；无进展模型循环未在测试期限内退出。日志分别为 `/tmp/grow-undispatched-red.log`、`/tmp/grow-planning-actor-red.log`。

## Scenario coverage

- 先记录启动未派发失败、再修改父项：required/optional × Released/未 Released 都保留原失败原因和来源，后续更新为 Unapplied；活跃关联仍生成 Pending。必需交付没有 Released 时继续阻塞。
- 相同失败回执清理旧 Pending、Dispatching、Received、Consumed，保留 identity/payload、非 terminal 和无结果事实；再次确认幂等，冲突原因拒绝且不修改状态。
- 从旧 Timeline 重建 owner，hydration 先提交闭合事实再投影；重复 hydration 的 Planning seq 不变；显式 Released 后完成检查与 scope 切换通过。
- 普通 final 和 StructuredOutput 在未配置 max-turns、Agent completion recovery 已开启时，相同阻塞第二次返回即停止，实际模型请求均为两次；错误保留具体义务，Runtime Step 已关闭。沿 run_session 相同的结算顺序持久暂停 Goal，idle 仲裁不再派发。
- 处理提示之后合法记录未派发并 Released，下一次 final 正常完成。

## Adversarial review

核对了失败事实与真实执行终态的区别、未派发回执幂等、来源身份和旧 payload、Released 活跃/未知执行、普通及结构化完成路径。审查发现初稿在 StepEnded 之前返回阻塞错误；已移到 Step 关闭、排队控制、预算及 Behavior 切换之后，并增加活动 Step 已关闭的回归断言。

首轮相关 actor/sampler 回归为 65 passed、1 failed：正常完成对照中的命名阻塞 expectation 优先于排队 response，导致它误拦首请求。已将两次响应都登记为按序命名 expectation，修正测试同步；没有放宽生产断言。

## Final checks

- Rust/Cargo 1.93.1，`CARGO_BUILD_JOBS=2`、`RUST_MIN_STACK=16777216`。
- LocalPlan 全部规划测试：23 passed、0 failed；日志 `/tmp/grow-undispatched-green.log`。
- `cargo test --locked -p shell --lib -- session::actor::tests::planning_dispatch_tests:: session::actor::tests::truncation_recovery_tests:: session::actor::turn::planning_completion_gate_tests:: --test-threads=1`：66 passed、0 failed，含两种完成输出的真实请求次数、Goal 暂停和 Step 闭合断言；日志 `/tmp/grow-planning-final.log`。
- 归档前 `openspec validate --all --strict --no-interactive`：17 passed、0 failed。
- 定向 rustfmt `--check` 与全局 `git diff --check` 通过。
- 已归档为 `2026-10-31-fix-undispatched-replanning`；归档后全量 strict：16 passed、0 failed。
- 归档 strict 校验：759 passed、0 failed。
- `cargo clean -p shell -p tools -p tool-types` 清理 4548 个文件、18.2 GiB 编译产物，未修改源码或其他工作区内容。
