## ADDED Requirements

### Requirement: Goal completion archives its report with the terminal state

Goal complete SHALL 要求非空完成报告，说明原目标的达成结果、完成依据与验证结果及剩余限制。报告 SHALL 与原 Goal 的 Complete 状态在同一持久控制提交中确认；失败不得发布完成或丢失原状态。原有整体完成审计、必需交付与 authority 门槛 SHALL 保持。blocked SHALL 不接受完成报告。后续用量更新 SHALL 保留报告，编辑重激活 SHALL 清除当前报告；历史缺失报告 SHALL 不被虚构补写。

#### Scenario: Completion includes evidence
- **WHEN** 有效 Active Goal 通过既有完成检查并提供非空报告，控制提交确认
- **THEN** 原 Goal 与完整报告一起成为 Complete，查询、恢复及后续用量更新保留报告原文。

#### Scenario: Missing report or failed commit
- **WHEN** 完成报告缺失或全为空白，或完成提交失败
- **THEN** 返回失败且不发布完成状态，原 Goal 和报告保持不变。

#### Scenario: Report does not bypass completion gates
- **WHEN** 完成操作带有报告但 authority 失效或存在必需交付未闭合
- **THEN** 操作仍被拒绝，报告不获得当前完成结果身份。

#### Scenario: Reactivate by editing
- **WHEN** 用户编辑已完成 Goal 并重激活
- **THEN** 当前 Goal 不再展示旧报告作为新定义的完成依据。

实施入口：`shell/src/session/actor/goal.rs::handle_goal_command`、`shell/src/session/goal_tracker.rs`、`tools/src/implementations/grow_build/update_goal/mod.rs`。
