## Evidence and reproduction

截图来自其他用户，没有其 session Timeline；本次不将本机其他 session 当作来源，也不声称已确认截图的唯一触发条件。

已确认的契约/实现偏差：

- `behavior-goal` 的既有审批身份要求不同计划周期、相同正文的新提交具有不同 submission identity。
- `BehaviorCoordinator::allocate_plan_submission_id` 在 session 内单调递增；Plan runtime 重置后 artifact revision 从当前周期重新开始。
- Shell `admit_plan_handoff_notification` 已将 ordinal source version 设置为 submission ID，`plan_handoff_notification_matches` 也按该 ID 精确匹配。
- Timeline `valid_notification_source_version` 却要求该 ordinal 等于 artifact revision，因此拒绝合法的不同值；live resample 对 admission 错误统一映射成 AcknowledgementLost，产生与截图相同的外层错误。

开发期间用 actor harness 提交 revision=1/submission=7 的真实 Plan receipt，实际在 `receive_notification_durably` 观察到 `Invalid(InvalidNotification)`，发生在持久化之前。最终 Timeline 回归保留同一失败场景及反方向 revision=7/submission=1；Shell 回归再覆盖两个连续 Plan 周期：相同正文、revision 都是1，而 submission 分别为1、2。

## Final change and boundaries

生产只把 PlanHandoff 的 source version 归入现有正 ordinal 校验；合法 hash 和正 artifact revision 的 owner 校验保持。事件形状、source identity、精确当前 handoff 匹配、审批决定及 durable consumption 均不变，无 schema 迁移、重试、查询或第二份状态。

对抗性复核未发现执行授权或身份边界放宽。原有 submission、receipt 去重和消费边界测试随相关回归核对。

另确认了已消费 receipt 再次检查的独立误报路径。其原型和消费归属查询已撤回，仅登记 backlog；不计入本次最终实现或最终测试结果。

## Validation

- 使用项目指定 rustup Rust/Cargo 1.93.1；`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state -p shell --lib plan_ -- --test-threads=4 --quiet` 通过：ChatState 8 项、Shell 99 项，均无失败或忽略。
- 从该次 Cargo 输出确认 ChatState 唯一测试产物 `chat_state-39fbb8a1003fb842`，在 package 目录直接执行同一产物完整 suite：580 passed，0 failed，3 existing ignored（583 项）。未通过再次 Cargo 调用替换枚举/执行产物。
- Timeline 新回归覆盖 revision/submission=(1,7)/(7,1) 的接纳与冷 replay；零 submission、零 revision 和非法 artifact hash 仍拒绝。
- Shell 新回归覆盖两个连续 Plan 周期使用同一正文，revision=1，submission=1/2，各自经过生产 live handoff admission 和 durable consumption；既有 fresh live resample 边界和相关 Plan 回归均通过。
- `cargo fmt --all -- --check`、`git diff --check`、归档前 `openspec validate --all --strict --no-interactive` 通过（17 items）。
- 只修改身份校验和对应回归、说明；保留本轮之前的其他未提交修改。没有构建、安装或发布 CLI，也没有真实 provider/终端集成回归。

## Archive

`openspec archive fix-plan-handoff-submission-identity --yes` 合入一项行为契约。归档后全量严格校验通过（16 items），archive 校验通过（714 items）；`git diff --check` 通过。构建进程停止后已清理本次生成的 Cargo target，约 2.9 GiB；该目录在本轮开始时不存在。
