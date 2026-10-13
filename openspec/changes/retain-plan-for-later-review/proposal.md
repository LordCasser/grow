## Why

用户需要把生成的计划保留，离开当前审批界面后重新审阅，再明确批准执行。Grow 固定 HEAD 已有 Drafting/AwaitingApproval/Executing/Amending、不可变 artifact、冻结 snapshot 审批，以及无活跃 turn 时的 resume_plan_approval/PlanHandoff；这些不是缺失能力。

当前需补的是脱离活跃 waiter 的候选引用与可发现入口。HEAD 中 Pager 审批外仍回退读 plan.md，Shell 使用 artifacts/plan/<hash>.md；这是未做 PTY 复现的静态差异，须优先统一来源。

## What Changes

- 统一通过后端 canonical plan artifact 引用查看正文，展示正在审阅的版本。
- 区分收起、保留稍后审阅、批准并执行、要求修改和放弃；收起不改变审批决定。
- 在现有 SessionActor/Behavior/Timeline owner 下保留一个当前候选，释放原交互后仍可找回、重新绑定真实审批关系。
- 批准绑定候选提交身份、revision/hash 和稳定决定身份；修改后重审，未知 ACK 核对原决定，不制造第二次执行。
- 沿已有 live ResampleStep 或 idle PlanHandoff 仲裁执行；保持工具权限、Goal、Workflow 和输入队列边界。
- 明确 defer 的 EndTurn/崩溃收尾、独立候选的 Control 校验、不可变决定重试、异步预览与 session 整体回收保护；历史复制不传递执行资格。

## Capabilities

### New Capabilities

无；不新增计划工作流框架。

### Modified Capabilities

- `behavior-goal`：保留候选与 Plan 执行协议分离、重新审阅及版本审批。
- `session-timeline`：候选引用、决定/交接身份、恢复及 artifact 保留。
- `client-surfaces`：canonical 预览、收起和延后审阅入口、接纳反馈。

## Impact

主要入口为 `shell/src/session/behavior.rs`、`actor/tool/preparation.rs`、`actor/tool/mod.rs`、`actor/notification_drain.rs`、chat-state Timeline、Pager plan view 及 /view-plan。实现时更新开发者说明，不提前改主规范。

保留最顶部 task 布局；不采用统一 Dock、Memory 或连贯模型选择。不建立第二套 workflow owner，不把 Markdown 自动转 Rhai，不激活 Goal，不做多计划库、定时执行或长期预授权。

## Evidence and Status

本次仅建立提案，产品代码未实现。上游 post-turn gate 在公开本地后端默认关闭，测试打开不能作为可移植完整执行路径的证据。详见 [固定评审](../record-upstream-review-2bdd1d6/review.md)；[验收矩阵](acceptance.md) P-01～P-24 未执行。
